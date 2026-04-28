// protection.rs - ARCHITECTURE.md §3.3 Path Protection Module
use std::env;
use std::path::PathBuf;

/// Outcome of checking a command against protected paths.
pub enum ProtectionResult {
    /// Command does not touch any protected path — allow freely.
    Allowed,
    /// Command touches a protected path and is in the allowlist — require auth.
    RequiresAuth,
    /// Command touches a protected path but is NOT in the allowlist — block outright.
    Blocked,
}

/// Resolve an argument (which may be relative or absolute) to an absolute path
/// by joining it with `cwd` when relative.
fn resolve_arg(arg: &str, cwd: &PathBuf) -> PathBuf {
    let p = PathBuf::from(arg);
    if p.is_absolute() {
        p
    } else {
        cwd.join(p)
    }
}

/// Returns true if `path` falls under any entry in `protected_paths`.
/// Uses component-aware prefix matching so `/root/test2` does NOT match `/root/test`.
fn is_under_protection(path: &PathBuf, protected_paths: &[String]) -> bool {
    protected_paths
        .iter()
        .any(|p| path.starts_with(PathBuf::from(p)))
}

/// Check whether `cmd` targets a protected path, and if so what action to take.
/// All policy is driven entirely by `protected_paths` and `allowlist` from the
/// configuration file — there are no hardcoded command lists in this function.
///
/// Effective only in `enforcing` mode (caller's responsibility).
///
/// Algorithm:
/// 1. Split the command line into whitespace-delimited tokens.
/// 2. Extract the bare command name (strips any leading path, e.g. `/bin/rm` → `rm`).
/// 3. A command "touches" a protected path if ANY of the following is true:
///    a. An explicit non-flag argument resolves (relative → absolute via cwd) to a
///       path that starts with a protected prefix.
///    b. No path argument was present (or all were flags), AND the current working
///       directory itself is under a protected prefix — the command implicitly
///       operates on the cwd (e.g. `ls`, `cat file`, `touch newfile` with no path).
/// 4. If nothing touches a protected path → `Allowed`.
/// 5. If a protected path IS touched:
///    - Command is in `allowlist`     → `RequiresAuth`  (prompt for password)
///    - Command is NOT in `allowlist` → `Blocked`       (deny immediately)
///    - `allowlist` is empty          → `Blocked`       (secure default)
pub fn check_protected_operation(
    cmd: &str,
    protected_paths: &[String],
    allowlist: &[String],
) -> ProtectionResult {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return ProtectionResult::Allowed;
    }

    // Bare command name: strip any leading path prefix (e.g. /usr/bin/rm → rm).
    let command = PathBuf::from(parts[0])
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(parts[0])
        .to_string();

    // Collect non-flag tokens from the argument list (skip -x / --foo style flags).
    let path_args: Vec<&str> = parts[1..]
        .iter()
        .copied()
        .filter(|t| !t.starts_with('-'))
        .collect();

    // Check (a): any explicit path argument resolves into a protected prefix.
    let explicit_hit = path_args
        .iter()
        .any(|arg| is_under_protection(&resolve_arg(arg, &cwd), protected_paths));

    // Check (b): no path arguments were supplied (or all tokens were flags), but
    // cwd itself is inside a protected directory — the command implicitly targets
    // it.  Examples that reach this branch while inside /root/test:
    //   `ls`            – no args, lists the protected directory
    //   `ls -la`        – only flag args, still lists the protected directory
    //   `cat test_file` – has a path arg, caught by (a) above instead
    let implicit_hit = !explicit_hit
        && path_args.is_empty()
        && is_under_protection(&cwd, protected_paths);

    if !explicit_hit && !implicit_hit {
        return ProtectionResult::Allowed;
    }

    // A protected path is involved — policy decided solely by the allowlist.
    // An empty allowlist means no command is permitted: secure-by-default.
    if allowlist.iter().any(|a| a == &command) {
        ProtectionResult::RequiresAuth
    } else {
        ProtectionResult::Blocked
    }
}