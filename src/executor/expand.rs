// expand.rs - Argument Expansion (Globbing)
//
// SECURITY-CRITICAL CHANGES vs. original:
//
//   1. try_numeric_range() now uses checked arithmetic.  The previous
//      `(hi - lo).unsigned_abs() as usize + 1` panicked in debug and silently
//      wrapped in release on inputs like `{0..9223372036854775807}` because
//      the subtraction itself overflowed.
//
//   2. expand_braces() now refuses absurd ranges before allocating, with the
//      same MAX_EXPANDED_ARGS cap as glob expansion (was previously unbounded
//      until the cap was hit at the `expand_argv` level — which only catches
//      the total, not a single bad token).
//
// NOTE on glob+symlink bypass:
//   `glob` follows symlinks, which means `rm /tmp/lnk_to_etc/*` produces paths
//   like "/tmp/lnk_to_etc/passwd" that lexically miss a "/etc" protection
//   prefix.  This is now handled DOWNSTREAM in protection::resolve_arg, which
//   canonicalizes the longest existing ancestor before the prefix check.
//   We therefore intentionally do NOT post-filter glob results here; doing so
//   would require I/O for every match and would break the "nullglob off"
//   behaviour for non-existent prefixes.

use glob::{glob_with, MatchOptions};
use std::path::PathBuf;

const MAX_EXPANDED_ARGS: usize = 65_536;
const GLOB_CHARS: &[char] = &['*', '?', '['];

#[derive(Debug)]
pub enum ExpandError {
    InvalidPattern { pattern: String, reason: String },
    TooManyArgs { limit: usize, actual: usize },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::InvalidPattern { pattern, reason } => {
                write!(f, "invalid glob pattern '{}': {}", pattern, reason)
            }
            ExpandError::TooManyArgs { limit, actual } => write!(
                f,
                "glob expansion produced {} arguments (limit {})",
                actual, limit
            ),
        }
    }
}

fn match_options() -> MatchOptions {
    MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    }
}

pub fn expand_argv(argv: &[String]) -> Result<Vec<String>, ExpandError> {
    if argv.is_empty() {
        return Ok(Vec::new());
    }

    let mut result: Vec<String> = vec![argv[0].clone()];

    for token in &argv[1..] {
        if !contains_glob_char(token) {
            result.push(token.clone());
            continue;
        }

        let expanded = expand_one(token)?;

        let new_total = result.len().saturating_add(expanded.len());
        if new_total > MAX_EXPANDED_ARGS {
            return Err(ExpandError::TooManyArgs {
                limit: MAX_EXPANDED_ARGS,
                actual: new_total,
            });
        }
        result.extend(expanded);
    }

    Ok(result)
}

fn expand_one(pattern: &str) -> Result<Vec<String>, ExpandError> {
    let opts = match_options();
    let entries = glob_with(pattern, opts).map_err(|e| ExpandError::InvalidPattern {
        pattern: pattern.to_string(),
        reason: e.to_string(),
    })?;

    let mut matches: Vec<String> = Vec::new();

    for entry in entries {
        match entry {
            Ok(path) => match path_to_string(path) {
                Some(s) => matches.push(s),
                None => eprintln!(
                    "dpshell: glob: skipping non-UTF-8 path matched by '{}'",
                    pattern
                ),
            },
            Err(e) => eprintln!("dpshell: glob: {}", e),
        }
    }

    if matches.is_empty() {
        Ok(vec![pattern.to_string()])
    } else {
        matches.sort_unstable();
        Ok(matches)
    }
}

#[inline]
fn contains_glob_char(token: &str) -> bool {
    if token.starts_with('-') {
        return false;
    }
    token.chars().any(|c| GLOB_CHARS.contains(&c))
}

fn path_to_string(path: PathBuf) -> Option<String> {
    path.into_os_string().into_string().ok()
}

// ──────────────────────────────────────────────────────────────────────────────
// Brace expansion  {1..5}  {a,b,c}
// ──────────────────────────────────────────────────────────────────────────────

pub fn expand_braces(token: &str) -> Vec<String> {
    let Some(open) = token.find('{') else {
        return vec![token.to_string()];
    };
    let Some(rel_close) = token[open..].find('}') else {
        return vec![token.to_string()];
    };
    let close = open + rel_close;

    let prefix = &token[..open];
    let inner = &token[open + 1..close];
    let suffix = &token[close + 1..];

    if let Some(result) = try_numeric_range(prefix, inner, suffix) {
        return result;
    }
    if let Some(result) = try_char_range(prefix, inner, suffix) {
        return result;
    }
    if inner.contains(',') {
        return inner
            .split(',')
            .map(|item| format!("{}{}{}", prefix, item, suffix))
            .collect();
    }

    vec![token.to_string()]
}

fn try_numeric_range(prefix: &str, inner: &str, suffix: &str) -> Option<Vec<String>> {
    let (lo_str, hi_str) = inner.split_once("..")?;
    let lo: i64 = lo_str.trim().parse().ok()?;
    let hi: i64 = hi_str.trim().parse().ok()?;

    // ── BUG FIX ──────────────────────────────────────────────────────────────
    // Previously: `(hi - lo).unsigned_abs() as usize + 1`.
    // The subtraction `hi - lo` itself overflowed for inputs like
    //   {-9223372036854775808..9223372036854775807}
    // panicking in debug and silently wrapping in release.
    //
    // Now: use checked arithmetic on the absolute distance, then bound by the
    // global expansion cap.
    let span = (hi as i128 - lo as i128).unsigned_abs(); // up to ~2^64
    let count = span.saturating_add(1);
    if count > MAX_EXPANDED_ARGS as u128 {
        eprintln!(
            "dpshell: brace expansion: range {{{lo}..{hi}}} too large ({count} items, limit {MAX_EXPANDED_ARGS})"
        );
        return None;
    }
    let count = count as usize;

    let range: Box<dyn Iterator<Item = i64>> = if lo <= hi {
        Box::new(lo..=hi)
    } else {
        Box::new((hi..=lo).rev())
    };

    let mut out = Vec::with_capacity(count);
    out.extend(range.map(|n| format!("{}{}{}", prefix, n, suffix)));
    Some(out)
}

fn try_char_range(prefix: &str, inner: &str, suffix: &str) -> Option<Vec<String>> {
    let (lo_str, hi_str) = inner.split_once("..")?;
    let lo = lo_str
        .trim()
        .chars()
        .next()
        .filter(|_| lo_str.trim().len() == 1)?;
    let hi = hi_str
        .trim()
        .chars()
        .next()
        .filter(|_| hi_str.trim().len() == 1)?;

    // chars are u32 ≤ 0x10FFFF; subtraction in i64 cannot overflow.
    let count = (lo as i64 - hi as i64).unsigned_abs() as usize + 1;
    if count > 256 {
        return None;
    }

    let range: Box<dyn Iterator<Item = char>> = if lo <= hi {
        Box::new(lo..=hi)
    } else {
        Box::new((hi..=lo).rev())
    };

    Some(
        range
            .map(|c| format!("{}{}{}", prefix, c, suffix))
            .collect(),
    )
}

pub fn expand_command_argv(argv: &[String]) -> Result<Vec<String>, ExpandError> {
    if argv.is_empty() {
        return Ok(Vec::new());
    }

    let mut after_brace: Vec<String> = vec![argv[0].clone()];
    for token in &argv[1..] {
        let expanded = expand_braces(token);
        // Bound brace expansion at the same global limit to fail fast on absurd
        // input rather than building a multi-MB Vec.
        if after_brace.len().saturating_add(expanded.len()) > MAX_EXPANDED_ARGS {
            return Err(ExpandError::TooManyArgs {
                limit: MAX_EXPANDED_ARGS,
                actual: after_brace.len() + expanded.len(),
            });
        }
        after_brace.extend(expanded);
    }

    expand_argv(&after_brace)
}
