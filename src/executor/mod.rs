pub mod expand;

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

use crate::jobs::{JobManager, JobStatus};
use crate::parser::sanitised_env;
use crate::parser::syntax::{CommandNode, RedirectKind, SimpleCommand};
use crate::security::protection::{check_expanded_argv, ProtectionResult};
use expand::expand_command_argv;
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{close, execve, fork, setpgid, ForkResult, Pid};
use std::ffi::CString;
use std::os::unix::io::RawFd;

const STDIN_FD: RawFd = 0;
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
// ── Fork-bomb rate limiter (thread-local, single-threaded shell) ──────────────
use std::cell::Cell;
use std::time::Instant;

thread_local! {
    /// Timestamp of the start of the current 1-second window.
    static BG_WINDOW_START: Cell<Option<Instant>> = Cell::new(None);
    /// Number of background forks in the current window.
    static BG_FORKS_THIS_WINDOW: Cell<u32> = Cell::new(0);
    /// Total live background children.
    static BG_CHILD_COUNT: Cell<u32> = Cell::new(0);
}

/// Returns `true` if we are under the fork rate / child-count limits and
/// records the fork.  Returns `false` if any limit would be exceeded.
fn bg_fork_allowed() -> bool {
    let now = Instant::now();

    // Reset window counter if the 1-second window has elapsed.
    let window_elapsed = BG_WINDOW_START.with(|s| match s.get() {
        Some(start) => now.duration_since(start).as_secs() >= 1,
        None => true,
    });
    if window_elapsed {
        BG_WINDOW_START.with(|s| s.set(Some(now)));
        BG_FORKS_THIS_WINDOW.with(|c| c.set(0));
    }

    let rate_ok = BG_FORKS_THIS_WINDOW.with(|c| c.get() < BG_FORK_RATE_LIMIT);
    let count_ok = BG_CHILD_COUNT.with(|c| c.get() < BG_CHILD_LIMIT);

    if rate_ok && count_ok {
        BG_FORKS_THIS_WINDOW.with(|c| c.set(c.get() + 1));
        BG_CHILD_COUNT.with(|c| c.set(c.get() + 1));
        true
    } else {
        false
    }
}

/// Call when a background child is reaped.
#[allow(dead_code)]
pub fn bg_child_reaped() {
    BG_CHILD_COUNT.with(|c| c.set(c.get().saturating_sub(1)));
}

/// Hard limits — tuned conservatively so the shell survives a fork bomb
/// while still allowing normal interactive use.
///
/// CALL_DEPTH_LIMIT: maximum number of recursive shell-function activations.
/// A depth of 32 lets legitimate scripts nest comfortably while stopping an
/// infinite `:() { : }; :` after 32 frames instead of blowing the stack.
pub const CALL_DEPTH_LIMIT: u32 = 128;

/// Maximum background forks per second.  A fork bomb like `:|:&` creates
/// O(2^n) children per second; 64/s blocks the geometric growth immediately
/// while still allowing realistic background workloads.
pub const BG_FORK_RATE_LIMIT: u32 = 64;

/// Absolute cap on live background children.  Once hit, every `&` returns
/// an error until children exit.
pub const BG_CHILD_LIMIT: u32 = 256;

pub struct ExecContext<'a> {
    pub protected_paths: &'a [String],
    pub allowlist: &'a [String],
    /// SHA-256 hex of the admin password — for `RequiresAuth` prompts.
    pub password_hash: &'a str,
    /// When `false` the post-expansion audit is skipped entirely.
    pub enforce: bool,
    /// Shell function table: name → body AST node.
    pub functions: &'a std::cell::RefCell<std::collections::HashMap<String, CommandNode>>,
    /// Current shell-function call depth (incremented on each function call).
    pub call_depth: u32,
    /// Shell variables for $VAR expansion (shared ref into DpShell).
    pub shell_vars: &'a std::collections::HashMap<String, String>,
    /// Exit code of the last command (for $?).
    pub last_exit: i32,
    /// Positional parameters for the current function call ($1, $2, ...).
    pub positional_params: &'a [String],
}

impl<'a> ExecContext<'a> {
    /// Convenience: a no-op context that never blocks (disable/permissive).
    pub fn permissive_with_fns(
        functions: &'a std::cell::RefCell<std::collections::HashMap<String, CommandNode>>,
        shell_vars: &'a std::collections::HashMap<String, String>,
        last_exit: i32,
    ) -> Self {
        Self {
            protected_paths: &[],
            allowlist: &[],
            password_hash: "",
            enforce: false,
            functions,
            call_depth: 0,
            shell_vars,
            last_exit,
            positional_params: &[],
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ──────────────────────────────────────────────────────────────────────────────

/// Execute a `CommandNode` tree.  Returns the exit code of the last command.
pub fn execute_node(node: &CommandNode, jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    match node {
        CommandNode::Simple(sc) => {
            // Expand positional params in the command name for function dispatch
            let expanded_name = crate::parser::expand_vars::expand_word_with_params(
                sc.name(), ctx.shell_vars, ctx.last_exit, ctx.positional_params,
            );
            let lookup_name = if expanded_name.is_empty() { sc.name().to_string() } else { expanded_name };

            // Check for bare variable assignment (VAR=value)
            if is_var_assignment_cmd(sc) {
                execute_assignments(sc, ctx, jobs);
                return 0;
            }

            // Check if this resolves to a shell function.
            let func_body = ctx.functions.borrow().get(lookup_name.as_str()).cloned();
            if let Some(body) = func_body {
                if ctx.call_depth >= CALL_DEPTH_LIMIT {
                    eprintln!(
                        "dpshell: {}: maximum function call depth ({}) exceeded — aborting",
                        lookup_name,
                        CALL_DEPTH_LIMIT
                    );
                    return 1;
                }
                let func_args: Vec<String> = sc.args().iter()
                    .map(|a| crate::parser::expand_vars::expand_word(a, ctx.shell_vars, ctx.last_exit))
                    .collect();
                let deeper = ExecContext {
                    call_depth: ctx.call_depth + 1,
                    functions: ctx.functions,
                    protected_paths: ctx.protected_paths,
                    allowlist: ctx.allowlist,
                    password_hash: ctx.password_hash,
                    enforce: ctx.enforce,
                    shell_vars: ctx.shell_vars,
                    last_exit: ctx.last_exit,
                    positional_params: &func_args,
                };
                return execute_node(&body, jobs, &deeper);
            }
            if sc.is_builtin {
                let expanded_args: Vec<String> = sc.args().iter()
                    .map(|a| {
                        let v = crate::parser::expand_vars::expand_word_with_params(
                            a, ctx.shell_vars, ctx.last_exit, ctx.positional_params,
                        );
                        let cs = expand_command_substitutions(&v, jobs, ctx);
                        crate::parser::expand_vars::unescape_dollars(&cs)
                    })
                    .collect();
                return dispatch_builtin(sc.name(), &expanded_args, jobs, ctx);
            }
            execute_simple(sc, jobs, ctx)
        }
        CommandNode::Pipeline(cmds) => execute_pipeline(cmds, jobs, ctx),
        CommandNode::Logical { left, op, right } => {
            let left_code = execute_node(left, jobs, ctx);
            let run_right = match op {
                crate::parser::syntax::LogicOp::Seq => true,
                crate::parser::syntax::LogicOp::And => left_code == 0,
                crate::parser::syntax::LogicOp::Or => left_code != 0,
            };
            if run_right {
                execute_node(right, jobs, ctx)
            } else {
                left_code
            }
        }
        // Background: fork a child that runs the inner node, don't wait.
        CommandNode::Background(inner) => execute_background(inner, jobs, ctx),
        // Compound: execute each node in sequence.
        CommandNode::Compound(nodes) => {
            let mut last = 0;
            for n in nodes {
                let updated = ExecContext { last_exit: last, ..*ctx };
                last = execute_node(n, jobs, &updated);
            }
            last
        }
        CommandNode::FunctionDef { name, body } => {
            ctx.functions
                .borrow_mut()
                .insert(name.clone(), (**body).clone());
            0
        }
        CommandNode::If { cond, then_body, elifs, else_body } => {
            let code = execute_node(cond, jobs, ctx);
            if code == 0 {
                return execute_list(then_body, jobs, ctx);
            }
            for (elif_cond, elif_body) in elifs {
                let ec = execute_node(elif_cond, jobs, ctx);
                if ec == 0 {
                    return execute_list(elif_body, jobs, ctx);
                }
            }
            execute_list(else_body, jobs, ctx)
        }
        CommandNode::For { var, words, body } => {
            let mut last = 0;
            for word in words {
                unsafe { std::env::set_var(var, word); }
                last = execute_list(body, jobs, ctx);
            }
            last
        }
        CommandNode::While { cond, body, redirections } => {
            let saved_stdin = apply_compound_redirections(redirections);
            let mut last = 0;
            loop {
                let code = execute_node(cond, jobs, ctx);
                if code != 0 { break; }
                last = execute_list(body, jobs, ctx);
            }
            restore_fd(saved_stdin, STDIN_FD);
            last
        }
        CommandNode::Until { cond, body, redirections } => {
            let saved_stdin = apply_compound_redirections(redirections);
            let mut last = 0;
            loop {
                let code = execute_node(cond, jobs, ctx);
                if code == 0 { break; }
                last = execute_list(body, jobs, ctx);
            }
            restore_fd(saved_stdin, STDIN_FD);
            last
        }
        CommandNode::Case { word, arms } => {
            let expanded_word = crate::parser::expand_vars::expand_word_with_params(
                word, ctx.shell_vars, ctx.last_exit, ctx.positional_params,
            );
            let mut code = 0;
            for (patterns, body) in arms {
                if patterns.iter().any(|p| {
                    let ep = crate::parser::expand_vars::expand_word_with_params(
                        p, ctx.shell_vars, ctx.last_exit, ctx.positional_params,
                    );
                    crate::parser::expand_vars::simple_glob_match(&ep, &expanded_word)
                }) {
                    code = execute_list(body, jobs, ctx);
                    break;
                }
            }
            code
        }
    }
}

fn execute_list(list: &[CommandNode], jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    let mut last = 0;
    for node in list {
        let updated = ExecContext { last_exit: last, ..*ctx };
        last = execute_node(node, jobs, &updated);
    }
    last
}

fn apply_compound_redirections(redirections: &[crate::parser::syntax::Redirection]) -> Option<RawFd> {
    use std::os::unix::io::IntoRawFd;
    for r in redirections {
        if r.kind == RedirectKind::Input {
            let saved = unsafe { libc::dup(STDIN_FD) };
            if let Ok(file) = std::fs::File::open(&r.target) {
                let raw = file.into_raw_fd();
                unsafe { libc::dup2(raw, STDIN_FD); }
                unsafe { libc::close(raw); }
                return if saved >= 0 { Some(saved) } else { None };
            }
        }
    }
    None
}

fn restore_fd(saved: Option<RawFd>, target_fd: RawFd) {
    if let Some(fd) = saved {
        unsafe { libc::dup2(fd, target_fd); }
        unsafe { libc::close(fd); }
    }
}

fn is_var_assignment_str(s: &str) -> bool {
    if let Some(eq) = s.find('=') {
        let name = &s[..eq];
        !name.is_empty()
            && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

fn is_var_assignment_cmd(sc: &SimpleCommand) -> bool {
    if !is_var_assignment_str(&sc.program) {
        return false;
    }
    sc.argv.iter().all(|w| is_var_assignment_str(w))
}

fn execute_assignments(sc: &SimpleCommand, ctx: &ExecContext, jobs: &mut JobManager) {
    for word in &sc.argv {
        let expanded = crate::parser::expand_vars::expand_word_with_params(
            word, ctx.shell_vars, ctx.last_exit, ctx.positional_params,
        );
        let cmd_expanded = expand_command_substitutions(&expanded, jobs, ctx);
        if let Some((k, v)) = cmd_expanded.split_once('=') {
            unsafe { std::env::set_var(k, v); }
        }
    }
}

fn dispatch_builtin(name: &str, args: &[String], jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    match name {
        ":" | "true" => 0,
        "false" => 1,
        "set" => 0,
        "cd" => match crate::builtins::cd::execute_cd(&args.to_vec()) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("dpshell: cd: {}", e);
                1
            }
        },
        "test" => crate::builtins::builtin_test(args),
        "[" => {
            let stripped: Vec<String> = if args.last().map(String::as_str) == Some("]") {
                args[..args.len()-1].to_vec()
            } else {
                args.to_vec()
            };
            crate::builtins::builtin_test(&stripped)
        }
        "echo" => {
            let text = args.join(" ");
            println!("{}", text);
            0
        }
        "kill" => crate::builtins::builtin_kill(args),
        "umask" => crate::builtins::builtin_umask(args),
        "wait" => {
            crate::builtins::builtin_wait(args);
            0
        }
        "break" => {
            crate::builtins::builtin_break(args);
            0
        }
        "continue" => {
            crate::builtins::builtin_continue(args);
            0
        }
        "shift" => {
            crate::builtins::builtin_shift(args);
            0
        }
        "unset" => {
            for name in args {
                unsafe { std::env::remove_var(name); }
            }
            0
        }
        "export" => {
            for arg in args {
                if let Some((k, v)) = arg.split_once('=') {
                    unsafe { std::env::set_var(k, v); }
                }
            }
            0
        }
        "return" => {
            args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0)
        }
        "read" => {
            let stdin = std::io::stdin();
            use std::io::BufRead;
            let mut line = String::new();
            let var_args: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
            match stdin.lock().read_line(&mut line) {
                Ok(0) => return 1,
                Ok(_) => {}
                Err(_) => return 1,
            }
            if line.ends_with('\n') { line.pop(); }
            if line.ends_with('\r') { line.pop(); }
            if var_args.is_empty() {
                unsafe { std::env::set_var("REPLY", &line); }
            } else if var_args.len() == 1 {
                unsafe { std::env::set_var(var_args[0], &line); }
            } else {
                let words: Vec<&str> = line.splitn(var_args.len(), |c: char| c == ' ' || c == '\t').collect();
                for (i, var_name) in var_args.iter().enumerate() {
                    let val = words.get(i).unwrap_or(&"");
                    unsafe { std::env::set_var(*var_name, val); }
                }
            }
            0
        }
        "command" => {
            if args.first().map(String::as_str) == Some("-v") {
                for cmd_name in &args[1..] {
                    let search_path = std::env::var("PATH").unwrap_or_default();
                    match crate::parser::resolve_binary(cmd_name, &search_path) {
                        Some(p) => { println!("{}", p.display()); }
                        None => {
                            eprintln!("dpshell: command: {}: not found", cmd_name);
                            return 1;
                        }
                    }
                }
                0
            } else if let Some(_cmd_name) = args.first() {
                let rebuilt = args.join(" ");
                match crate::parser::syntax::parse_command_line(&rebuilt) {
                    Ok(node) => execute_node(&node, jobs, ctx),
                    Err(_) => 127,
                }
            } else {
                0
            }
        }
        "printf" => {
            if args.is_empty() { return 0; }
            let format_str = &args[0];
            let fmt_args = &args[1..];
            let mut arg_idx = 0;
            let mut output = String::new();
            let mut chars = format_str.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('n') => output.push('\n'),
                        Some('t') => output.push('\t'),
                        Some('\\') => output.push('\\'),
                        Some(o) => { output.push('\\'); output.push(o); }
                        None => output.push('\\'),
                    }
                } else if c == '%' {
                    match chars.peek() {
                        Some('%') => { chars.next(); output.push('%'); }
                        _ => {
                            let spec = chars.next().unwrap_or('s');
                            let arg = fmt_args.get(arg_idx).map(String::as_str).unwrap_or("");
                            arg_idx += 1;
                            match spec {
                                's' => output.push_str(arg),
                                'd' => output.push_str(&arg.parse::<i64>().unwrap_or(0).to_string()),
                                _ => { output.push('%'); output.push(spec); }
                            }
                        }
                    }
                } else {
                    output.push(c);
                }
            }
            print!("{}", output);
            let _ = std::io::Write::flush(&mut std::io::stdout());
            0
        }
        _ => {
            eprintln!("dpshell: {}: builtin not supported in this context", name);
            1
        }
    }
}

/// Fork and execute `inner` in the background (no wait, no terminal transfer).
fn execute_background(inner: &CommandNode, jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    use nix::unistd::{fork, ForkResult};

    // ── Fork-bomb protection: check rate and child count before forking ───────
    if !bg_fork_allowed() {
        eprintln!(
            "dpshell: background fork limit reached ({}/s or {} children) — try again later",
            BG_FORK_RATE_LIMIT, BG_CHILD_LIMIT
        );
        return 1;
    }

    let _shell_pgid = unsafe { libc::getpgrp() };

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Child: new process group, no terminal ownership, run inner.
            let _ = nix::unistd::setpgid(Pid::from_raw(0), Pid::from_raw(0));
            // Ignore SIGTTIN/SIGTTOU so background reads block gracefully.
            unsafe {
                let _ = nix::sys::signal::signal(
                    nix::sys::signal::Signal::SIGTTIN,
                    nix::sys::signal::SigHandler::SigIgn,
                );
                let _ = nix::sys::signal::signal(
                    nix::sys::signal::Signal::SIGTTOU,
                    nix::sys::signal::SigHandler::SigIgn,
                );
            }
            let mut dummy_jobs =
                crate::jobs::JobManager::new(Pid::from_raw(unsafe { libc::getpgrp() }));
            let _ = execute_node(inner, &mut dummy_jobs, ctx);
            std::process::exit(0);
        }
        Ok(ForkResult::Parent { child }) => {
            let _ = nix::unistd::setpgid(child, child);
            // Build a display string for the job table.
            let display = match inner {
                CommandNode::Simple(sc) => format!("{} &", sc.raw),
                CommandNode::Pipeline(cmds) => format!(
                    "{} &",
                    cmds.iter()
                        .map(|c| c.raw.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
                _ => "<compound> &".to_string(),
            };
            let job_id = jobs.add(child, &display, crate::jobs::JobStatus::Background);
            eprintln!("[{}] {}", job_id, child.as_raw());
            0
        }
        Err(e) => {
            eprintln!("dpshell: fork (background): {}", e);
            1
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Simple command
// ──────────────────────────────────────────────────────────────────────────────

/// Fork + execve a single `SimpleCommand`.
/// Built-ins are never passed here — dispatched in `main.rs` before this call.
pub fn execute_simple(sc: &SimpleCommand, jobs: &mut JobManager, ctx: &ExecContext) -> i32 {
    // ── 0. Variable expansion ($VAR, ${VAR}) ─────────────────────────────────
    let var_expanded_argv =
        crate::parser::expand_vars::expand_argv_with_params(&sc.argv, ctx.shell_vars, ctx.last_exit, ctx.positional_params);

    // ── 0b. Command substitution expansion ($(...)) in each arg ──────────────
    let cmd_sub_argv: Vec<String> = var_expanded_argv.iter()
        .map(|a| expand_command_substitutions(a, jobs, ctx))
        .collect();

    // ── 1. Glob / brace expansion ─────────────────────────────────────────────
    let expanded_argv = match expand_command_argv(&cmd_sub_argv) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("dpshell: {}", e);
            return 1;
        }
    };

    // ── 2. Post-expansion security audit ──────────────────────────────────────
    // Runs in the parent before fork; sees the concrete expanded paths.
    if ctx.enforce {
        match check_expanded_argv(
            sc.name(),
            &expanded_argv,
            ctx.protected_paths,
            ctx.allowlist,
        ) {
            ProtectionResult::Blocked(offender) => {
                eprintln!(
                    "dpshell: \x1b[31;5m[!]\x1b[0m Blocked (post-expand): {}",
                    offender
                );
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
    // If the program wasn't resolved at parse time (no `/`), try resolving now.
    let resolved_program = if sc.program.contains('/') {
        sc.program.clone()
    } else {
        let search_path = std::env::var("PATH").unwrap_or_default();
        match crate::parser::resolve_binary(&sc.program, &search_path) {
            Some(p) => p.to_string_lossy().into_owned(),
            None => {
                eprintln!("dpshell: {}: command not found", sc.program);
                return 127;
            }
        }
    };
    let expanded_sc = SimpleCommand {
        program: resolved_program,
        argv: expanded_argv,
        is_builtin: sc.is_builtin,
        raw: sc.raw.clone(),
        redirections: sc.redirections.clone(),
    };
    let (c_program, c_argv, c_env) = match build_exec_args(&expanded_sc) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("dpshell: encoding error: {}", e);
            return 1;
        }
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
                let _ = signal(Signal::SIGINT, SigHandler::SigDfl);
            }
            apply_redirections(&expanded_sc.redirections);
            let _ = execve(&c_program, &c_argv, &c_env);
            eprintln!(
                "dpshell: exec '{}': {}",
                sc.program,
                nix::errno::Errno::last()
            );
            std::process::exit(127);
        }
        Ok(ForkResult::Parent { child }) => child,
        Err(e) => {
            eprintln!("dpshell: fork: {}", e);
            return 1;
        }
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
            for (r, w) in &pipes {
                let _ = close(*r);
                let _ = close(*w);
            }
            return 1;
        }
        pipes.push((fds[0], fds[1]));
    }

    let shell_pgid = unsafe { libc::getpgrp() };
    let mut child_pids: Vec<Pid> = Vec::with_capacity(n);
    let mut pgid_opt: Option<Pid> = None;

    for (i, sc) in cmds.iter().enumerate() {
        // ── Expand variables for this stage ───────────────────────────────────
        let var_expanded_argv =
            crate::parser::expand_vars::expand_argv_with_params(&sc.argv, ctx.shell_vars, ctx.last_exit, ctx.positional_params);

        // ── Command substitution expansion ───────────────────────────────────
        let cmd_sub_argv: Vec<String> = var_expanded_argv.iter()
            .map(|a| expand_command_substitutions(a, jobs, ctx))
            .collect();

        // ── Expand globs for this stage ───────────────────────────────────────
        let expanded_argv = match expand_command_argv(&cmd_sub_argv) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("dpshell: {}", e);
                cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                return 1;
            }
        };

        // ── Post-expansion audit for this stage ───────────────────────────────
        if ctx.enforce {
            match check_expanded_argv(
                sc.name(),
                &expanded_argv,
                ctx.protected_paths,
                ctx.allowlist,
            ) {
                ProtectionResult::Blocked(offender) => {
                    eprintln!(
                        "dpshell: \x1b[31;5m[!]\x1b[0m Blocked (post-expand): {}",
                        offender
                    );
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

        // Detect whether this stage should be executed in-process (builtin or
        // brace-group compound serialised as `self -c "..."`)
        let is_self_c = sc.argv.len() >= 3 && sc.argv[1] == "-c"
            && sc.argv[0] == sc.program && !sc.is_builtin
            && std::env::current_exe().map_or(false, |p| sc.program == p.to_string_lossy());
        let run_inprocess = sc.is_builtin || is_self_c;

        let child_stdin: Option<RawFd> = if i == 0 { None } else { Some(pipes[i - 1].0) };
        let child_stdout: Option<RawFd> = if i == n - 1 { None } else { Some(pipes[i].1) };
        let pgid_for_child = pgid_opt.unwrap_or(Pid::from_raw(0));

        if run_inprocess {
            // Fork a child that executes the builtin/compound directly
            match unsafe { fork() } {
                Ok(ForkResult::Child) => {
                    let _ = setpgid(Pid::from_raw(0), pgid_for_child);
                    if let Some(r) = child_stdin {
                        unsafe { libc::dup2(r, STDIN_FD); }
                    }
                    if let Some(w) = child_stdout {
                        unsafe { libc::dup2(w, STDOUT_FD); }
                    }
                    for (r, w) in &pipes {
                        let _ = close(*r);
                        let _ = close(*w);
                    }
                    unsafe {
                        let _ = signal(Signal::SIGTSTP, SigHandler::SigDfl);
                        let _ = signal(Signal::SIGTTOU, SigHandler::SigDfl);
                        let _ = signal(Signal::SIGTTIN, SigHandler::SigDfl);
                        let _ = signal(Signal::SIGINT, SigHandler::SigDfl);
                    }
                    apply_redirections(&sc.redirections);

                    let exit_code = if is_self_c {
                        match crate::parser::syntax::parse_command_line(&sc.argv[2]) {
                            Ok(node) => {
                                let mut child_jobs = crate::jobs::JobManager::new(
                                    Pid::from_raw(unsafe { libc::getpgrp() }));
                                execute_node(&node, &mut child_jobs, ctx)
                            }
                            Err(_) => 127,
                        }
                    } else {
                        dispatch_builtin(sc.name(), &expanded_argv[1..].to_vec(), jobs, ctx)
                    };
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    std::process::exit(exit_code);
                }
                Ok(ForkResult::Parent { child }) => {
                    if pgid_opt.is_none() {
                        pgid_opt = Some(child);
                        let _ = setpgid(child, child);
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
            continue;
        }

        let resolved_program = if sc.program.contains('/') {
            sc.program.clone()
        } else {
            let search_path = std::env::var("PATH").unwrap_or_default();
            match crate::parser::resolve_binary(&sc.program, &search_path) {
                Some(p) => p.to_string_lossy().into_owned(),
                None => {
                    eprintln!("dpshell: {}: command not found", sc.program);
                    cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                    return 127;
                }
            }
        };
        let expanded_sc = SimpleCommand {
            program: resolved_program,
            argv: expanded_argv,
            is_builtin: sc.is_builtin,
            raw: sc.raw.clone(),
            redirections: sc.redirections.clone(),
        };
        let (c_program, c_argv, c_env) = match build_exec_args(&expanded_sc) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("dpshell: encoding error in pipeline: {}", e);
                cleanup_pipeline(&pipes, &child_pids, shell_pgid);
                return 1;
            }
        };

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                // Join the pipeline's process group.
                let _ = setpgid(Pid::from_raw(0), pgid_for_child);

                // Wire up stdin/stdout.
                // Safety: pipe fds are valid; STDIN_FD/STDOUT_FD are always open.
                if let Some(r) = child_stdin {
                    unsafe {
                        libc::dup2(r, STDIN_FD);
                    }
                }
                if let Some(w) = child_stdout {
                    unsafe {
                        libc::dup2(w, STDOUT_FD);
                    }
                }

                // Close ALL pipe fds — the child only needs the ones it dup2'd.
                // Keeping any extra write end open would prevent the next reader
                // from ever seeing EOF.
                for (r, w) in &pipes {
                    let _ = close(*r);
                    let _ = close(*w);
                }

                unsafe {
                    let _ = signal(Signal::SIGTSTP, SigHandler::SigDfl);
                    let _ = signal(Signal::SIGTTOU, SigHandler::SigDfl);
                    let _ = signal(Signal::SIGTTIN, SigHandler::SigDfl);
                    let _ = signal(Signal::SIGINT, SigHandler::SigDfl);
                }
                apply_redirections(&expanded_sc.redirections);
                let _ = execve(&c_program, &c_argv, &c_env);
                eprintln!(
                    "dpshell: exec '{}': {}",
                    sc.program,
                    nix::errno::Errno::last()
                );
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
    for (r, w) in &pipes {
        let _ = close(*r);
        let _ = close(*w);
    }

    let pgid = pgid_opt.unwrap();
    let cmd_display = cmds
        .iter()
        .map(|c| c.raw.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
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
                unsafe {
                    libc::tcsetpgrp(STDIN_FD, shell_pgid);
                }
                jobs.update_by_pgid(child_pid, JobStatus::Done(code));
                exit_code = code;
                break;
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                unsafe {
                    libc::tcsetpgrp(STDIN_FD, shell_pgid);
                }
                if sig != Signal::SIGINT {
                    eprintln!();
                }
                jobs.update_by_pgid(child_pid, JobStatus::Done(-1));
                exit_code = 128 + sig as i32;
                break;
            }
            Ok(WaitStatus::Stopped(_, sig)) => {
                unsafe {
                    libc::tcsetpgrp(STDIN_FD, shell_pgid);
                }
                eprintln!("\n[{}] stopped (signal {})", cmd_display, sig as i32);
                jobs.update_by_pgid(child_pid, JobStatus::Stopped);
                exit_code = 128 + sig as i32;
                break;
            }
            Err(nix::Error::EINTR) => continue,
            Err(e) => {
                eprintln!("dpshell: waitpid: {}", e);
                unsafe {
                    libc::tcsetpgrp(STDIN_FD, shell_pgid);
                }
                break;
            }
            Ok(_) => continue,
        }
    }
    unsafe {
        let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
    }
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
    let last_pid = *child_pids.last().unwrap();
    let mut last_exit = 0i32;

    while !remaining.is_empty() {
        match waitpid(Pid::from_raw(-pgid.as_raw()), Some(WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Exited(pid, code)) => {
                remaining.remove(&pid);
                if pid == last_pid {
                    last_exit = code;
                }
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                remaining.remove(&pid);
                if pid == last_pid {
                    last_exit = 128 + sig as i32;
                }
            }
            Ok(WaitStatus::Stopped(_, sig)) => {
                unsafe {
                    libc::tcsetpgrp(STDIN_FD, shell_pgid);
                }
                eprintln!("\n[{}] stopped (signal {})", cmd_display, sig as i32);
                jobs.update_by_pgid(pgid, JobStatus::Stopped);
                unsafe {
                    let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
                }
                return 128 + sig as i32;
            }
            Err(nix::Error::EINTR) => continue,
            Err(nix::Error::ECHILD) => break,
            Err(e) => {
                eprintln!("dpshell: waitpid (pipeline): {}", e);
                break;
            }
            Ok(_) => continue,
        }
    }

    unsafe {
        libc::tcsetpgrp(STDIN_FD, shell_pgid);
    }
    jobs.update_by_pgid(pgid, JobStatus::Done(last_exit));
    unsafe {
        let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
    }
    last_exit
}

fn cleanup_pipeline(pipes: &[(RawFd, RawFd)], pids: &[Pid], shell_pgid: libc::pid_t) {
    for (r, w) in pipes {
        let _ = close(*r);
        let _ = close(*w);
    }
    for &pid in pids {
        let _ = nix::sys::signal::kill(pid, Signal::SIGTERM);
    }
    unsafe {
        libc::tcsetpgrp(STDIN_FD, shell_pgid);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// C-string builder
// ──────────────────────────────────────────────────────────────────────────────

fn build_exec_args(
    sc: &SimpleCommand,
) -> Result<(CString, Vec<CString>, Vec<CString>), std::ffi::NulError> {
    let c_program = CString::new(sc.program.as_str())?;
    let c_argv = sc
        .argv
        .iter()
        .map(|s| CString::new(s.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    let env_pairs = sanitised_env();
    let c_env = env_pairs
        .iter()
        .map(|(k, v)| CString::new(format!("{}={}", k, v)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((c_program, c_argv, c_env))
}

// ──────────────────────────────────────────────────────────────────────────────
// I/O redirection
// ──────────────────────────────────────────────────────────────────────────────

/// Apply all redirections attached to a command.  Called in the child process
/// after fork, after signals are restored, before execve.
///
/// On failure, prints a message to stderr and exits the child process — there
/// is no shell to return to after a failed redirect in a forked child.
fn apply_redirections(redirs: &[crate::parser::syntax::Redirection]) {
    use std::fs::OpenOptions;
    use std::os::unix::io::IntoRawFd;

    for r in redirs {
        let default_fd: i32 = match r.kind {
            RedirectKind::Input | RedirectKind::DupIn => 0,
            _ => 1,
        };
        let fd = r.fd.unwrap_or(default_fd);

        match r.kind {
            RedirectKind::Input => {
                let file = match OpenOptions::new().read(true).open(&r.target) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("dpshell: {}: {}", r.target, e);
                        std::process::exit(1);
                    }
                };
                let raw = file.into_raw_fd();
                unsafe { libc::dup2(raw, fd) };
                unsafe { libc::close(raw) };
            }
            RedirectKind::Output | RedirectKind::Clobber => {
                let file = match OpenOptions::new()
                    .write(true).create(true).truncate(true)
                    .open(&r.target)
                {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("dpshell: {}: {}", r.target, e);
                        std::process::exit(1);
                    }
                };
                let raw = file.into_raw_fd();
                unsafe { libc::dup2(raw, fd) };
                unsafe { libc::close(raw) };
            }
            RedirectKind::Append => {
                let file = match OpenOptions::new()
                    .write(true).create(true).append(true)
                    .open(&r.target)
                {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("dpshell: {}: {}", r.target, e);
                        std::process::exit(1);
                    }
                };
                let raw = file.into_raw_fd();
                unsafe { libc::dup2(raw, fd) };
                unsafe { libc::close(raw) };
            }
            RedirectKind::ReadWrite => {
                let file = match OpenOptions::new()
                    .read(true).write(true).create(true)
                    .open(&r.target)
                {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("dpshell: {}: {}", r.target, e);
                        std::process::exit(1);
                    }
                };
                let raw = file.into_raw_fd();
                unsafe { libc::dup2(raw, fd) };
                unsafe { libc::close(raw) };
            }
            RedirectKind::DupIn | RedirectKind::DupOut => {
                match r.target.parse::<i32>() {
                    Ok(target_fd) => {
                        unsafe { libc::dup2(target_fd, fd) };
                    }
                    Err(_) => {
                        // `>&-` or `<&-` — close the fd.
                        if r.target == "-" {
                            unsafe { libc::close(fd) };
                        } else {
                            // Target is a filename for `>&file` shorthand:
                            // redirect fd to file (same as `>file fd`).
                            let file = match OpenOptions::new()
                                .write(true).create(true).truncate(true)
                                .open(&r.target)
                            {
                                Ok(f) => f,
                                Err(e) => {
                                    eprintln!("dpshell: {}: {}", r.target, e);
                                    std::process::exit(1);
                                }
                            };
                            let raw = file.into_raw_fd();
                            unsafe { libc::dup2(raw, fd) };
                            unsafe { libc::close(raw) };
                        }
                    }
                }
            }
            RedirectKind::Heredoc | RedirectKind::HeredocStrip => {
                // Heredoc body is stored in a temp file by the REPL layer
                // before parsing.  The delimiter stored in r.target is also
                // used as the temp-file key.  Try to open a temp file named
                // after a hash of the delimiter+body.
                //
                // For now, if the target is an actual file path (the common
                // pre-written case), open it as stdin redirect.
                let file = match OpenOptions::new().read(true).open(&r.target) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("dpshell: heredoc: {}: {}", r.target, e);
                        std::process::exit(1);
                    }
                };
                let raw = file.into_raw_fd();
                unsafe { libc::dup2(raw, fd) };
                unsafe { libc::close(raw) };
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Arithmetic evaluation for $((...))
// ──────────────────────────────────────────────────────────────────────────────

pub fn eval_arithmetic_expr(expr: &str, shell_vars: &std::collections::HashMap<String, String>) -> i64 {
    let expr = expr.trim();
    if expr.is_empty() {
        return 0;
    }

    // Try to parse as a plain integer
    if let Ok(n) = expr.parse::<i64>() {
        return n;
    }

    // Variable lookup (env first for latest values, then shell_vars)
    if expr.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && expr.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        let val = std::env::var(expr).ok()
            .or_else(|| shell_vars.get(expr).cloned())
            .unwrap_or_default();
        return val.parse::<i64>().unwrap_or(0);
    }

    // Handle parenthesized expressions
    if expr.starts_with('(') && expr.ends_with(')') {
        return eval_arithmetic_expr(&expr[1..expr.len()-1], shell_vars);
    }

    // Find lowest-precedence operator outside parentheses (right-to-left for left-assoc)
    let mut paren_depth: i32 = 0;
    let bytes = expr.as_bytes();
    let len = bytes.len();

    // Ternary and assignment not needed for tests; handle +, -, *, /, %
    // Scan for + or - (lowest precedence, left-to-right → find rightmost)
    let mut last_add_sub: Option<(usize, u8)> = None;
    let mut last_mul_div: Option<(usize, u8)> = None;
    let mut last_compare: Option<(usize, usize)> = None; // (pos, len of operator)
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'(' => { paren_depth += 1; i += 1; }
            b')' => { paren_depth -= 1; i += 1; }
            b'+' | b'-' if paren_depth == 0 && i > 0 => {
                // Make sure it's not a unary minus at the start or after another operator
                let prev = bytes[i - 1];
                if prev != b'*' && prev != b'/' && prev != b'%' && prev != b'+' && prev != b'-' && prev != b'(' {
                    last_add_sub = Some((i, bytes[i]));
                }
                i += 1;
            }
            b'*' | b'/' | b'%' if paren_depth == 0 => {
                last_mul_div = Some((i, bytes[i]));
                i += 1;
            }
            b'<' | b'>' if paren_depth == 0 => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    last_compare = Some((i, 2));
                    i += 2;
                } else {
                    last_compare = Some((i, 1));
                    i += 1;
                }
            }
            b'=' if paren_depth == 0 && i + 1 < len && bytes[i + 1] == b'=' => {
                last_compare = Some((i, 2));
                i += 2;
            }
            b'!' if paren_depth == 0 && i + 1 < len && bytes[i + 1] == b'=' => {
                last_compare = Some((i, 2));
                i += 2;
            }
            _ => { i += 1; }
        }
    }

    // Evaluate in precedence order: comparisons < add/sub < mul/div
    if let Some((pos, op_len)) = last_compare {
        let left = eval_arithmetic_expr(&expr[..pos], shell_vars);
        let right = eval_arithmetic_expr(&expr[pos + op_len..], shell_vars);
        let op = &expr[pos..pos + op_len];
        return match op {
            "<=" => if left <= right { 1 } else { 0 },
            ">=" => if left >= right { 1 } else { 0 },
            "<" => if left < right { 1 } else { 0 },
            ">" => if left > right { 1 } else { 0 },
            "==" => if left == right { 1 } else { 0 },
            "!=" => if left != right { 1 } else { 0 },
            _ => 0,
        };
    }

    if let Some((pos, op)) = last_add_sub {
        let left = eval_arithmetic_expr(&expr[..pos], shell_vars);
        let right = eval_arithmetic_expr(&expr[pos + 1..], shell_vars);
        return match op {
            b'+' => left + right,
            b'-' => left - right,
            _ => 0,
        };
    }

    if let Some((pos, op)) = last_mul_div {
        let left = eval_arithmetic_expr(&expr[..pos], shell_vars);
        let right = eval_arithmetic_expr(&expr[pos + 1..], shell_vars);
        return match op {
            b'*' => left * right,
            b'/' => if right != 0 { left / right } else { 0 },
            b'%' => if right != 0 { left % right } else { 0 },
            _ => 0,
        };
    }

    0
}

// ──────────────────────────────────────────────────────────────────────────────
// Command substitution  $()  and  ``
// ──────────────────────────────────────────────────────────────────────────────

/// Execute `cmd_text` in a subshell and capture stdout as a String.
/// Returns empty string on any failure (fail-soft for substitutions).
pub fn capture_subshell_output(
    cmd_text: &str,
    jobs: &mut JobManager,
    ctx: &ExecContext,
) -> String {

    let mut pipe_fds = [0i32; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        eprintln!("dpshell: command substitution: pipe failed");
        return String::new();
    }

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            unsafe { libc::close(pipe_fds[0]) };
            unsafe { libc::dup2(pipe_fds[1], STDOUT_FD) };
            unsafe { libc::close(pipe_fds[1]) };

            match crate::parser::syntax::parse_command_line(cmd_text) {
                Ok(node) => {
                    let exit_code = execute_node(&node, jobs, ctx);
                    std::process::exit(exit_code);
                }
                Err(_) => std::process::exit(1),
            }
        }
        Ok(ForkResult::Parent { child }) => {
            unsafe { libc::close(pipe_fds[1]) };

            let mut output = String::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = unsafe { libc::read(pipe_fds[0], buf.as_mut_ptr() as *mut _, buf.len()) };
                if n <= 0 { break; }
                output.push_str(&String::from_utf8_lossy(&buf[..n as usize]));
            }
            unsafe { libc::close(pipe_fds[0]) };

            loop {
                match waitpid(child, None) {
                    Ok(_) => break,
                    Err(nix::Error::EINTR) => continue,
                    Err(_) => break,
                }
            }

            if output.ends_with('\n') {
                output.pop();
                if output.ends_with('\r') {
                    output.pop();
                }
            }
            output
        }
        Err(e) => {
            eprintln!("dpshell: command substitution: fork failed: {}", e);
            String::new()
        }
    }
}

/// Expand `$(...)` and backtick command substitutions in `input`.
pub fn expand_command_substitutions(
    input: &str,
    jobs: &mut JobManager,
    ctx: &ExecContext,
) -> String {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(input.len());
    let mut i = 0;

    while i < len {
        match chars[i] {
            '\'' => {
                result.push('\'');
                i += 1;
                while i < len && chars[i] != '\'' {
                    result.push(chars[i]);
                    i += 1;
                }
                if i < len { result.push('\''); i += 1; }
            }
            '\\' => {
                result.push('\\');
                i += 1;
                if i < len { result.push(chars[i]); i += 1; }
            }
            '$' if i + 1 < len && chars[i + 1] == '(' => {
                if i + 2 < len && chars[i + 2] == '(' {
                    // Arithmetic expansion $((expr))
                    i += 3;
                    let mut depth: u32 = 1;
                    let mut expr = String::new();
                    while i < len && depth > 0 {
                        if chars[i] == '(' && i + 1 < len && chars[i + 1] == '(' {
                            depth += 1; expr.push('('); expr.push('('); i += 2;
                        } else if chars[i] == ')' && i + 1 < len && chars[i + 1] == ')' {
                            depth -= 1;
                            if depth > 0 { expr.push(')'); expr.push(')'); i += 2; }
                            else { i += 2; }
                        } else {
                            expr.push(chars[i]); i += 1;
                        }
                    }
                    let val = eval_arithmetic_expr(&expr, ctx.shell_vars);
                    result.push_str(&val.to_string());
                } else {
                    // Command substitution $(cmd)
                    i += 2;
                    let mut depth: u32 = 1;
                    let mut inner = String::new();
                    while i < len && depth > 0 {
                        match chars[i] {
                            '(' => { depth += 1; inner.push('('); }
                            ')' => { depth -= 1; if depth > 0 { inner.push(')'); } }
                            '\\' => { inner.push('\\'); i += 1; if i < len { inner.push(chars[i]); } }
                            c => inner.push(c),
                        }
                        i += 1;
                    }
                    result.push_str(&capture_subshell_output(&inner, jobs, ctx));
                }
            }
            '`' => {
                i += 1;
                let mut inner = String::new();
                while i < len && chars[i] != '`' {
                    if chars[i] == '\\' && i + 1 < len {
                        match chars[i + 1] {
                            '$' | '`' | '\\' | '\n' => { inner.push(chars[i + 1]); i += 2; continue; }
                            _ => { inner.push('\\'); }
                        }
                    }
                    inner.push(chars[i]);
                    i += 1;
                }
                if i < len { i += 1; }
                result.push_str(&capture_subshell_output(&inner, jobs, ctx));
            }
            '"' => {
                result.push('"');
                i += 1;
                let mut inner = String::new();
                while i < len && chars[i] != '"' {
                    if chars[i] == '$' && i + 1 < len && chars[i + 1] == '(' {
                        if i + 2 < len && chars[i + 2] == '(' {
                            // Arithmetic expansion inside double quotes
                            i += 3;
                            let mut depth: u32 = 1;
                            let mut expr = String::new();
                            while i < len && depth > 0 {
                                if chars[i] == '(' && i + 1 < len && chars[i + 1] == '(' {
                                    depth += 1; expr.push('('); expr.push('('); i += 2;
                                } else if chars[i] == ')' && i + 1 < len && chars[i + 1] == ')' {
                                    depth -= 1;
                                    if depth > 0 { expr.push(')'); expr.push(')'); i += 2; }
                                    else { i += 2; }
                                } else {
                                    expr.push(chars[i]); i += 1;
                                }
                            }
                            let val = eval_arithmetic_expr(&expr, ctx.shell_vars);
                            inner.push_str(&val.to_string());
                        } else {
                            // Command substitution inside double quotes
                            i += 2;
                            let mut depth: u32 = 1;
                            let mut sub = String::new();
                            while i < len && depth > 0 {
                                match chars[i] {
                                    '(' => { depth += 1; sub.push('('); }
                                    ')' => { depth -= 1; if depth > 0 { sub.push(')'); } }
                                    '\\' => { sub.push('\\'); i += 1; if i < len { sub.push(chars[i]); } }
                                    c => sub.push(c),
                                }
                                i += 1;
                            }
                            inner.push_str(&capture_subshell_output(&sub, jobs, ctx));
                        }
                    } else if chars[i] == '`' {
                        i += 1;
                        let mut sub = String::new();
                        while i < len && chars[i] != '`' {
                            if chars[i] == '\\' && i + 1 < len {
                                match chars[i + 1] {
                                    '$' | '`' | '\\' => { sub.push(chars[i + 1]); i += 2; continue; }
                                    _ => { sub.push('\\'); }
                                }
                            }
                            sub.push(chars[i]);
                            i += 1;
                        }
                        if i < len { i += 1; }
                        inner.push_str(&capture_subshell_output(&sub, jobs, ctx));
                    } else if chars[i] == '\\' && i + 1 < len {
                        inner.push('\\');
                        inner.push(chars[i + 1]);
                        i += 2;
                    } else {
                        inner.push(chars[i]);
                        i += 1;
                    }
                }
                result.push_str(&inner);
                if i < len { result.push('"'); i += 1; }
            }
            c => { result.push(c); i += 1; }
        }
    }
    result
}
