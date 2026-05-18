#![allow(dead_code)]
/// Bash-style basic tab completion (enhance_completion = false).
///
/// Returns all matching suggestions and delegates LCP fill and candidate
/// display to reedline's built-in quick_completions / partial_completions
/// and the ColumnarMenu registered in mod.rs.
///
/// This is required because reedline's completion logic only runs inside
/// the menu-handling code paths — without a registered menu the completer
/// is never even called.
use reedline::{Completer, Span, Suggestion};
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::BUILTIN_NAMES;

pub struct BashModeCompleter {
    cached_executables: Vec<String>,
}

impl BashModeCompleter {
    pub fn new() -> Self {
        Self {
            cached_executables: collect_executables(),
        }
    }

    // ── candidate generation ──────────────────────────────────────────────────

    fn complete_commands(&self, prefix: &str, span: Span) -> Vec<Suggestion> {
        self.cached_executables
            .iter()
            .filter(|n| n.starts_with(prefix))
            .map(|n| Suggestion {
                value: n.clone(),
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: true,
            })
            .collect()
    }

    fn complete_paths(prefix: &str, span: Span) -> Vec<Suggestion> {
        let expanded = expand_tilde(prefix);
        let (dir_part, file_prefix) = split_path_prefix(&expanded);
        // Use the ORIGINAL (unexpanded) directory portion for span calculation.
        let (orig_dir_part, _) = split_path_prefix(prefix);

        let base_dir = if dir_part.is_empty() {
            env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(&dir_part)
        };

        if !base_dir.is_dir() {
            return Vec::new();
        }

        // Span only covers the filename portion so replacement inserts only
        // the name, leaving the directory prefix intact (Bash behaviour).
        let file_span = if orig_dir_part.is_empty() {
            span
        } else {
            let file_start = span.start + orig_dir_part.len();
            Span::new(file_start, span.end)
        };

        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base_dir) {
            for entry in entries.flatten() {
                let name = match entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                // Bash: hide dotfiles unless the user typed a leading dot.
                if !file_prefix.starts_with('.') && name.starts_with('.') {
                    continue;
                }
                if !file_prefix.is_empty() && !name.starts_with(file_prefix) {
                    continue;
                }

                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let value = if is_dir {
                    format!("{}/", name)
                } else {
                    name.clone()
                };

                results.push(Suggestion {
                    value,
                    description: None,
                    style: None,
                    extra: None,
                    span: file_span,
                    append_whitespace: !is_dir,
                });
            }
        }

        results.sort_by(|a, b| a.value.cmp(&b.value));
        results
    }

    fn complete_env_var(prefix: &str, span: Span) -> Vec<Suggestion> {
        let var_prefix = &prefix[1..]; // strip leading '$'
        let mut results: Vec<Suggestion> = env::vars()
            .filter(|(k, _)| k.starts_with(var_prefix))
            .map(|(k, _)| Suggestion {
                value: format!("${}", k),
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: true,
            })
            .collect();
        results.sort_by(|a, b| a.value.cmp(&b.value));
        results
    }
}

impl Completer for BashModeCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let (word, word_start) = current_word(line, pos);
        let span = Span::new(word_start, pos);

        if word.is_empty() {
            return Vec::new();
        }

        let results = if word.starts_with('$') {
            Self::complete_env_var(word, span)
        } else if is_command_position(line, pos) {
            if word.contains('/') {
                Self::complete_paths(word, span)
            } else {
                self.complete_commands(word, span)
            }
        } else {
            Self::complete_paths(word, span)
        };

        // Reorder to column-major so reedline's row-major display matches Bash's visual layout.
        reorder_column_major(results)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn collect_executables() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_NAMES.iter().map(|s| s.to_string()).collect();
    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(':') {
            if dir.is_empty() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if is_executable(&path) {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            names.push(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

pub fn current_word(line: &str, pos: usize) -> (&str, usize) {
    let before = &line[..pos];
    let word_start = before
        .rfind(|c: char| c.is_whitespace() || matches!(c, '|' | ';' | '&'))
        .map(|i| i + 1)
        .unwrap_or(0);
    (&line[word_start..pos], word_start)
}

/// Determine whether the cursor is at a command position (i.e. the word
/// being completed should be a command name, not an argument).
///
/// A position is a command position when:
///   - It is the very first word on the line (ignoring leading whitespace).
///   - It immediately follows a command separator (`|`, `;`, `&`).
///
/// The function scans backwards from the cursor, skipping any trailing
/// whitespace, then looks for a command separator.  If one is found, the
/// current word is in a command position.
pub fn is_command_position(line: &str, pos: usize) -> bool {
    let before = &line[..pos];
    let trimmed = before.trim_start();
    if trimmed.is_empty() {
        return true;
    }

    // Scan backwards: skip trailing whitespace, then look for a command
    // separator.  If we find one, the current word is in a command position.
    let mut idx = before.len();
    // Skip trailing whitespace
    while idx > 0 && before.as_bytes()[idx - 1].is_ascii_whitespace() {
        idx -= 1;
    }
    // Check if the character before the trailing whitespace is a separator
    if idx > 0 {
        let c = before.as_bytes()[idx - 1] as char;
        if matches!(c, '|' | ';' | '&') {
            return true;
        }
    }

    // No separator found anywhere before the cursor.
    // We're in a command position only if we're still typing the first
    // word on the line (i.e. no whitespace appears before the current
    // word, ignoring leading whitespace).
    !trimmed.contains(char::is_whitespace)
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        if let Ok(home) = env::var("HOME") {
            if path == "~" {
                return home;
            }
            if path.starts_with("~/") {
                return format!("{}{}", home, &path[1..]);
            }
        }
    }
    path.to_string()
}

pub fn split_path_prefix(prefix: &str) -> (String, &str) {
    match prefix.rfind('/') {
        Some(pos) => (prefix[..=pos].to_string(), &prefix[pos + 1..]),
        None => (String::new(), prefix),
    }
}

fn reorder_column_major(items: Vec<Suggestion>) -> Vec<Suggestion> {
    let n = items.len();
    if n <= 1 {
        return items;
    }
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);
    let max_width = items.iter().map(|s| s.value.len()).max().unwrap_or(1) + 2;
    let cols = (term_width / max_width).max(1);
    let rows = (n + cols - 1) / cols;

    let mut reordered = Vec::with_capacity(n);
    for r in 0..rows {
        for c in 0..cols {
            let idx = c * rows + r;
            if idx < n {
                reordered.push(items[idx].clone());
            }
        }
    }
    reordered
}

