// executor.rs - Command Execution Engine (Phase 4: Glob Expansion)
//
// Accepts a `CommandNode` AST from `syntax.rs` and executes it without any
// `sh -c` wrapper.  Handles:
//
//   Simple commands  — fork + execve
//   Pipelines        — N forks with connected pipe(2) file descriptors
//   Logical chains   — ; && || evaluated by exit-code inspection
//
// Expansion phase
// ───────────────
// Before any fork, each SimpleCommand's argv is passed through
// `expand::expand_command_argv` which runs:
//   1. Brace expansion  {1..5}  {a,b,c}
//   2. Glob expansion   *.log   file?.txt   [abc]*
//
// Expansion happens in the PARENT before fork so that:
//   a) The post-expansion security audit sees concrete paths, not patterns.
//   b) Errors (invalid pattern, too many args) surface before any fork.
//   c) The child's execve receives a fully-materialised argv.
//
// Security audit order
// ────────────────────
//   main.rs  → check_node(unexpanded tree)    pre-fork, catches pipelines/chains
//   executor → expand_command_argv            glob/brace
//   executor → check_expanded_argv            post-expansion wildcard-bypass fix
//   fork + execve
//
// Note on dup2
// ────────────
// nix::unistd::dup2 is gated behind nix's "fs" feature (an unusual grouping).
// We call libc::dup2 directly throughout to avoid adding that feature dep.
// The call is still inside an `unsafe` block with the same soundness guarantee.

use crate::expand::expand_command_argv;
use crate::jobs::{JobManager, JobStatus};
use crate::parser::sanitised_env;
use crate::protection::{check_expanded_argv, ProtectionResult};
use crate::syntax::{CommandNode, SimpleCommand};
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{close, execve, fork, setpgid, ForkResult, Pid};
use std::ffi::CString;
use std::os::unix::io::RawFd;

const STDIN_FD:  RawFd = 0;
const STDOUT_FD: RawFd = 1;

// ──────────────────────────────────────────────────────────────────────────────
// Execution context
// ──────────────────────────────────────────────────────────────────────────────

/// Per-execution configuration threaded through the executor so it can perform
/// the post-expansion security audit without touching global state.
///
/// In `disable`/`permissive` modes, set `enforce = false` — the post-expansion
/// audit short-circuits immediately and never blocks anything.
/// In `enforcing` mode, populate `protected_paths` and `allowlist` from config.
pub struct ExecContext<'a> {
    pub protected_paths: &'a [String],
    pub allowlist:       &'a [String],
    /// SHA-256 hex of the admin password — for `RequiresAuth` prompts.
    pub password_hash:   &'a str,
    /// When `false` the post-expansion audit is skipped entirely.
    pub enforce:         bool,
}

impl<'a> ExecContext<'a> {
    /// Convenience: a no-op context that never blocks (disable/permissive).
    pub fn permissive() -> Self {
        Self {
            protected_paths: &[],
            allowlist:       &[],
            password_hash:   "",
            enforce:         false,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ──────────────────────────────────────────────────────────────────────────────

/// Execute a `CommandNode` tree.  Returns the exit code of the last command.
pub fn execute_node(node: &CommandNode, jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    match node {
        CommandNode::Simple(sc)     => execute_simple(sc, jobs, ctx),
        CommandNode::Pipeline(cmds) => execute_pipeline(cmds, jobs, ctx),
        CommandNode::Logical { left, op, right } => {
            let left_code = execute_node(left, jobs, ctx);
            let run_right = match op {
                crate::syntax::LogicOp::Seq => true,
                crate::syntax::LogicOp::And => left_code == 0,
                crate::syntax::LogicOp::Or  => left_code != 0,
            };
            if run_right { execute_node(right, jobs, ctx) } else { left_code }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Simple command
// ──────────────────────────────────────────────────────────────────────────────

/// Fork + execve a single `SimpleCommand`.
/// Built-ins are never passed here — dispatched in `main.rs` before this call.
pub fn execute_simple(sc: &SimpleCommand, jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    // ── 1. Glob / brace expansion ─────────────────────────────────────────────
    let expanded_argv = match expand_command_argv(&sc.argv) {
        Ok(v)  => v,
        Err(e) => { eprintln!("dpshell: {}", e); return 1; }
    };

    // ── 2. Post-expansion security audit ──────────────────────────────────────
    // Runs in the parent before fork; sees the concrete expanded paths.
    if ctx.enforce {
        match check_expanded_argv(sc.name(), &expanded_argv,
                                  ctx.protected_paths, ctx.allowlist) {
            ProtectionResult::Blocked(offender) => {
                eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m Blocked (post-expand): {}", offender);
                return 1;
            }
            ProtectionResult::RequiresAuth(offender) => {
                eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m Auth required: {}", offender);
                if !crate::authenticate(ctx.password_hash) {
                    eprintln!("dpshell: authorization denied.");
                    return 1;
                }
            }
            ProtectionResult::Allowed => {}
        }
    }

    // ── 3. Build C-string argv with expanded paths ────────────────────────────
    let expanded_sc = SimpleCommand {
        program:    sc.program.clone(),
        argv:       expanded_argv,
        is_builtin: sc.is_builtin,
        raw:        sc.raw.clone(),
    };
    let (c_program, c_argv, c_env) = match build_exec_args(&expanded_sc) {
        Ok(t)  => t,
        Err(e) => { eprintln!("dpshell: encoding error: {}", e); return 1; }
    };

    let shell_pgid = unsafe { libc::getpgrp() };

    // ── 4. Fork ───────────────────────────────────────────────────────────────
    let child_pid = match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Child: new process group, take terminal, restore signals, exec.
            let _ = setpgid(Pid::from_raw(0), Pid::from_raw(0));
            unsafe {
                // Safety: single-threaded child; STDIN_FD (0) is always open.
                let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
                libc::tcsetpgrp(STDIN_FD, libc::getpid());
                let _ = signal(Signal::SIGTSTP, SigHandler::SigDfl);
                let _ = signal(Signal::SIGTTOU, SigHandler::SigDfl);
                let _ = signal(Signal::SIGTTIN, SigHandler::SigDfl);
                let _ = signal(Signal::SIGINT,  SigHandler::SigDfl);
            }
            let _ = execve(&c_program, &c_argv, &c_env);
            eprintln!("dpshell: exec '{}': {}", sc.program, nix::errno::Errno::last());
            std::process::exit(127);
        }
        Ok(ForkResult::Parent { child }) => child,
        Err(e) => { eprintln!("dpshell: fork: {}", e); return 1; }
    };

    // ── 5. Parent: hand off terminal and wait ────────────────────────────────
    let _ = setpgid(child_pid, child_pid); // race-safe: child does the same
    jobs.add(child_pid, &sc.raw, JobStatus::Foreground);
    unsafe {
        let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
        libc::tcsetpgrp(STDIN_FD, child_pid.as_raw());
    }
    let exit_code = wait_foreground_simple(child_pid, shell_pgid, &sc.raw, jobs);
    jobs.reap_done();
    exit_code
}

// ──────────────────────────────────────────────────────────────────────────────
// Pipeline execution
// ──────────────────────────────────────────────────────────────────────────────

/// Execute a pipeline of N ≥ 2 simple commands connected by pipes.
///
/// fd wiring for N=3:  ls | grep foo | wc -l
///   child[0]: stdin=orig,      stdout=pipe[0].w
///   child[1]: stdin=pipe[0].r, stdout=pipe[1].w
///   child[2]: stdin=pipe[1].r, stdout=orig
///   parent:   close all pipe fds after forking all children.
///
/// ALL pipe fds are closed in both parent and child immediately after they are
/// dup2'd or no longer needed.  Failing to close the write end in a reader
/// (or the read end in a writer) is the classic "pipeline hangs forever" bug.
pub fn execute_pipeline(cmds: &[SimpleCommand], jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    let n = cmds.len();
    assert!(n >= 2, "pipeline must have at least 2 commands");

    // Create (n-1) pipes: each element is (read_fd, write_fd).
    let mut pipes: Vec<(RawFd, RawFd)> = Vec::with_capacity(n - 1);
    for _ in 0..(n - 1) {
        let mut fds = [0i32; 2];
        // Safety: `fds` is a valid 2-element array; pipe(2) fills [read, write].
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            eprintln!("dpshell: pipe: {}", nix::errno::Errno::last());
            for (r, w) in &pipes { let _ = close(*r); let _ = close(*w); }
            return 1;
        }
        pipes.push((fds[0], fds[1]));
    }

    let shell_pgid     = unsafe { libc::getpgrp() };
    let mut child_pids: Vec<Pid> = Vec::with_capacity(n);
    let mut pgid_opt:   Option<Pid> = None;

    for (i, sc) in cmds.iter().enumerate() {
        // ── Expand globs for this stage ───────────────────────────────────────
        let expanded_argv = match expand_command_argv(&sc.argv) {
            Ok(v)  => v,
            Err(e) => {
                eprintln!("dpshell: {}", e);
                cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                return 1;
            }
        };

        // ── Post-expansion audit for this stage ───────────────────────────────
        if ctx.enforce {
            match check_expanded_argv(sc.name(), &expanded_argv,
                                      ctx.protected_paths, ctx.allowlist) {
                ProtectionResult::Blocked(offender) => {
                    eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m Blocked (post-expand): {}", offender);
                    cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                    return 1;
                }
                ProtectionResult::RequiresAuth(offender) => {
                    eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m Auth required: {}", offender);
                    if !crate::authenticate(ctx.password_hash) {
                        eprintln!("dpshell: authorization denied.");
                        cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                        return 1;
                    }
                }
                ProtectionResult::Allowed => {}
            }
        }

        let expanded_sc = SimpleCommand {
            program:    sc.program.clone(),
            argv:       expanded_argv,
            is_builtin: sc.is_builtin,
            raw:        sc.raw.clone(),
        };
        let (c_program, c_argv, c_env) = match build_exec_args(&expanded_sc) {
            Ok(t)  => t,
            Err(e) => {
                eprintln!("dpshell: encoding error in pipeline: {}", e);
                cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                return 1;
            }
        };

        let child_stdin:  Option<RawFd> = if i == 0     { None } else { Some(pipes[i-1].0) };
        let child_stdout: Option<RawFd> = if i == n - 1 { None } else { Some(pipes[i].1)   };
        let pgid_for_child = pgid_opt.unwrap_or(Pid::from_raw(0));

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                // Join the pipeline's process group.
                let _ = setpgid(Pid::from_raw(0), pgid_for_child);

                // Wire up stdin/stdout.
                // Safety: pipe fds are valid; STDIN_FD/STDOUT_FD are always open.
                if let Some(r) = child_stdin  { unsafe { libc::dup2(r, STDIN_FD);  } }
                if let Some(w) = child_stdout { unsafe { libc::dup2(w, STDOUT_FD); } }

                // Close ALL pipe fds — the child only needs the ones it dup2'd.
                // Keeping any extra write end open would prevent the next reader
                // from ever seeing EOF.
                for (r, w) in &pipes { let _ = close(*r); let _ = close(*w); }

                unsafe {
                    let _ = signal(Signal::SIGTSTP, SigHandler::SigDfl);
                    let _ = signal(Signal::SIGTTOU, SigHandler::SigDfl);
                    let _ = signal(Signal::SIGTTIN, SigHandler::SigDfl);
                    let _ = signal(Signal::SIGINT,  SigHandler::SigDfl);
                }
                let _ = execve(&c_program, &c_argv, &c_env);
                eprintln!("dpshell: exec '{}': {}", sc.program, nix::errno::Errno::last());
                std::process::exit(127);
            }
            Ok(ForkResult::Parent { child }) => {
                if pgid_opt.is_none() {
                    pgid_opt = Some(child);
                    let _ = setpgid(child, child); // race-safe
                } else {
                    let _ = setpgid(child, pgid_opt.unwrap());
                }
                child_pids.push(child);
            }
            Err(e) => {
                eprintln!("dpshell: fork (pipeline): {}", e);
                cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                return 1;
            }
        }
    }

    // Parent: close ALL pipe fds.  CRITICAL: if the parent holds a write end
    // open, the corresponding reader will never receive EOF and will hang.
    for (r, w) in &pipes { let _ = close(*r); let _ = close(*w); }

    let pgid = pgid_opt.unwrap();
    let cmd_display = cmds.iter().map(|c| c.raw.as_str()).collect::<Vec<_>>().join(" | ");
    jobs.add(pgid, &cmd_display, JobStatus::Foreground);
    unsafe {
        let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
        libc::tcsetpgrp(STDIN_FD, pgid.as_raw());
    }

    let exit_code = wait_pipeline(pgid, &child_pids, shell_pgid, &cmd_display, jobs);
    jobs.reap_done();
    exit_code
}

// ──────────────────────────────────────────────────────────────────────────────
// Wait helpers
// ──────────────────────────────────────────────────────────────────────────────

fn wait_foreground_simple(
    child_pid: Pid,
    shell_pgid: libc::pid_t,
    cmd_display: &str,
    jobs: &mut JobManager,
) -> i32 {
    let mut exit_code = 0;
    loop {
        match waitpid(child_pid, Some(WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Exited(_, code)) => {
                unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
                jobs.update_by_pgid(child_pid, JobStatus::Done(code));
                exit_code = code;
                break;
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
                if sig != Signal::SIGINT { eprintln!(); }
                jobs.update_by_pgid(child_pid, JobStatus::Done(-1));
                exit_code = 128 + sig as i32;
                break;
            }
            Ok(WaitStatus::Stopped(_, sig)) => {
                unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
                eprintln!("\n[{}] stopped (signal {})", cmd_display, sig as i32);
                jobs.update_by_pgid(child_pid, JobStatus::Stopped);
                exit_code = 128 + sig as i32;
                break;
            }
            Err(nix::Error::EINTR) => continue,
            Err(e) => {
                eprintln!("dpshell: waitpid: {}", e);
                unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
                break;
            }
            Ok(_) => continue,
        }
    }
    unsafe { let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn); }
    exit_code
}

fn wait_pipeline(
    pgid: Pid,
    child_pids: &[Pid],
    shell_pgid: libc::pid_t,
    cmd_display: &str,
    jobs: &mut JobManager,
) -> i32 {
    let mut remaining: std::collections::HashSet<Pid> = child_pids.iter().copied().collect();
    let last_pid      = *child_pids.last().unwrap();
    let mut last_exit = 0i32;

    while !remaining.is_empty() {
        match waitpid(Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Exited(pid, code)) => {
                remaining.remove(&pid);
                if pid == last_pid { last_exit = code; }
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                remaining.remove(&pid);
                if pid == last_pid { last_exit = 128 + sig as i32; }
            }
            Ok(WaitStatus::Stopped(_, sig)) => {
                unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
                eprintln!("\n[{}] stopped (signal {})", cmd_display, sig as i32);
                jobs.update_by_pgid(pgid, JobStatus::Stopped);
                unsafe { let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn); }
                return 128 + sig as i32;
            }
            Err(nix::Error::EINTR)  => continue,
            Err(nix::Error::ECHILD) => break,
            Err(e) => { eprintln!("dpshell: waitpid (pipeline): {}", e); break; }
            Ok(_)  => continue,
        }
    }

    unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
    jobs.update_by_pgid(pgid, JobStatus::Done(last_exit));
    unsafe { let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn); }
    last_exit
}

fn cleanup_pipeline(pipes: &[(RawFd, RawFd)], pids: &[Pid], shell_pgid: libc::pid_t) {
    for (r, w) in pipes { let _ = close(*r); let _ = close(*w); }
    for &pid in pids { let _ = nix::sys::signal::kill(pid, Signal::SIGTERM); }
    unsafe { libc::tcsetpgrp(STDIN_FD, shell_pgid); }
}

// ──────────────────────────────────────────────────────────────────────────────
// C-string builder
// ──────────────────────────────────────────────────────────────────────────────

fn build_exec_args(
    sc: &SimpleCommand,
) -> Result<(CString, Vec<CString>, Vec<CString>), std::ffi::NulError> {
    let c_program = CString::new(sc.program.as_str())?;
    let c_argv = sc.argv.iter()
        .map(|s| CString::new(s.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    let env_pairs = sanitised_env();
    let c_env = env_pairs.iter()
        .map(|(k, v)| CString::new(format!("{}={}", k, v)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((c_program, c_argv, c_env))
}