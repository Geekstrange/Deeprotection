// builtins.rs - Shell Built-in Command Implementations
//
// Covers everything in the pending completion table except the already-implemented
// cd (cd.rs), fg/bg/jobs (jobs.rs), and exit/export/unset (main.rs inline).
//
// Built-in dispatch table (handled here):
//   Basic:        :  history  kill
//   Variables:    export  readonly  set  source  unset
//   Execution:    exec  eval  trap  wait
//   Script:       break  continue  return  shift  local  test/[
//   Directory:    dirs  pushd  popd  umask
//   Alias:        alias  unalias
//   Information:  help  command  builtin  type
//
// Each function takes `args: &[String]` and a mutable `ShellState` reference
// that carries the mutable pieces of shell state (aliases, dir stack, vars, …).
// Functions that cannot return a useful exit code return `i32` (0 = success).

use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

// ──────────────────────────────────────────────────────────────────────────────
// Shell state carried into every built-in
// ──────────────────────────────────────────────────────────────────────────────

/// Mutable shell state that built-ins read from or write to.
pub struct ShellState {
    /// Command history — newest entry at the back.
    pub history: Vec<String>,
    /// Named aliases: name → expansion string.
    pub aliases: HashMap<String, String>,
    /// Read-only variable names (set by `readonly`).
    pub readonly_vars: std::collections::HashSet<String>,
    /// Directory stack for pushd/popd (top = back of Vec).
    pub dir_stack: Vec<PathBuf>,
    /// Signal trap table: signal name/number → command string.
    pub traps: HashMap<String, String>,
    /// Shell variables (separate from env — set by `set VAR=val`).
    pub shell_vars: HashMap<String, String>,
    /// Shell function table: name → body AST node.
    pub functions: RefCell<HashMap<String, crate::syntax::CommandNode>>,
}

impl ShellState {
    pub fn new() -> Self {
        Self {
            history:      Vec::new(),
            aliases:      HashMap::new(),
            readonly_vars: std::collections::HashSet::new(),
            dir_stack:    Vec::new(),
            traps:        HashMap::new(),
            shell_vars:   HashMap::new(),
            functions:    RefCell::new(HashMap::new()),
        }
    }

    /// Push a raw input line onto the history, skipping blank lines and
    /// consecutive duplicates (matches bash `ignoreboth` behaviour).
    pub fn push_history(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() { return; }
        if self.history.last().map(|l| l.as_str()) == Some(line) { return; }
        self.history.push(line.to_string());
    }
}

impl Default for ShellState {
    fn default() -> Self { Self::new() }
}

// ──────────────────────────────────────────────────────────────────────────────
// Basic Operations
// ──────────────────────────────────────────────────────────────────────────────

/// `:` — null command; always succeeds.
pub fn builtin_colon(_args: &[String]) -> i32 {
    0
}

/// `history [N]` — print command history, optionally last N entries.
pub fn builtin_history(args: &[String], state: &ShellState) -> i32 {
    let entries = &state.history;
    let start = if let Some(n_str) = args.first() {
        match n_str.parse::<usize>() {
            Ok(n) => entries.len().saturating_sub(n),
            Err(_) => {
                eprintln!("dpshell: history: {}: numeric argument required", n_str);
                return 1;
            }
        }
    } else {
        0
    };

    for (i, cmd) in entries[start..].iter().enumerate() {
        println!("{:5}  {}", start + i + 1, cmd);
    }
    0
}

/// `kill [-SIGNAL] PID…` — send a signal to processes.
///
/// Signal can be specified as:
///   -9          → SIGKILL
///   -KILL       → SIGKILL
///   -SIGKILL    → SIGKILL
///   (omitted)   → SIGTERM
pub fn builtin_kill(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("dpshell: kill: usage: kill [-SIGNAL] PID...");
        return 1;
    }

    let (signal, pid_args) = parse_kill_signal(args);
    let signal = match signal {
        Ok(s)  => s,
        Err(e) => { eprintln!("dpshell: kill: {}", e); return 1; }
    };

    let mut rc = 0;
    for pid_str in pid_args {
        match pid_str.parse::<i32>() {
            Ok(pid) => {
                if let Err(e) = nix::sys::signal::kill(Pid::from_raw(pid), signal) {
                    eprintln!("dpshell: kill: ({}) - {}", pid, e);
                    rc = 1;
                }
            }
            Err(_) => {
                eprintln!("dpshell: kill: {}: arguments must be process or job IDs", pid_str);
                rc = 1;
            }
        }
    }
    rc
}

fn parse_kill_signal<'a>(args: &'a [String]) -> (Result<Signal, String>, &'a [String]) {
    if args[0].starts_with('-') {
        let sig_str = args[0].trim_start_matches('-')
                              .trim_start_matches("SIG");
        let signal = sig_str.parse::<i32>()
            .ok()
            .and_then(|n| Signal::try_from(n).ok())
            .or_else(|| signal_from_name(sig_str));
        match signal {
            Some(s) => (Ok(s), &args[1..]),
            None    => (Err(format!("{}: invalid signal specification", args[0])), &args[1..]),
        }
    } else {
        (Ok(Signal::SIGTERM), args)
    }
}

fn signal_from_name(name: &str) -> Option<Signal> {
    match name.to_ascii_uppercase().as_str() {
        "HUP"  | "1"  => Some(Signal::SIGHUP),
        "INT"  | "2"  => Some(Signal::SIGINT),
        "QUIT" | "3"  => Some(Signal::SIGQUIT),
        "KILL" | "9"  => Some(Signal::SIGKILL),
        "TERM" | "15" => Some(Signal::SIGTERM),
        "STOP" | "19" => Some(Signal::SIGSTOP),
        "CONT" | "18" => Some(Signal::SIGCONT),
        "USR1" | "10" => Some(Signal::SIGUSR1),
        "USR2" | "12" => Some(Signal::SIGUSR2),
        _             => None,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Variables & Environment
// ──────────────────────────────────────────────────────────────────────────────

/// `export [NAME[=VALUE]…]` — set env vars, optionally with values.
/// With no args, prints all currently exported environment variables.
pub fn builtin_export(args: &[String], state: &ShellState) -> i32 {
    if args.is_empty() {
        // Print all env vars in `declare -x NAME="VALUE"` format (bash compat).
        let mut pairs: Vec<(String, String)> = env::vars().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in pairs {
            println!("declare -x {}=\"{}\"", k, v.replace('"', "\\\""));
        }
        return 0;
    }

    let mut rc = 0;
    for arg in args {
        if let Some((k, v)) = arg.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: export: {}: readonly variable", k);
                rc = 1;
                continue;
            }
            unsafe { env::set_var(k, v) };
        } else {
            // `export NAME` without a value — export whatever is already in env.
            // If it's a shell variable, promote it.
            // (No-op if already exported; env vars are always inherited.)
        }
    }
    rc
}

/// `readonly [NAME[=VALUE]…]` — mark variables as immutable.
/// With no args, prints all read-only variables.
pub fn builtin_readonly(args: &[String], state: &mut ShellState) -> i32 {
    if args.is_empty() {
        let mut names: Vec<&String> = state.readonly_vars.iter().collect();
        names.sort();
        for name in names {
            let val = env::var(name)
                .or_else(|_| state.shell_vars.get(name).cloned().ok_or(()))
                .unwrap_or_default();
            println!("declare -r {}=\"{}\"", name, val);
        }
        return 0;
    }

    for arg in args {
        let name = if let Some((k, v)) = arg.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: readonly: {}: readonly variable", k);
                continue;
            }
            unsafe { env::set_var(k, v) };
            k.to_string()
        } else {
            arg.clone()
        };
        state.readonly_vars.insert(name);
    }
    0
}

/// `set [NAME=VALUE…]` — set shell variables or print all variables.
///
/// Note: full `set` with option flags (-e, -x, …) is out of scope for this
/// implementation.  We handle the most common interactive form: `set VAR=VAL`.
pub fn builtin_set(args: &[String], state: &mut ShellState) -> i32 {
    if args.is_empty() {
        // Print all shell vars and env vars.
        let mut pairs: Vec<(String, String)> = env::vars().collect();
        for (k, v) in &state.shell_vars {
            if !pairs.iter().any(|(ek, _)| ek == k) {
                pairs.push((k.clone(), v.clone()));
            }
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in pairs {
            println!("{}={}", k, shell_quote(&v));
        }
        return 0;
    }

    let mut rc = 0;
    for arg in args {
        if let Some((k, v)) = arg.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: set: {}: readonly variable", k);
                rc = 1;
                continue;
            }
            state.shell_vars.insert(k.to_string(), v.to_string());
        } else {
            eprintln!("dpshell: set: {}: not in NAME=VALUE form", arg);
            rc = 1;
        }
    }
    rc
}

/// `unset NAME…` — delete shell variables and env vars.
pub fn builtin_unset(args: &[String], state: &mut ShellState) -> i32 {
    let mut rc = 0;
    for name in args {
        if state.readonly_vars.contains(name.as_str()) {
            eprintln!("dpshell: unset: {}: cannot unset: readonly variable", name);
            rc = 1;
            continue;
        }
        unsafe { env::remove_var(name) };
        state.shell_vars.remove(name);
    }
    rc
}

/// `source FILE [ARGS…]` / `. FILE [ARGS…]` — execute a script file in the
/// current shell context.
///
/// Lines are read from FILE and fed through the parse+execute pipeline.
/// The caller (main.rs) must handle the actual execution; this function
/// just reads and returns the lines.
pub fn read_source_file(path: &str) -> Result<Vec<String>, String> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_string).collect())
        .map_err(|e| format!("{}: {}", path, e))
}

// ──────────────────────────────────────────────────────────────────────────────
// Execution & Debugging
// ──────────────────────────────────────────────────────────────────────────────

/// `exec COMMAND [ARGS…]` — replace the shell process with COMMAND.
///
/// If COMMAND is not found this returns an error string; on success it never
/// returns (the process is replaced).
pub fn builtin_exec(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        // `exec` with no args is a no-op in bash (just applies redirections).
        return Ok(());
    }

    use crate::parser::resolve_binary;
    let search_path = env::var("PATH").unwrap_or_default();
    let program = resolve_binary(&args[0], &search_path)
        .ok_or_else(|| format!("{}: command not found", args[0]))?;

    let c_program = std::ffi::CString::new(program.to_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let c_argv: Vec<std::ffi::CString> = args.iter()
        .map(|s| std::ffi::CString::new(s.as_str()).unwrap())
        .collect();
    let env_pairs = crate::parser::sanitised_env();
    let c_env: Vec<std::ffi::CString> = env_pairs.iter()
        .map(|(k, v)| std::ffi::CString::new(format!("{}={}", k, v)).unwrap())
        .collect();

    nix::unistd::execve(&c_program, &c_argv, &c_env)
        .map_err(|e| format!("{}: {}", args[0], e))?;
    Ok(()) // unreachable after successful execve
}

/// `eval ARGS…` — join args with spaces and return the string for the caller
/// to re-parse and execute.  The caller is responsible for the actual execution.
pub fn builtin_eval(args: &[String]) -> String {
    args.join(" ")
}

/// `trap [ACTION SIGNAL…]` — register or display signal handlers.
///
/// Stores handlers in `state.traps`.  The caller's signal-delivery loop
/// checks `state.traps` on each iteration.
///
/// `trap`         — print current traps
/// `trap - SIG`   — reset SIG to default
/// `trap '' SIG`  — ignore SIG
/// `trap CMD SIG` — run CMD when SIG is received
pub fn builtin_trap(args: &[String], state: &mut ShellState) -> i32 {
    if args.is_empty() {
        // Print all registered traps.
        let mut entries: Vec<(&String, &String)> = state.traps.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        for (sig, action) in entries {
            println!("trap -- {} {}", shell_quote(action), sig);
        }
        return 0;
    }

    if args.len() < 2 {
        eprintln!("dpshell: trap: usage: trap [ACTION] SIGNAL...");
        return 1;
    }

    let action = &args[0];
    for sig in &args[1..] {
        if action == "-" {
            state.traps.remove(sig);
        } else {
            state.traps.insert(sig.clone(), action.clone());
        }
    }
    0
}

/// `wait [PID…]` — wait for background processes to finish.
///
/// With no args, waits for all children.  Returns the exit status of the
/// last waited process.
pub fn builtin_wait(args: &[String]) -> i32 {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

    if args.is_empty() {
        // Wait for all children.
        loop {
            match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) | Err(nix::Error::ECHILD) => break,
                _ => {}
            }
        }
        return 0;
    }

    let mut last_status = 0i32;
    for pid_str in args {
        match pid_str.parse::<i32>() {
            Ok(pid) => {
                loop {
                    match waitpid(Pid::from_raw(pid), None) {
                        Ok(WaitStatus::Exited(_, code))    => { last_status = code; break; }
                        Ok(WaitStatus::Signaled(_, s, _))  => { last_status = 128 + s as i32; break; }
                        Err(nix::Error::EINTR)             => continue,
                        Err(e) => {
                            eprintln!("dpshell: wait: {}: {}", pid, e);
                            last_status = 127;
                            break;
                        }
                        _ => continue,
                    }
                }
            }
            Err(_) => {
                eprintln!("dpshell: wait: {}: not a valid PID", pid_str);
                last_status = 1;
            }
        }
    }
    last_status
}

// ──────────────────────────────────────────────────────────────────────────────
// Script Control
// ──────────────────────────────────────────────────────────────────────────────

/// Signals that `break` or `continue` should propagate out of the executor.
/// Since dpshell doesn't have loop constructs yet, these are no-ops that
/// print a warning when called outside a loop.
pub fn builtin_break(args: &[String]) -> i32 {
    let _n: usize = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    eprintln!("dpshell: break: only meaningful inside a loop");
    0
}

pub fn builtin_continue(args: &[String]) -> i32 {
    let _n: usize = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    eprintln!("dpshell: continue: only meaningful inside a loop");
    0
}

/// `shift [N]` — remove the first N positional parameters.
/// In dpshell's interactive mode there are no positional parameters, so
/// this is a no-op that reports success.
pub fn builtin_shift(_args: &[String]) -> i32 {
    0
}

/// `local NAME[=VALUE]…` — declare local variables.
/// In an interactive shell there is no function scope, so `local` behaves
/// exactly like `set` (stores in shell_vars).
pub fn builtin_local(args: &[String], state: &mut ShellState) -> i32 {
    builtin_set(args, state)
}

/// `test EXPR` / `[ EXPR ]` — evaluate a conditional expression.
///
/// Supports the most common forms used in scripts and interactively:
///   File tests:   -e -f -d -r -w -x -s -L -z (string) -n (string)
///   String:       S1 = S2  S1 != S2  -z S  -n S
///   Integer:      N1 -eq/-ne/-lt/-le/-gt/-ge N2
///   Logical:      ! EXPR  EXPR -a EXPR  EXPR -o EXPR
pub fn builtin_test(args: &[String]) -> i32 {
    // Strip surrounding `[` `]` if present.
    let args: Vec<&str> = {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        if a.first() == Some(&"[") && a.last() == Some(&"]") {
            a[1..a.len()-1].to_vec()
        } else {
            a
        }
    };

    match eval_test_expr(&args) {
        true  => 0,
        false => 1,
    }
}

fn eval_test_expr(args: &[&str]) -> bool {
    match args {
        [] => false,
        ["!",  rest @ ..] => !eval_test_expr(rest),
        [a, "-a", b @ ..] => eval_test_expr(&[a]) && eval_test_expr(b),
        [a, "-o", b @ ..] => eval_test_expr(&[a]) || eval_test_expr(b),

        // File tests
        ["-e", f] => std::path::Path::new(f).exists(),
        ["-f", f] => std::path::Path::new(f).is_file(),
        ["-d", f] => std::path::Path::new(f).is_dir(),
        ["-L", f] | ["-h", f] => std::fs::symlink_metadata(f)
            .map(|m| m.file_type().is_symlink()).unwrap_or(false),
        ["-r", f] => file_test_mode(f, 0o444),
        ["-w", f] => file_test_mode(f, 0o222),
        ["-x", f] => file_test_mode(f, 0o111),
        ["-s", f] => std::fs::metadata(f).map(|m| m.len() > 0).unwrap_or(false),

        // String tests
        ["-z", s] => s.is_empty(),
        ["-n", s] => !s.is_empty(),
        [a, "=",  b] => a == b,
        [a, "!=", b] => a != b,
        [a, "<",  b] => a < b,
        [a, ">",  b] => a > b,

        // Integer comparisons
        [a, op, b] => {
            if let (Ok(n1), Ok(n2)) = (a.parse::<i64>(), b.parse::<i64>()) {
                match *op {
                    "-eq" => n1 == n2,
                    "-ne" => n1 != n2,
                    "-lt" => n1 <  n2,
                    "-le" => n1 <= n2,
                    "-gt" => n1 >  n2,
                    "-ge" => n1 >= n2,
                    _ => { eprintln!("dpshell: test: {}: unknown operator", op); false }
                }
            } else {
                eprintln!("dpshell: test: integer expression expected");
                false
            }
        }

        // Single string: true if non-empty
        [s] => !s.is_empty(),

        _ => { eprintln!("dpshell: test: too many arguments"); false }
    }
}

fn file_test_mode(path: &str, mode: u32) -> bool {
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & mode != 0)
        .unwrap_or(false)
}

// ──────────────────────────────────────────────────────────────────────────────
// Directory Stack
// ──────────────────────────────────────────────────────────────────────────────

/// `dirs [-v]` — print the directory stack.
pub fn builtin_dirs(args: &[String], state: &ShellState) -> i32 {
    let verbose = args.iter().any(|a| a == "-v");
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("?"));

    // Stack: cwd is always the implicit top.
    let mut stack = vec![cwd];
    stack.extend(state.dir_stack.iter().rev().cloned());

    if verbose {
        for (i, dir) in stack.iter().enumerate() {
            println!("{:2}  {}", i, tilde_collapse(dir));
        }
    } else {
        let parts: Vec<String> = stack.iter().map(|d| tilde_collapse(d)).collect();
        println!("{}", parts.join("  "));
    }
    0
}

/// `pushd [DIR]` — push DIR (or swap top two entries) onto the directory stack.
pub fn builtin_pushd(args: &[String], state: &mut ShellState) -> i32 {
    let cwd = match env::current_dir() {
        Ok(p)  => p,
        Err(e) => { eprintln!("dpshell: pushd: {}", e); return 1; }
    };

    match args.first().map(String::as_str) {
        None => {
            // No arg: swap top two entries (like bash).
            if let Some(top) = state.dir_stack.pop() {
                state.dir_stack.push(cwd.clone());
                if let Err(e) = env::set_current_dir(&top) {
                    eprintln!("dpshell: pushd: {}: {}", top.display(), e);
                    state.dir_stack.pop();
                    state.dir_stack.push(cwd);
                    return 1;
                }
            } else {
                eprintln!("dpshell: pushd: no other directory");
                return 1;
            }
        }
        Some(dir) => {
            state.dir_stack.push(cwd);
            if let Err(e) = env::set_current_dir(dir) {
                eprintln!("dpshell: pushd: {}: {}", dir, e);
                state.dir_stack.pop();
                return 1;
            }
        }
    }

    builtin_dirs(&[], state)
}

/// `popd` — pop the top directory and change to it.
pub fn builtin_popd(_args: &[String], state: &mut ShellState) -> i32 {
    match state.dir_stack.pop() {
        None => {
            eprintln!("dpshell: popd: directory stack empty");
            1
        }
        Some(dir) => {
            if let Err(e) = env::set_current_dir(&dir) {
                eprintln!("dpshell: popd: {}: {}", dir.display(), e);
                state.dir_stack.push(dir);
                return 1;
            }
            builtin_dirs(&[], state)
        }
    }
}

/// `umask [MASK]` — get or set the file creation mask.
pub fn builtin_umask(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None => {
            // Print current umask in octal.
            let mask = unsafe { libc::umask(0) };
            unsafe { libc::umask(mask); } // restore
            println!("{:04o}", mask);
            0
        }
        Some(mask_str) => {
            match u32::from_str_radix(mask_str.trim_start_matches('0'), 8) {
                Ok(mask) if mask <= 0o777 => {
                    unsafe { libc::umask(mask as libc::mode_t); }
                    0
                }
                _ => {
                    eprintln!("dpshell: umask: {}: invalid octal mask", mask_str);
                    1
                }
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Alias
// ──────────────────────────────────────────────────────────────────────────────

/// `alias [NAME[=VALUE]…]` — define or list aliases.
pub fn builtin_alias(args: &[String], state: &mut ShellState) -> i32 {
    if args.is_empty() {
        let mut entries: Vec<(&String, &String)> = state.aliases.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        for (name, val) in entries {
            println!("alias {}={}", name, shell_quote(val));
        }
        return 0;
    }

    for arg in args {
        if let Some((name, val)) = arg.split_once('=') {
            state.aliases.insert(name.to_string(), val.to_string());
        } else {
            // Print specific alias.
            match state.aliases.get(arg.as_str()) {
                Some(val) => println!("alias {}={}", arg, shell_quote(val)),
                None      => { eprintln!("dpshell: alias: {}: not found", arg); }
            }
        }
    }
    0
}

/// `unalias NAME…` / `unalias -a`
pub fn builtin_unalias(args: &[String], state: &mut ShellState) -> i32 {
    if args.first().map(String::as_str) == Some("-a") {
        state.aliases.clear();
        return 0;
    }
    let mut rc = 0;
    for name in args {
        if state.aliases.remove(name).is_none() {
            eprintln!("dpshell: unalias: {}: not found", name);
            rc = 1;
        }
    }
    rc
}

// ──────────────────────────────────────────────────────────────────────────────
// Information
// ──────────────────────────────────────────────────────────────────────────────

/// `type NAME…` — show how each name would be interpreted.
pub fn builtin_type(args: &[String], state: &ShellState) -> i32 {
    let mut rc = 0;
    for name in args {
        if ALL_BUILTINS.contains(&name.as_str()) {
            println!("{} is a shell builtin", name);
        } else if let Some(expansion) = state.aliases.get(name.as_str()) {
            println!("{} is aliased to `{}'", name, expansion);
        } else {
            let path = env::var("PATH").unwrap_or_default();
            match crate::parser::resolve_binary(name, &path) {
                Some(p) => println!("{} is {}", name, p.display()),
                None    => {
                    eprintln!("dpshell: type: {}: not found", name);
                    rc = 1;
                }
            }
        }
    }
    rc
}

/// `command [-v] NAME [ARGS…]` — bypass aliases, execute external binary.
///
/// `-v`  → print the path of NAME without executing it.
/// Otherwise: returns the args to execute (caller handles actual execution).
pub fn builtin_command_v(name: &str) -> Option<PathBuf> {
    let path = env::var("PATH").unwrap_or_default();
    crate::parser::resolve_binary(name, &path)
}

/// `builtin NAME [ARGS…]` — execute NAME as a built-in, ignoring aliases.
/// Returns the args for the caller to dispatch through the built-in table.
/// The name resolution itself is just the identity here.
#[allow(dead_code)]
pub fn is_shell_builtin(name: &str) -> bool {
    ALL_BUILTINS.contains(&name)
}

/// `help [PATTERN]` — show help for built-in commands.
pub fn builtin_help(args: &[String]) -> i32 {
    let filter = args.first().map(String::as_str).unwrap_or("");

    let topics: &[(&str, &str)] = &[
        (":",        "Null command; always exits 0."),
        ("alias",    "alias [name[=value]...]  Define or display aliases."),
        ("bg",       "bg [jobspec]  Resume a job in the background."),
        ("break",    "break [n]  Exit from a loop."),
        ("builtin",  "builtin cmd [args]  Execute cmd as a built-in."),
        ("cd",       "cd [-L|-P] [dir]  Change the working directory."),
        ("command",  "command [-v] name  Execute name bypassing aliases."),
        ("continue", "continue [n]  Skip to the next loop iteration."),
        ("dirs",     "dirs [-v]  Display the directory stack."),
        ("eval",     "eval [args...]  Execute args as a command."),
        ("exec",     "exec [cmd [args]]  Replace the shell with cmd."),
        ("exit",     "exit [n]  Exit the shell with status n."),
        ("export",   "export [name[=value]...]  Mark names for export."),
        ("fg",       "fg [jobspec]  Resume a job in the foreground."),
        ("help",     "help [pattern]  Show help for built-in commands."),
        ("history",  "history [n]  Display command history."),
        ("jobs",     "jobs  List active background/stopped jobs."),
        ("kill",     "kill [-SIG] pid...  Send a signal to processes."),
        ("local",    "local [name[=value]...]  Declare local variables."),
        ("popd",     "popd  Remove top directory from the stack and cd."),
        ("pushd",    "pushd [dir]  Push dir onto the directory stack."),
        ("readonly", "readonly [name[=value]...]  Mark variables read-only."),
        ("return",   "return [n]  Exit from a function."),
        ("set",      "set [name=value...]  Set shell variables."),
        ("shift",    "shift [n]  Shift positional parameters."),
        ("source",   "source file [args]  Execute file in current shell."),
        ("test",     "test expr  Evaluate a conditional expression."),
        ("trap",     "trap [action signal...]  Manage signal handlers."),
        ("type",     "type name...  Describe how each name would be used."),
        ("umask",    "umask [mask]  Set/print the file creation mask."),
        ("unalias",  "unalias [-a] [name...]  Remove aliases."),
        ("unset",    "unset [name...]  Unset variables."),
        ("wait",     "wait [pid...]  Wait for background processes."),
        (".",        "source FILE  Execute FILE in the current shell."),
        ("[",        "[ expr ]  Evaluate expr (same as test)."),
    ];

    let mut printed = 0;
    for (name, desc) in topics {
        if filter.is_empty() || name.contains(filter) || desc.contains(filter) {
            println!("{}", desc);
            printed += 1;
        }
    }

    if printed == 0 && !filter.is_empty() {
        eprintln!("dpshell: help: no help topics match `{}'", filter);
        return 1;
    }
    0
}

// ──────────────────────────────────────────────────────────────────────────────
// Complete built-ins registry (used by type, is_shell_builtin)
// ──────────────────────────────────────────────────────────────────────────────

pub const ALL_BUILTINS: &[&str] = &[
    ":", "alias", "bg", "bind", "break", "builtin", "cd", "command",
    "continue", "dirs", "eval", "exec", "exit", "export", "fg",
    "help", "history", "jobs", "kill", "local", "popd", "pushd",
    "readonly", "return", "set", "shift", "source", "test", "trap",
    "type", "umask", "unalias", "unset", "wait", ".", "[",
];

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Wrap a string in single quotes if it contains shell-special characters.
fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Replace the home directory prefix with `~` for display.
fn tilde_collapse(path: &PathBuf) -> String {
    if let Ok(home) = env::var("HOME") {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}