// protection.rs - ARCHITECTURE.md §3.3 Path Protection Module
//
// SECURITY-CRITICAL CHANGES vs. original:
//
//   1. resolve_arg() now resolves the longest existing prefix via canonicalize(),
//      then re-attaches the (possibly non-existent) tail.  This catches:
//        * path traversal:    rm /tmp/../etc/shadow   →  /etc/shadow
//        * symlink redirects: rm /tmp/lnk_to_etc/foo  →  /etc/foo
//        * glob-via-symlink:  rm /tmp/lnk_to_etc/*    →  /etc/<expanded>
//      For paths whose parent does not exist, falls back to lexical normalization.
//
//   2. The argument filter now also inspects `--option=VALUE` forms.  Tools like
//      `tar --file=/etc/shadow` or `dd of=/etc/shadow` previously bypassed the
//      check because the entire token starts with `-` or contains `=`.
//
//   3. env::current_dir() failure is now FAIL-CLOSED in enforcing mode (returns
//      Blocked) instead of silently using "." which never matched any protected
//      prefix.

use crate::syntax::{CommandNode, SimpleCommand, flatten_commands};
use std::env;
use std::path::{Component, Path, PathBuf};

// ──────────────────────────────────────────────────────────────────────────────
// Result type
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ProtectionResult {
    Allowed,
    RequiresAuth(String),
    Blocked(String),
}

// ──────────────────────────────────────────────────────────────────────────────
// Tree-level audit  (called from main.rs, unexpanded argv)
// ──────────────────────────────────────────────────────────────────────────────

pub fn check_node(
    node: &CommandNode,
    protected_paths: &[String],
    allowlist: &[String],
) -> ProtectionResult {
    // FAIL-CLOSED on cwd error: if we cannot determine cwd, we cannot
    // safely audit relative paths against protected prefixes.
    let cwd = match env::current_dir() {
        Ok(p)  => p,
        Err(e) => {
            return ProtectionResult::Blocked(
                format!("(cwd unavailable: {} — failing closed)", e)
            );
        }
    };
    let leaves = flatten_commands(node);
    let mut needs_auth: Option<String> = None;

    for sc in leaves {
        match check_simple(sc, protected_paths, allowlist, &cwd) {
            ProtectionResult::Blocked(name) => return ProtectionResult::Blocked(name),
            ProtectionResult::RequiresAuth(name) => {
                if needs_auth.is_none() { needs_auth = Some(name); }
            }
            ProtectionResult::Allowed => {}
        }
    }

    match needs_auth {
        Some(name) => ProtectionResult::RequiresAuth(name),
        None       => ProtectionResult::Allowed,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Post-expansion audit  (called from executor.rs, concrete expanded argv)
// ──────────────────────────────────────────────────────────────────────────────

pub fn check_expanded_argv(
    program_name: &str,
    expanded_argv: &[String],
    protected_paths: &[String],
    allowlist: &[String],
) -> ProtectionResult {
    if protected_paths.is_empty() {
        return ProtectionResult::Allowed;
    }

    // FAIL-CLOSED on cwd error.
    let cwd = match env::current_dir() {
        Ok(p)  => p,
        Err(e) => {
            return ProtectionResult::Blocked(
                format!("(cwd unavailable: {} — failing closed)", e)
            );
        }
    };

    // Collect every token that could be a path: positional arguments OR the
    // VALUE side of `--option=VALUE` style flags.
    let path_args: Vec<&str> = expanded_argv.iter()
        .skip(1)
        .filter_map(|t| extract_path_arg(t))
        .collect();

    let explicit_hit = path_args.iter()
        .any(|arg| is_under_protection(&resolve_arg(arg, &cwd), protected_paths));

    let implicit_hit = !explicit_hit
        && path_args.is_empty()
        && is_under_protection(&cwd, protected_paths);

    if !explicit_hit && !implicit_hit {
        return ProtectionResult::Allowed;
    }

    let display = expanded_argv.join(" ");
    if allowlist.iter().any(|a| a == program_name) {
        ProtectionResult::RequiresAuth(display)
    } else {
        ProtectionResult::Blocked(display)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Per-SimpleCommand audit helper (used by check_node)
// ──────────────────────────────────────────────────────────────────────────────

fn check_simple(
    sc: &SimpleCommand,
    protected_paths: &[String],
    allowlist: &[String],
    cwd: &Path,
) -> ProtectionResult {
    if sc.argv.is_empty() || protected_paths.is_empty() {
        return ProtectionResult::Allowed;
    }

    let command = sc.name().to_string();

    let path_args: Vec<&str> = sc.args().iter()
        .filter_map(|s| extract_path_arg(s.as_str()))
        .collect();

    let explicit_hit = path_args.iter()
        .any(|arg| is_under_protection(&resolve_arg(arg, cwd), protected_paths));

    let implicit_hit = !explicit_hit
        && path_args.is_empty()
        && is_under_protection(cwd, protected_paths);

    if !explicit_hit && !implicit_hit {
        return ProtectionResult::Allowed;
    }

    if allowlist.iter().any(|a| a == &command) {
        ProtectionResult::RequiresAuth(sc.raw.clone())
    } else {
        ProtectionResult::Blocked(sc.raw.clone())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Argument classification
// ──────────────────────────────────────────────────────────────────────────────

/// Decide whether a token might denote a filesystem path that the audit
/// should inspect.  Returns the path-bearing slice, or None for pure flags
/// (e.g. `-f`, `--force`) that carry no path payload.
///
/// Handles three common forms:
///   "foo/bar"            → Some("foo/bar")               (positional)
///   "--file=/etc/shadow" → Some("/etc/shadow")           (long-option=value)
///   "of=/etc/shadow"     → Some("/etc/shadow")           (key=value, dd-style)
///   "--force"            → None                          (no path payload)
///   "-f"                 → None                          (short flag)
fn extract_path_arg(token: &str) -> Option<&str> {
    if token.is_empty() {
        return None;
    }

    // Long option with embedded value: `--name=VALUE`.
    if token.starts_with("--") {
        if let Some(eq) = token.find('=') {
            let value = &token[eq + 1..];
            // Only treat as path if the value looks like one.
            if value_looks_like_path(value) {
                return Some(value);
            }
        }
        return None;
    }

    // Short flag: `-f`, `-rf`, etc. — never a path.
    if token.starts_with('-') {
        return None;
    }

    // Bare key=value (dd, makeinfo, …): treat the value as a candidate path.
    if let Some(eq) = token.find('=') {
        let key = &token[..eq];
        let value = &token[eq + 1..];
        // Conservative: only re-route if both the key is a plain identifier
        // and the value looks like a path. Otherwise keep the whole token
        // (e.g. `FOO=bar` env-var-style assignment that we still want to
        // audit verbatim against any literal protected prefix).
        if key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && value_looks_like_path(value)
        {
            return Some(value);
        }
    }

    Some(token)
}

#[inline]
fn value_looks_like_path(v: &str) -> bool {
    !v.is_empty() && (v.starts_with('/') || v.contains('/') || v.starts_with('.'))
}

// ──────────────────────────────────────────────────────────────────────────────
// Path resolution & normalization
// ──────────────────────────────────────────────────────────────────────────────

/// Resolve an argument to an absolute, traversal-safe PathBuf for protection
/// checks.  Strategy:
///
///   1. Make the path absolute by joining with cwd if needed.
///   2. Walk the path from the deepest existing ancestor; canonicalize that
///      ancestor (resolving symlinks), then re-attach the trailing components
///      that do not exist on disk.
///   3. If no ancestor can be canonicalized, fall back to lexical
///      normalization (collapses `.` and `..` without filesystem access).
///
/// This catches both `..` traversal AND symlinked directory redirects, while
/// still handling the common case of operating on files that do not yet exist
/// (e.g. `touch /etc/newfile`).
pub(crate) fn resolve_arg(arg: &str, cwd: &Path) -> PathBuf {
    let raw = PathBuf::from(arg);
    let abs = if raw.is_absolute() { raw } else { cwd.join(raw) };

    // Walk up to find the deepest ancestor that exists, and canonicalize it.
    let mut ancestor = abs.as_path();
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(real) => {
                let mut out = real;
                for seg in tail.iter().rev() {
                    out.push(seg);
                }
                return out;
            }
            Err(_) => match ancestor.parent() {
                Some(parent) => {
                    if let Some(name) = ancestor.file_name() {
                        tail.push(name);
                    }
                    ancestor = parent;
                }
                None => break,
            },
        }
    }

    // No ancestor canonicalized — fall back to lexical normalization.
    lexically_normalize(&abs)
}

/// Pure-string normalization: collapse `.` and `..` without touching the
/// filesystem.  Conservative — does not resolve symlinks (cannot, without I/O).
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {} // skip "."
            Component::ParentDir => {
                // pop unless that would take us above the root
                let popped = out.pop();
                if !popped {
                    out.push("..");
                }
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

fn is_under_protection(path: &Path, protected_paths: &[String]) -> bool {
    protected_paths.iter().any(|p| path.starts_with(Path::new(p)))
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_normalize_removes_dotdot() {
        let p = lexically_normalize(Path::new("/tmp/../etc/shadow"));
        assert_eq!(p, PathBuf::from("/etc/shadow"));
    }

    #[test]
    fn lex_normalize_keeps_above_root_marker_for_relative() {
        let p = lexically_normalize(Path::new("a/../../b"));
        assert_eq!(p, PathBuf::from("../b"));
    }

    #[test]
    fn extract_path_arg_handles_long_opt_eq() {
        assert_eq!(extract_path_arg("--file=/etc/shadow"), Some("/etc/shadow"));
        assert_eq!(extract_path_arg("--count=5"), None);
        assert_eq!(extract_path_arg("--force"), None);
        assert_eq!(extract_path_arg("-rf"), None);
    }

    #[test]
    fn extract_path_arg_handles_dd_style() {
        assert_eq!(extract_path_arg("of=/etc/shadow"), Some("/etc/shadow"));
        assert_eq!(extract_path_arg("count=5"), Some("count=5"));
    }

    #[test]
    fn extract_path_arg_passes_positionals() {
        assert_eq!(extract_path_arg("/etc/passwd"), Some("/etc/passwd"));
        assert_eq!(extract_path_arg("foo.txt"), Some("foo.txt"));
    }

    #[test]
    fn dotdot_traversal_is_caught() {
        let cwd = PathBuf::from("/");
        let resolved = resolve_arg("/tmp/../etc/shadow", &cwd);
        // canonicalize() of "/etc" should succeed on a Unix host.
        assert!(resolved.starts_with("/etc"),
            "resolved={:?}", resolved);
    }
}
