// parser.rs - True Shell Lexer & Binary Resolution
//
// ParsedCommand and parse_input are superseded by syntax::parse_command_line
// for the main execution path, but are retained for:
//   • unit tests (see #[cfg(test)] below)
//   • future single-command API callers
// The #[allow(dead_code)] attributes silence warnings while keeping the API.
#![allow(dead_code)]

use std::env;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────────

/// A fully parsed, ready-to-inspect command.
///
/// The binary path is always absolute after `parse_input` succeeds, so security
/// checks downstream can trust `argv[0]` without re-resolving it.
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// Absolute path of the binary (e.g. `/usr/bin/ls`).
    /// For built-ins this is set to the bare name (`cd`, `exit`, `export`).
    pub program: String,

    /// Full argument vector including `argv[0]` (the program name as typed).
    /// `argv[0]` is always the user-facing token, NOT the resolved absolute path,
    /// so that programs that inspect their own name (e.g. `busybox`) behave correctly.
    pub argv: Vec<String>,

    /// Whether this command is a built-in handled entirely inside dpshell.
    /// Kept for external consumers (e.g. plugins, tests) that may inspect it.
    #[allow(dead_code)]
    pub is_builtin: bool,

    /// The original, un-tokenised input string — kept for logging and display.
    pub raw: String,
}

impl ParsedCommand {
    /// Convenience: the bare command name (last component of `program`).
    pub fn name(&self) -> &str {
        Path::new(&self.program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.program)
    }

    /// All tokens after `argv[0]`, i.e. the actual arguments.
    pub fn args(&self) -> &[String] {
        if self.argv.is_empty() {
            &[]
        } else {
            &self.argv[1..]
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Parse errors
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ParseError {
    /// The input was empty or contained only whitespace.
    Empty,
    /// `shlex` rejected the input (e.g. unterminated quote).
    Lex(String),
    /// The requested command was not found on PATH.
    NotFound(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty input"),
            ParseError::Lex(s) => write!(f, "parse error: {}", s),
            ParseError::NotFound(cmd) => write!(f, "{}: command not found", cmd),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in registry
// ──────────────────────────────────────────────────────────────────────────────

/// Commands that are handled entirely within dpshell (no fork/exec).
const BUILTINS: &[&str] = &["cd", "exit", "export", "unset", "history"];

fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

// ──────────────────────────────────────────────────────────────────────────────
// PATH resolution
// ──────────────────────────────────────────────────────────────────────────────

/// Resolve `token` to an absolute binary path using the supplied `search_path`
/// string (colon-separated, like `$PATH`).
///
/// Security properties:
/// - If `token` already contains a `/`, it is treated as a literal path and
///   returned only if it is executable; no PATH search is performed.
/// - Only regular files that are executable are accepted.
/// - Symlinks are followed (the OS does this anyway on exec).
pub fn resolve_binary(token: &str, search_path: &str) -> Option<PathBuf> {
    let p = Path::new(token);

    // Absolute or relative-with-slash: validate directly, no PATH search.
    if token.contains('/') {
        if is_executable(p) {
            return Some(p.to_path_buf());
        }
        return None;
    }

    // Plain name: walk PATH entries left-to-right.
    for dir in search_path.split(':').filter(|d| !d.is_empty()) {
        let candidate = Path::new(dir).join(token);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Returns `true` if `path` is a regular file (or symlink to one) that is
/// executable by the current process.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Main parsing entry-point
// ──────────────────────────────────────────────────────────────────────────────

/// Lex `input`, resolve the binary, and return a `ParsedCommand`.
///
/// Steps:
/// 1. `shlex::split` — handles quotes, escapes, whitespace.
/// 2. Built-in check — no PATH resolution needed; `program` = bare name.
/// 3. PATH resolution — absolute binary path stored in `program`.
///
/// Returns `Err(ParseError::NotFound)` when the binary cannot be found; the
/// caller should print the message and continue the REPL without calling exec.
pub fn parse_input(input: &str) -> Result<ParsedCommand, ParseError> {
    let raw = input.to_string();

    // Step 1: lex
    let tokens = shlex::split(input)
        .ok_or_else(|| ParseError::Lex("unterminated quote or invalid escape".to_string()))?;

    if tokens.is_empty() {
        return Err(ParseError::Empty);
    }

    let command_token = &tokens[0];

    // Step 2: built-ins bypass PATH resolution entirely.
    if is_builtin(command_token) {
        return Ok(ParsedCommand {
            program: command_token.clone(),
            argv: tokens,
            is_builtin: true,
            raw,
        });
    }

    // Step 3: resolve binary.
    let search_path = env::var("PATH").unwrap_or_default();
    let resolved = resolve_binary(command_token, &search_path)
        .ok_or_else(|| ParseError::NotFound(command_token.clone()))?;

    Ok(ParsedCommand {
        program: resolved.to_string_lossy().into_owned(),
        argv: tokens,
        is_builtin: false,
        raw,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Environment sanitisation
// ──────────────────────────────────────────────────────────────────────────────

/// Dangerous variables that should never be inherited by child processes
/// unless the user explicitly re-sets them.
///
/// These are classic privilege-escalation vectors in setuid/setgid contexts.
const DANGEROUS_ENV_VARS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "LD_DEBUG",
    "DYLD_INSERT_LIBRARIES",   // macOS equivalent of LD_PRELOAD
    "DYLD_LIBRARY_PATH",
    "PYTHONPATH",
    "RUBYLIB",
    "PERL5LIB",
    "NODE_PATH",
    "IFS",                      // Historically exploited in setuid shell scripts
];

/// Return a sanitised copy of the current environment, with dangerous
/// variables stripped.  This is applied to every child process spawned by
/// `executor.rs`.
pub fn sanitised_env() -> Vec<(String, String)> {
    env::vars()
        .filter(|(k, _)| !DANGEROUS_ENV_VARS.contains(&k.as_str()))
        .collect()
}

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_cd_recognised() {
        let cmd = parse_input("cd /tmp").unwrap();
        assert!(cmd.is_builtin);
        assert_eq!(cmd.program, "cd");
        assert_eq!(cmd.args(), &["/tmp"]);
    }

    #[test]
    fn empty_input_is_error() {
        assert!(matches!(parse_input("   "), Err(ParseError::Empty)));
    }

    #[test]
    fn unknown_command_is_not_found() {
        assert!(matches!(
            parse_input("__dpshell_nonexistent_cmd__"),
            Err(ParseError::NotFound(_))
        ));
    }

    #[test]
    fn quoted_argument_is_single_token() {
        // "echo hello world" tokenises into ["echo", "hello world"]
        let cmd = parse_input(r#"echo "hello world""#).unwrap();
        assert_eq!(cmd.args(), &["hello world"]);
    }

    #[test]
    fn resolve_absolute_path() {
        // /bin/sh or /usr/bin/sh must exist on any Unix system.
        let resolved = resolve_binary("sh", "/bin:/usr/bin");
        assert!(resolved.is_some());
        assert!(resolved.unwrap().is_absolute());
    }

    #[test]
    fn sanitised_env_strips_ld_preload() {
        unsafe { env::set_var("LD_PRELOAD", "/evil.so") };
        let env = sanitised_env();
        assert!(env.iter().all(|(k, _)| k != "LD_PRELOAD"));
        unsafe { env::remove_var("LD_PRELOAD") };
    }
}