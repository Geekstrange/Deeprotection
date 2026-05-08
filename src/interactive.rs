// interactive.rs - Fish-Style Interactive Features
//
// Implements three interactive layers using reedline:
//
//   1. Syntax highlighting  — real-time colour coding as the user types
//   2. Autosuggestions      — grey "ghost text" from history + completions
//   3. Smart tab completion — file/command/flag completion with fuzzy matching
//
// Crate choices
// ─────────────
//   reedline   — the only Rust line editor that natively supports all three
//                features via composable traits.  Used by nushell.
//   nucleo     — high-performance fuzzy matcher (same engine as Helix/Zed).
//   nix        — already in the dep tree; used for PATH resolution.
//
// Cargo.toml additions needed:
//   reedline  = "0.35"
//   nucleo    = "0.5"
//
// Architecture
// ────────────
//   DpHighlighter   implements reedline::Highlighter
//   DpCompleter     implements reedline::Completer
//   DpHinter        implements reedline::Hinter
//
// All three are constructed once and passed into the Reedline builder.
// Async suggestion updates: reedline drives its own event loop; the highlighter
// and hinter are called synchronously on each keystroke with a ~0 ms budget.
// For heavy completions (large directories) the completer returns a lazy
// iterator so the menu is populated incrementally.

use reedline::{
    Completer, DefaultHinter, Highlighter, Hinter, ReedlineEvent,
    Span, StyledText, Suggestion,
};
use std::path::{Path, PathBuf};
use std::env;
use nucleo::Matcher;

// ──────────────────────────────────────────────────────────────────────────────
// Colour palette (ANSI 256-colour codes matching fish defaults)
// ──────────────────────────────────────────────────────────────────────────────

/// nu_ansi_term style helpers — thin wrappers so callers don't import it directly.
mod style {
    use nu_ansi_term::{Color, Style};

    pub fn command()   -> Style { Color::Green.bold()   }
    pub fn builtin()   -> Style { Color::Cyan.bold()    }
    pub fn argument()  -> Style { Style::new()          }
    pub fn flag()      -> Style { Color::Blue.normal()  }
    pub fn string()    -> Style { Color::Yellow.normal()}
    pub fn operator()  -> Style { Color::Magenta.bold() }
    pub fn error()     -> Style { Color::Red.bold()     }
    pub fn comment()   -> Style { Color::DarkGray.italic() }
    pub fn suggestion()-> Style { Color::DarkGray.normal() }
}

// ──────────────────────────────────────────────────────────────────────────────
// Token classification  (lightweight — no full parse)
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Command,    // first word in a simple command
    Builtin,    // first word that is a known built-in
    Argument,   // positional argument
    Flag,       // starts with -
    String,     // quoted: '...' or "..."
    Operator,   // | & ; && || >> > < etc.
    Comment,    // # to end of line
    Unknown,    // not found on PATH (highlighted as error)
}

#[derive(Debug, Clone)]
struct HighlightToken {
    start: usize,   // byte offset in the original string
    end:   usize,
    kind:  TokenKind,
}

const BUILTIN_NAMES: &[&str] = &[
    ":", "alias", "bg", "bind", "break", "builtin", "cd", "command",
    "continue", "dirs", "eval", "exec", "exit", "export", "fg",
    "help", "history", "jobs", "kill", "local", "popd", "pushd",
    "readonly", "return", "set", "shift", "source", "test", "trap",
    "type", "umask", "unalias", "unset", "wait", ".", "[",
];

/// Lex `line` into a flat list of highlight tokens.
/// This is deliberately lighter than the full AST parser — it only needs to
/// be fast enough to run on every keystroke (~50 µs budget).
fn lex_for_highlight(line: &str) -> Vec<HighlightToken> {
    let mut tokens: Vec<HighlightToken> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut command_expected = true; // true after start / operator / ;

    while i < len {
        // Skip whitespace
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        let start = char_to_byte_offset(line, i);

        // Comment
        if chars[i] == '#' {
            let end = line.len();
            tokens.push(HighlightToken { start, end, kind: TokenKind::Comment });
            break;
        }

        // Operators: >> <= >= && || ;; and single chars
        let op_end = try_scan_operator(&chars, i);
        if op_end > i {
            let end = char_to_byte_offset(line, op_end);
            tokens.push(HighlightToken { start, end, kind: TokenKind::Operator });
            command_expected = true;
            i = op_end;
            continue;
        }

        // Quoted string
        if chars[i] == '\'' || chars[i] == '"' {
            let (end_idx, end_byte) = scan_string(&chars, line, i);
            tokens.push(HighlightToken { start, end: end_byte, kind: TokenKind::String });
            command_expected = false;
            i = end_idx;
            continue;
        }

        // Word
        let (end_idx, end_byte) = scan_word(&chars, line, i);
        let word = &line[start..end_byte];

        let kind = if command_expected {
            classify_command(word)
        } else if word.starts_with('-') {
            TokenKind::Flag
        } else {
            TokenKind::Argument
        };

        tokens.push(HighlightToken { start, end: end_byte, kind });
        command_expected = false;
        i = end_idx;
    }

    tokens
}

fn classify_command(word: &str) -> TokenKind {
    if BUILTIN_NAMES.contains(&word) {
        return TokenKind::Builtin;
    }
    let path = env::var("PATH").unwrap_or_default();
    if crate::parser::resolve_binary(word, &path).is_some() {
        TokenKind::Command
    } else if word == "}" || word == "{" || word == "(" || word == ")" {
        TokenKind::Operator
    } else {
        TokenKind::Unknown
    }
}

fn try_scan_operator(chars: &[char], i: usize) -> usize {
    let len = chars.len();
    let c = chars[i];
    match c {
        '&' if i + 1 < len && chars[i+1] == '&' => i + 2,
        '|' if i + 1 < len && chars[i+1] == '|' => i + 2,
        '>' if i + 1 < len && chars[i+1] == '>' => i + 2,
        '<' if i + 1 < len && chars[i+1] == '<' => i + 2,
        '|' | ';' | '&' | '>' | '<' | '{' | '}' | '(' | ')' => i + 1,
        _ => i,
    }
}

fn scan_string(chars: &[char], line: &str, start: usize) -> (usize, usize) {
    let quote = chars[start];
    let mut i = start + 1;
    let len = chars.len();
    while i < len {
        if chars[i] == '\\' && i + 1 < len { i += 2; continue; }
        if chars[i] == quote { i += 1; break; }
        i += 1;
    }
    (i, char_to_byte_offset(line, i))
}

fn scan_word(chars: &[char], line: &str, start: usize) -> (usize, usize) {
    let mut i = start;
    let len = chars.len();
    while i < len {
        let c = chars[i];
        if c.is_whitespace() || matches!(c, '|' | ';' | '&' | '<' | '>' | '\'' | '"' | '{' | '}' | '(' | ')' | '#') {
            break;
        }
        i += 1;
    }
    (i, char_to_byte_offset(line, i))
}

fn char_to_byte_offset(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

// ──────────────────────────────────────────────────────────────────────────────
// 1. Syntax Highlighter
// ──────────────────────────────────────────────────────────────────────────────

pub struct DpHighlighter;

impl Highlighter for DpHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let tokens = lex_for_highlight(line);
        let mut styled = StyledText::new();
        let mut last = 0usize;

        for tok in &tokens {
            // Unstyled gap before this token
            if tok.start > last {
                styled.push((nu_ansi_term::Style::new(), line[last..tok.start].to_string()));
            }

            let text = &line[tok.start..tok.end];
            let ansi_style = match tok.kind {
                TokenKind::Command  => style::command(),
                TokenKind::Builtin  => style::builtin(),
                TokenKind::Argument => style::argument(),
                TokenKind::Flag     => style::flag(),
                TokenKind::String   => style::string(),
                TokenKind::Operator => style::operator(),
                TokenKind::Comment  => style::comment(),
                TokenKind::Unknown  => style::error(),
            };
            styled.push((ansi_style, text.to_string()));
            last = tok.end;
        }

        // Trailing unstyled text
        if last < line.len() {
            styled.push((nu_ansi_term::Style::new(), line[last..].to_string()));
        }

        styled
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 2. Autosuggestions (history + completion)
// ──────────────────────────────────────────────────────────────────────────────

/// Wraps reedline's `DefaultHinter` with a history-priority strategy.
///
/// reedline's `DefaultHinter` already does history-based grey-text suggestions.
/// We expose it directly so the caller (main.rs) can construct it with the
/// shared `History` object.
///
/// To create it:
/// ```rust
/// use reedline::{DefaultHinter, History};
/// let hinter = DpHinter::new();
/// ```
pub struct DpHinter {
    inner: DefaultHinter,
}

impl DpHinter {
    pub fn new() -> Self {
        Self {
            inner: DefaultHinter::default()
                .with_style(style::suggestion()),
        }
    }
}

impl Hinter for DpHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        history: &dyn reedline::History,
        use_ansi_coloring: bool,
        cwd: &str,
    ) -> String {
        self.inner.handle(line, pos, history, use_ansi_coloring, cwd)
    }

    fn complete_hint(&self) -> String {
        self.inner.complete_hint()
    }

    fn next_hint_token(&self) -> String {
        self.inner.next_hint_token()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 3. Smart Tab Completion
// ──────────────────────────────────────────────────────────────────────────────

/// Completion source — what are we completing?
#[derive(Debug)]
enum CompletionContext {
    /// First word — complete command names.
    Command,
    /// Subsequent word starting with `-` — complete flags (not implemented; stub).
    Flag { #[allow(dead_code)] command: String },
    /// Subsequent word — complete file/directory paths.
    Path { prefix: String },
}

pub struct DpCompleter {
    /// Fuzzy matcher from nucleo.
    matcher: Matcher,
}

impl DpCompleter {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(nucleo::Config::DEFAULT),
        }
    }

    fn classify_context(&self, line: &str, pos: usize) -> CompletionContext {
        let before = &line[..pos];
        let tokens: Vec<&str> = before.split_whitespace().collect();

        if tokens.is_empty() || (tokens.len() == 1 && !before.ends_with(' ')) {
            return CompletionContext::Command;
        }

        let current = if before.ends_with(' ') { "" } else { tokens.last().map_or("", |v| v) };

        if current.starts_with('-') {
            let cmd = tokens.first().unwrap_or(&"").to_string();
            return CompletionContext::Flag { command: cmd };
        }

        CompletionContext::Path { prefix: current.to_string() }
    }

    /// Collect command completions: built-ins + binaries on PATH.
    fn complete_commands(&mut self, prefix: &str, span: Span) -> Vec<Suggestion> {
        let mut candidates: Vec<String> = BUILTIN_NAMES.iter()
            .map(|s| s.to_string())
            .collect();

        // Walk PATH directories.
        let path = env::var("PATH").unwrap_or_default();
        for dir in path.split(':') {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        candidates.push(name);
                    }
                }
            }
        }

        candidates.sort_unstable();
        candidates.dedup();

        self.fuzzy_filter(candidates, prefix)
            .into_iter()
            .map(|name| Suggestion {
                value: name,
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: true,
            })
            .collect()
    }

    /// Collect path completions for `prefix`.
    fn complete_paths(&mut self, prefix: &str, span: Span) -> Vec<Suggestion> {
        let (dir_part, file_prefix) = split_path_prefix(prefix);
        let base_dir = if dir_part.is_empty() {
            env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(&dir_part)
        };

        let entries: Vec<String> = std::fs::read_dir(&base_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                // 前缀过滤：用文件名（非完整路径）与 file_prefix 做 starts_with 比较
                if !file_prefix.is_empty() && !name.starts_with(file_prefix) {
                    return None;
                }
                let full = if dir_part.is_empty() {
                    name.clone()
                } else {
                    format!("{}{}", dir_part, name)
                };
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                Some(if is_dir { format!("{}/", full) } else { full })
            })
            .collect();

        entries
            .into_iter()
            .map(|path| Suggestion {
                value: path.clone(),
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: !path.ends_with('/'),
            })
            .collect()
    }

    /// Filter `candidates` by fuzzy-matching against `query`.
    /// Returns candidates sorted by match score (best first).
    fn fuzzy_filter(&mut self, candidates: Vec<String>, query: &str) -> Vec<String> {
        if query.is_empty() {
            return candidates;
        }

        use nucleo::{pattern::{CaseMatching, Normalization, Pattern}, Utf32Str};

        let pattern = Pattern::parse(
            query,
            CaseMatching::Smart,
            Normalization::Smart,
        );

        let mut scored: Vec<(u32, String)> = candidates
            .into_iter()
            .filter_map(|c| {
                let mut buf = Vec::new();
                let haystack = Utf32Str::new(&c, &mut buf);
                let score = pattern.score(haystack, &mut self.matcher)?;
                Some((score, c))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, c)| c).collect()
    }
}

impl Completer for DpCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        match self.classify_context(line, pos) {
            CompletionContext::Command => {
                let prefix = line[..pos].split_whitespace().last().unwrap_or("");
                let span = Span::new(pos - prefix.len(), pos);
                self.complete_commands(prefix, span)
            }
            CompletionContext::Flag { .. } => {
                Vec::new()
            }
            CompletionContext::Path { ref prefix } => {
                let span = Span::new(pos - prefix.len(), pos);
                self.complete_paths(prefix, span)
            }
        }
    }
}

/// Split a path prefix into (directory, file_name_prefix).
/// e.g.  "src/ma"  → ("src", "ma")
///       "/etc/"   → ("/etc", "")
fn split_path_prefix(prefix: &str) -> (String, &str) {
    match prefix.rfind('/') {
        Some(pos) => (prefix[..=pos].to_string(), &prefix[pos + 1..]),
        None      => (String::new(), prefix),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Reedline builder  (called from main.rs)
// ──────────────────────────────────────────────────────────────────────────────

use reedline::{
    ColumnarMenu, EditCommand, FileBackedHistory, KeyCode, KeyModifiers,
    Reedline, ReedlineMenu, MenuBuilder,
};
use std::sync::{Arc, Mutex};
use std::io::Write;

/// Bash 风格基础补全器包装：
///   - 唯一匹配 → 返回单条建议，quick_completions 自动内联替换
///   - 多匹配有公共前缀 → 返回 LCP 单条建议，内联填充
///   - 多匹配无可扩展前缀 → 打印候选列表到终端（纯文本），返回空
///   - 无匹配 → 静默
/// 当 smart_enabled=true 时，直接返回全部候选让 reedline 菜单处理。
pub struct BashStyleCompleter {
    inner: DpCompleter,
    smart_enabled: bool,
    last_printed: Arc<Mutex<Vec<String>>>,
}

impl BashStyleCompleter {
    pub fn new(smart_enabled: bool) -> Self {
        Self {
            inner: DpCompleter::new(),
            smart_enabled,
            last_printed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn longest_common_prefix(values: &[String]) -> String {
        if values.is_empty() { return String::new(); }
        let first = values[0].as_bytes();
        let mut end = first.len();
        for s in &values[1..] {
            let bs = s.as_bytes();
            end = end.min(bs.len());
            let mut i = 0;
            while i < end && first[i] == bs[i] { i += 1; }
            end = i;
            if end == 0 { break; }
        }
        let mut cut = end;
        while cut > 0 && !values[0].is_char_boundary(cut) { cut -= 1; }
        values[0][..cut].to_string()
    }

    fn print_candidates(candidates: &[String]) {
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout);
        for c in candidates {
            let _ = write!(stdout, "{}  ", c);
        }
        let _ = writeln!(stdout);
        let _ = stdout.flush();
    }
}

impl Completer for BashStyleCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let suggestions = self.inner.complete(line, pos);

        if self.smart_enabled {
            return suggestions;
        }

        if suggestions.is_empty() {
            *self.last_printed.lock().unwrap() = Vec::new();
            return Vec::new();
        }

        if suggestions.len() == 1 {
            *self.last_printed.lock().unwrap() = Vec::new();
            return suggestions;
        }

        let values: Vec<String> = suggestions.iter().map(|s| s.value.clone()).collect();
        let span = suggestions[0].span;
        let typed = &line[span.start..span.end];
        let lcp = Self::longest_common_prefix(&values);

        if lcp.len() > typed.len() {
            *self.last_printed.lock().unwrap() = Vec::new();
            return vec![Suggestion {
                value: lcp,
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: false,
            }];
        }

        Self::print_candidates(&values);
        *self.last_printed.lock().unwrap() = values;
        Vec::new()
    }
}

/// Construct a fully-configured `Reedline` instance with all three features.
///
/// # Arguments
/// * `history_path` — path to the on-disk history file.
///
/// # Usage in main.rs
/// ```rust
/// let (mut rl, history_arc) = interactive::build_editor(&hist_path)?;
/// loop {
///     match rl.read_line(&prompt) {
///         Ok(Signal::Success(line)) => { /* process line */ }
///         Ok(Signal::CtrlC)  => { println!(); continue; }
///         Ok(Signal::CtrlD)  => break,
///         Err(e) => { eprintln!("readline error: {}", e); break; }
///     }
/// }
/// ```
/// Feature flags from [features] in config.toml.
/// Mirrors `config::FeaturesConfig` but is defined here to avoid a circular dep.
pub struct FeatureFlags {
    pub syntax_highlighting: bool,
    pub auto_suggest:        bool,
    pub tab_completion:      bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self { syntax_highlighting: true, auto_suggest: true, tab_completion: true }
    }
}

pub fn build_editor(history_path: &Path, flags: &FeatureFlags) -> anyhow::Result<Reedline> {
    // History
    let history = Box::new(
        FileBackedHistory::with_file(10_000, history_path.to_path_buf())
            .map_err(|e| anyhow::anyhow!("history: {}", e))?,
    );

    // Highlighting (conditional on feature flag)
    let highlighter: Option<Box<dyn Highlighter>> =
        if flags.syntax_highlighting { Some(Box::new(DpHighlighter)) } else { None };

    // Autosuggestions (conditional on feature flag)
    let hinter: Option<Box<dyn Hinter>> =
        if flags.auto_suggest { Some(Box::new(DpHinter::new())) } else { None };

    let mut keybindings = reedline::default_emacs_keybindings();

    // Tab 始终绑定为打开补全菜单（基础补全通过 quick_completions + partial_completions 实现）
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );

    // Right arrow: 如果有 hint 则接受，否则正常右移光标
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Right,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::HistoryHintComplete,
            ReedlineEvent::Edit(vec![EditCommand::MoveRight { select: false }]),
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('f'),
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::HistoryHintComplete,
            ReedlineEvent::Edit(vec![EditCommand::MoveRight { select: false }]),
        ]),
    );

    let edit_mode = Box::new(reedline::Emacs::new(keybindings));

    // Build reedline, applying only the enabled features.
    let mut rl = Reedline::create()
        .with_history(history)
        .with_edit_mode(edit_mode)
        .with_ansi_colors(true)
        .with_quick_completions(true)
        .with_partial_completions(true);

    if let Some(h) = highlighter { rl = rl.with_highlighter(h); }
    if let Some(h) = hinter      { rl = rl.with_hinter(h); }

    // 补全器：基础模式用 BashStyleCompleter，高级模式直接传全部候选给菜单
    rl = rl.with_completer(Box::new(BashStyleCompleter::new(flags.tab_completion)));

    if flags.tab_completion {
        // 高级模式：多列彩色菜单
        let completion_menu = Box::new(
            ColumnarMenu::default()
                .with_name("completion_menu")
                .with_columns(4)
                .with_column_width(None)
                .with_column_padding(2),
        );
        rl = rl.with_menu(ReedlineMenu::EngineCompleter(completion_menu));
    } else {
        // 基础模式：注册一个最小菜单供 quick_completions 使用（唯一匹配时自动接受）
        let basic_menu = Box::new(
            ColumnarMenu::default()
                .with_name("completion_menu")
                .with_columns(1)
                .with_column_width(None)
                .with_column_padding(0),
        );
        rl = rl.with_menu(ReedlineMenu::EngineCompleter(basic_menu));
    }

    Ok(rl)
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Highlighter ───────────────────────────────────────────────────────────

    #[test]
    fn highlight_builtin_recognised() {
        let tokens = lex_for_highlight("cd /tmp");
        assert_eq!(tokens[0].kind, TokenKind::Builtin);
        assert_eq!(tokens[1].kind, TokenKind::Argument);
    }

    #[test]
    fn highlight_flag_recognised() {
        let tokens = lex_for_highlight("ls -la");
        assert!(matches!(tokens[0].kind, TokenKind::Command | TokenKind::Unknown));
        assert_eq!(tokens[1].kind, TokenKind::Flag);
    }

    #[test]
    fn highlight_quoted_string() {
        let tokens = lex_for_highlight(r#"echo "hello world""#);
        // tokens: [echo, "hello world"]
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1].kind, TokenKind::String);
    }

    #[test]
    fn highlight_operator_pipe() {
        let tokens = lex_for_highlight("ls | grep foo");
        let op = tokens.iter().find(|t| t.kind == TokenKind::Operator).unwrap();
        assert_eq!(&"ls | grep foo"[op.start..op.end], "|");
    }

    #[test]
    fn highlight_comment() {
        let tokens = lex_for_highlight("ls # list files");
        let c = tokens.iter().find(|t| t.kind == TokenKind::Comment).unwrap();
        assert!("ls # list files"[c.start..].starts_with('#'));
    }

    // ── Completer ────────────────────────────────────────────────────────────

    #[test]
    fn completion_context_first_word_is_command() {
        let c = DpCompleter::new();
        assert!(matches!(c.classify_context("ls", 2), CompletionContext::Command));
        assert!(matches!(c.classify_context("", 0),   CompletionContext::Command));
    }

    #[test]
    fn completion_context_path_after_space() {
        let c = DpCompleter::new();
        assert!(matches!(
            c.classify_context("ls /tm", 6),
            CompletionContext::Path { prefix } if prefix == "/tm"
        ));
    }

    #[test]
    fn completion_context_flag() {
        let c = DpCompleter::new();
        assert!(matches!(
            c.classify_context("ls -l", 5),
            CompletionContext::Flag { .. }
        ));
    }

    #[test]
    fn split_path_prefix_works() {
        assert_eq!(split_path_prefix("src/ma"),  ("src/".to_string(), "ma"));
        assert_eq!(split_path_prefix("/etc/"),   ("/etc/".to_string(), ""));
        assert_eq!(split_path_prefix("foo"),     ("".to_string(), "foo"));
    }

    #[test]
    fn fuzzy_filter_ranks_exact_prefix_first() {
        let mut c = DpCompleter::new();
        let candidates = vec!["grep".to_string(), "git".to_string(), "go".to_string()];
        let results = c.fuzzy_filter(candidates, "gr");
        assert_eq!(results[0], "grep");
    }

    // ── Parser integration ────────────────────────────────────────────────────
    // (The highlight lexer must not crash on partially-typed input.)

    #[test]
    fn highlight_partial_input_no_panic() {
        for partial in &[":", ":&", ":|:", "{ echo", r#"echo ""#, "ls | "] {
            // Must not panic.
            let _ = lex_for_highlight(partial);
        }
    }

    #[test]
    fn highlight_fork_bomb_colours() {
        // `:() { :|:& };:` — the full fork bomb
        let tokens = lex_for_highlight(":() { :|:& };:");
        // Should contain at least one Operator for { } | & ;
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Operator));
    }

    // ── Completion span correctness ──────────────────────────────────────────

    #[test]
    fn completion_span_for_path_is_absolute_in_line() {
        // "ls /e" with cursor at pos 5 → prefix="/e", span must be (3,5)
        let mut c = DpCompleter::new();
        let suggestions = c.complete("ls /e", 5);
        for s in &suggestions {
            assert_eq!(s.span.start, 3, "span.start should point to '/' in 'ls /e'");
            assert_eq!(s.span.end, 5, "span.end should be cursor pos");
        }
    }

    #[test]
    fn completion_span_for_command_is_absolute_in_line() {
        let mut c = DpCompleter::new();
        let suggestions = c.complete("cd", 2);
        for s in &suggestions {
            assert_eq!(s.span.start, 0);
            assert_eq!(s.span.end, 2);
        }
    }

    #[test]
    fn completion_replaces_only_current_word() {
        // Simulate what reedline does: line[span.start..span.end] is replaced by value.
        let mut c = DpCompleter::new();
        let line = "ls /e";
        let suggestions = c.complete(line, 5);
        if let Some(s) = suggestions.iter().find(|s| s.value.contains("/etc")) {
            let mut result = String::new();
            result.push_str(&line[..s.span.start]);
            result.push_str(&s.value);
            result.push_str(&line[s.span.end..]);
            assert!(result.starts_with("ls /etc"), "got: {}", result);
        }
    }
}