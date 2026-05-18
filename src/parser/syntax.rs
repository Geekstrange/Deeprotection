// syntax.rs - Shell Syntax AST & Parser
//
// Converts a raw input string into a CommandNode tree that the executor can
// walk without ever invoking `sh`.  Handles:
//
//   ;          sequential execution (always continue)
//   &&         short-circuit AND (continue only on success)
//   ||         short-circuit OR  (continue only on failure)
//   |          pipeline (stdout of left → stdin of right)
//
// Operator precedence (lowest → highest, matching POSIX sh):
//   ;  &&  ||   (left-associative, same precedence tier — parsed left to right)
//   |            (higher precedence than the above — binds tighter)
//
// Example parse trees:
//   "ls | grep foo && echo ok"
//   →  Logical(Pipeline([ls, grep foo]), And, Simple(echo ok))
//
//   "false || echo a ; echo b"
//   →  Seq(Logical(Simple(false), Or, Simple(echo a)), Simple(echo b))
//
// The parser is a hand-written recursive-descent tokeniser operating on the
// *already-shlex-tokenised* token stream from `parser.rs`.  Shell metacharacters
// (|, &&, ||, ;) must appear as separate whitespace-delimited tokens — this is
// the same rule POSIX sh uses when words are not quoted.

use crate::parser::{resolve_binary, ParseError};
use std::env;

// ──────────────────────────────────────────────────────────────────────────────
// AST
// ──────────────────────────────────────────────────────────────────────────────

/// I/O redirection attached to a simple command.
#[derive(Debug, Clone)]
pub struct Redirection {
    /// File descriptor to redirect (None = default: 0 for <, 1 for >).
    pub fd: Option<i32>,
    /// Kind of redirection.
    pub kind: RedirectKind,
    /// Target filename or file descriptor number.
    pub target: String,
}

/// Type of I/O redirection.
#[derive(Debug, Clone, PartialEq)]
pub enum RedirectKind {
    Input,      // <
    Output,     // >
    Append,     // >>
    DupIn,      // <&
    DupOut,     // >&
    ReadWrite,  // <>
    Clobber,    // >|
    Heredoc,    // <<
    HeredocStrip, // <<-
}

/// A single external command with its resolved binary path and argument vector.
#[derive(Debug, Clone)]
pub struct SimpleCommand {
    /// Absolute path of the binary (e.g. `/usr/bin/grep`).
    /// For built-ins this is the bare name (`cd`, `exit`, …).
    pub program: String,
    /// Full argv including argv[0] as the user typed it.
    pub argv: Vec<String>,
    /// Whether this is a dpshell built-in (never forked).
    /// Kept for security audit consumers that inspect the tree externally.
    #[allow(dead_code)]
    pub is_builtin: bool,
    /// Original token slice (for logging / display).
    pub raw: String,
    /// I/O redirections attached to this command.
    pub redirections: Vec<Redirection>,
}

impl SimpleCommand {
    pub fn name(&self) -> &str {
        std::path::Path::new(&self.program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.program)
    }

    pub fn args(&self) -> &[String] {
        if self.argv.is_empty() {
            &[]
        } else {
            &self.argv[1..]
        }
    }
}

/// Logical connector between two command nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum LogicOp {
    /// `;`  — always execute right side
    Seq,
    /// `&&` — execute right side only if left exited 0
    And,
    /// `||` — execute right side only if left exited non-0
    Or,
}

/// The full command AST.
#[derive(Debug, Clone)]
pub enum CommandNode {
    /// A single command: `ls -la`
    Simple(SimpleCommand),
    /// A pipeline: `ls | grep foo | wc -l`
    Pipeline(Vec<SimpleCommand>),
    /// A logical chain: `cmd1 && cmd2`, `cmd1 || cmd2`, `cmd1 ; cmd2`
    Logical {
        left: Box<CommandNode>,
        op: LogicOp,
        right: Box<CommandNode>,
    },
    /// A background command: `cmd &`
    Background(Box<CommandNode>),
    /// A brace-group compound command: `{ cmd1 ; cmd2 ; }`
    Compound(Vec<CommandNode>),
    /// A shell function definition: `name() { body }`
    FunctionDef {
        name: String,
        body: Box<CommandNode>,
    },
    /// `if pipeline; then list; [elif pipeline; then list;]* [else list;] fi`
    If {
        cond: Box<CommandNode>,
        then_body: Vec<CommandNode>,
        elifs: Vec<(CommandNode, Vec<CommandNode>)>,
        else_body: Vec<CommandNode>,
    },
    /// `for VAR [in WORDS...]; do list; done`
    For {
        var: String,
        words: Vec<String>,
        body: Vec<CommandNode>,
    },
    /// `while pipeline; do list; done [redirections]`
    While {
        cond: Box<CommandNode>,
        body: Vec<CommandNode>,
        redirections: Vec<Redirection>,
    },
    /// `until pipeline; do list; done [redirections]`
    Until {
        cond: Box<CommandNode>,
        body: Vec<CommandNode>,
        redirections: Vec<Redirection>,
    },
    /// `case WORD in [(PATTERN [| PATTERN]*) list ;;]* esac`
    Case {
        word: String,
        arms: Vec<(Vec<String>, Vec<CommandNode>)>,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in registry (kept in sync with parser.rs)
// ──────────────────────────────────────────────────────────────────────────────

const BUILTINS: &[&str] = &[
    // Already dispatched in main.rs / jobs.rs / cd.rs
    "cd", "exit", "export", "unset", "fg", "bg", "jobs", "logout",
    // All built-ins implemented in builtins.rs
    ":", "alias", "unalias", "bind", "history", "kill", "readonly", "set", "source", ".", "exec",
    "eval", "trap", "wait", "break", "continue", "return", "shift", "local", "test", "[", "dirs",
    "pushd", "popd", "umask", "help", "command", "builtin", "type",
    "echo", "read", "printf", "true", "false",
];

fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tokeniser
// ──────────────────────────────────────────────────────────────────────────────
//
// Problem: `shlex::split("ls|grep foo")` → `["ls|grep", "foo"]` because shlex
// is a *word* splitter, not a shell grammar tokeniser.  It never splits on `|`,
// `&&`, `||`, or `;` regardless of surrounding whitespace.
//
// Solution: scan the raw input character-by-character *before* invoking shlex,
// respecting single-quotes, double-quotes, and backslash escapes.  Whenever a
// metacharacter sequence is found outside quotes it is emitted as a separate
// `RawSegment::Op`; the surrounding word fragments are collected and then fed
// individually through `shlex::split` for proper quote/escape handling.
//
// Examples:
//   "ls|grep foo"   → [Word("ls"), Pipe, Word("grep foo")]
//   "ls | grep foo" → [Word("ls"), Pipe, Word("grep foo")]
//   "echo 'a|b'"    → [Word("echo"), Word("a|b")]   ← quoted: NOT a pipe
//   "a&&b||c;d"     → [Word("a"), And, Word("b"), Or, Word("c"), Semi, Word("d")]

/// Shell-level token kinds produced by the pre-scanner.
#[derive(Debug, Clone, PartialEq)]
enum SToken {
    Word(String),
    Pipe,          // |
    And,           // &&
    Or,            // ||
    Semi,          // ;
    Bg,            // &  (background)
    LBrace,        // {
    RBrace,        // }
    RedirectOut,   // >  (also >| for clobber)
    RedirectAppend,// >>
    RedirectIn,    // <
    RedirectDupIn, // <&
    RedirectDupOut,// >&
    RedirectRW,    // <>
    Heredoc,       // <<
    HeredocStrip,  // <<-
    DoubleSemi,    // ;;
    Newline,       // \n (statement separator in control structures)
}

/// Intermediate segment produced by the character-level pre-scanner.
#[derive(Debug)]
enum RawSegment {
    /// A fragment of raw text (may contain quotes/escapes — shlex will handle them).
    Text(String),
    /// An operator token (pipe, redirect, etc.).
    Op(SToken),
    /// A redirect operator that was NOT immediately preceded by a digit
    /// (there was whitespace between the last word and the operator).
    /// This prevents `-c 3 > file` from treating `3` as fd 3.
    OpSpaceAfter(SToken),
}

/// Returns true if `c` is a character that constitutes a word boundary for
/// brace-as-operator detection. Braces are only shell reserved words when
/// surrounded by these characters (matching bash behaviour).
fn is_brace_word_break(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\r' | '|' | '&' | ';' | '(' | ')' | '{' | '}'
    )
}

/// Scan `input` outside of any quoted region, splitting on `&&`, `||`, `|`, `;`.
/// Quoted regions (single-quote, double-quote, backslash) are passed through
/// verbatim so that `"foo|bar"` is never treated as a pipe.
#[allow(unused_assignments)]
fn prescan(input: &str) -> Vec<RawSegment> {
    let mut segments: Vec<RawSegment> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    // Helper: flush `current` as a Text segment (skip if empty/whitespace-only).
    macro_rules! flush {
        () => {
            if !current.trim().is_empty() {
                segments.push(RawSegment::Text(current.trim().to_string()));
            }
            current = String::new();
        };
    }

    while i < len {
        match chars[i] {
            // ── Newline (outside quotes) — command separator ─────────────
            '\n' => {
                flush!();
                segments.push(RawSegment::Op(SToken::Newline));
                i += 1;
            }
            // ── Backslash escape (outside quotes) ─────────────────────────
            '\\' => {
                // Backslash-newline is line continuation — skip both.
                if i + 1 < len && chars[i + 1] == '\n' {
                    i += 2;
                    continue;
                }
                current.push('\\');
                i += 1;
                if i < len {
                    current.push(chars[i]);
                    i += 1;
                }
            }

            // ── Single-quoted region: no escapes, ends at next ' ──────────
            '\'' => {
                current.push('\'');
                i += 1;
                while i < len && chars[i] != '\'' {
                    current.push(chars[i]);
                    i += 1;
                }
                if i < len {
                    current.push('\'');
                    i += 1;
                } // closing '
            }

            // ── Double-quoted region: backslash still active ──────────────
            '"' => {
                current.push('"');
                i += 1;
                while i < len && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < len {
                        current.push('\\');
                        current.push(chars[i + 1]);
                        i += 2;
                    } else {
                        current.push(chars[i]);
                        i += 1;
                    }
                }
                if i < len {
                    current.push('"');
                    i += 1;
                } // closing "
            }

            // ── Shell metacharacters (outside quotes) ─────────────────────
            '&' if i + 1 < len && chars[i + 1] == '&' => {
                flush!();
                segments.push(RawSegment::Op(SToken::And));
                i += 2;
            }
            '|' if i + 1 < len && chars[i + 1] == '|' => {
                flush!();
                segments.push(RawSegment::Op(SToken::Or));
                i += 2;
            }
            '|' => {
                flush!();
                segments.push(RawSegment::Op(SToken::Pipe));
                i += 1;
            }
            ';' if i + 1 < len && chars[i + 1] == ';' => {
                flush!();
                segments.push(RawSegment::Op(SToken::DoubleSemi));
                i += 2;
            }
            ';' => {
                flush!();
                segments.push(RawSegment::Op(SToken::Semi));
                i += 1;
            }
            // ── Redirect operators ───────────────────────────────────────
            // Must come BEFORE the Background `&` arm because `>&`, `<&`
            // contain `&` but are redirects, not background operators.
            //
            // Adjacency tracking: `3>file` means fd 3 redirect; `3 > file`
            // means argument "3" followed by fd 1 redirect.  We check
            // whether a digit immediately precedes the operator.
            '>' if i + 1 < len && chars[i + 1] == '>' => {
                let adj = i > 0 && chars[i - 1].is_ascii_digit();
                flush!();
                segments.push(if adj { RawSegment::Op(SToken::RedirectAppend) } else { RawSegment::OpSpaceAfter(SToken::RedirectAppend) });
                i += 2;
            }
            '>' if i + 1 < len && chars[i + 1] == '&' => {
                let adj = i > 0 && chars[i - 1].is_ascii_digit();
                flush!();
                segments.push(if adj { RawSegment::Op(SToken::RedirectDupOut) } else { RawSegment::OpSpaceAfter(SToken::RedirectDupOut) });
                i += 2;
            }
            '>' if i + 1 < len && chars[i + 1] == '|' => {
                flush!();
                segments.push(RawSegment::Op(SToken::RedirectOut)); // clobber: >|
                i += 2;
            }
            '>' => {
                let adj = i > 0 && chars[i - 1].is_ascii_digit();
                                flush!();
                segments.push(if adj { RawSegment::Op(SToken::RedirectOut) } else { RawSegment::OpSpaceAfter(SToken::RedirectOut) });
                i += 1;
            }
            '<' if i + 1 < len && chars[i + 1] == '<'
                 && i + 2 < len && chars[i + 2] == '-' => {
                flush!();
                segments.push(RawSegment::Op(SToken::HeredocStrip));
                i += 3;
            }
            '<' if i + 1 < len && chars[i + 1] == '<' => {
                flush!();
                segments.push(RawSegment::Op(SToken::Heredoc));
                i += 2;
            }
            '<' if i + 1 < len && chars[i + 1] == '&' => {
                let adj = i > 0 && chars[i - 1].is_ascii_digit();
                flush!();
                segments.push(if adj { RawSegment::Op(SToken::RedirectDupIn) } else { RawSegment::OpSpaceAfter(SToken::RedirectDupIn) });
                i += 2;
            }
            '<' if i + 1 < len && chars[i + 1] == '>' => {
                let adj = i > 0 && chars[i - 1].is_ascii_digit();
                flush!();
                segments.push(if adj { RawSegment::Op(SToken::RedirectRW) } else { RawSegment::OpSpaceAfter(SToken::RedirectRW) });
                i += 2;
            }
            '<' => {
                let adj = i > 0 && chars[i - 1].is_ascii_digit();
                flush!();
                segments.push(if adj { RawSegment::Op(SToken::RedirectIn) } else { RawSegment::OpSpaceAfter(SToken::RedirectIn) });
                i += 1;
            }
            // ── Background operator & (must come AFTER && and >& / <& arms)
            '&' => {
                flush!();
                segments.push(RawSegment::Op(SToken::Bg));
                i += 1;
            }

            // ── Parentheses: word-break characters ───────────────────────────
            // Only split if NOT part of $(...) or $((...)) — check if current ends with '$'
            '(' => {
                if current.ends_with('$') {
                    current.push('(');
                    i += 1;
                    // Collect the rest of $(...) or $((...)) respecting nesting
                    let mut depth = 1u32;
                    while i < len && depth > 0 {
                        match chars[i] {
                            '(' => { depth += 1; current.push('('); i += 1; }
                            ')' => { depth -= 1; if depth > 0 { current.push(')'); } else { current.push(')'); } i += 1; }
                            '\'' => { current.push('\''); i += 1; while i < len && chars[i] != '\'' { current.push(chars[i]); i += 1; } if i < len { current.push('\''); i += 1; } }
                            '"' => { current.push('"'); i += 1; while i < len && chars[i] != '"' { if chars[i] == '\\' && i+1 < len { current.push('\\'); current.push(chars[i+1]); i += 2; } else { current.push(chars[i]); i += 1; } } if i < len { current.push('"'); i += 1; } }
                            c => { current.push(c); i += 1; }
                        }
                    }
                } else {
                    flush!();
                    segments.push(RawSegment::Text("(".to_string()));
                    i += 1;
                }
            }
            ')' => {
                flush!();
                segments.push(RawSegment::Text(")".to_string()));
                i += 1;
            }
            // ── Braces: only operators when standalone (like bash reserved words).
            // When glued to adjacent text (e.g. `test{1..5}`, `{a,b}`), they
            // remain ordinary characters so brace expansion can process them.
            '{' => {
                let glued =
                    !current.is_empty() && !current.ends_with(|c: char| c.is_ascii_whitespace());
                let next_is_break = i + 1 >= len || is_brace_word_break(chars[i + 1]);
                if !glued && next_is_break {
                    flush!();
                    segments.push(RawSegment::Op(SToken::LBrace));
                } else {
                    current.push('{');
                }
                i += 1;
            }
            '}' => {
                let glued =
                    !current.is_empty() && !current.ends_with(|c: char| c.is_ascii_whitespace());
                let next_is_break = i + 1 >= len || is_brace_word_break(chars[i + 1]);
                if !glued && next_is_break {
                    flush!();
                    segments.push(RawSegment::Op(SToken::RBrace));
                } else {
                    current.push('}');
                }
                i += 1;
            }
            // ── Ordinary character ────────────────────────────────────────
            c => {
                current.push(c);
                i += 1;
            }
        }
    }
    flush!();
    segments
}

/// Convert pre-scanned segments into STokens, running shlex on each Text fragment.
///
/// Each Text segment may contain multiple words (e.g. `"grep -i foo"`), so we
/// shlex-split it and emit one `SToken::Word` per resulting token.
/// Returns (tokens, adjacency_flags).  adjacency_flags[i] is true iff
/// token i is a redirect operator that was immediately preceded by a digit
/// (i.e. the integer should be treated as an explicit fd number).
fn segments_to_stokens(segments: Vec<RawSegment>) -> Result<(Vec<SToken>, Vec<bool>), ParseError> {
    let mut out = Vec::new();
    let mut adj = Vec::new();
    for seg in &segments {
        match seg {
            RawSegment::Op(op) => { out.push(op.clone()); adj.push(true); }
            RawSegment::OpSpaceAfter(op) => { out.push(op.clone()); adj.push(false); }
            RawSegment::Text(text) => {
                if text.contains("$(") || text.contains("\\$") {
                    let words = split_respecting_dollar_paren(text);
                    for w in words {
                        out.push(SToken::Word(w));
                        adj.push(true);
                    }
                } else {
                    let words = shlex::split(&text)
                        .ok_or_else(|| ParseError::Lex(format!("unterminated quote in: {}", text)))?;
                    for w in words {
                        out.push(SToken::Word(w.clone()));
                        adj.push(true);
                    }
                }
            }
        }
    }
    Ok((out, adj))
}

fn split_respecting_dollar_paren(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            ' ' | '\t' => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                i += 1;
            }
            '\'' => {
                current.push(chars[i]);
                i += 1;
                while i < len && chars[i] != '\'' {
                    current.push(chars[i]);
                    i += 1;
                }
                if i < len { current.push(chars[i]); i += 1; }
            }
            '"' => {
                i += 1;
                while i < len && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < len {
                        match chars[i + 1] {
                            '$' | '`' | '"' | '\\' => {
                                current.push('\\');
                                current.push(chars[i + 1]);
                                i += 2;
                            }
                            _ => {
                                current.push(chars[i + 1]);
                                i += 2;
                            }
                        }
                    } else {
                        current.push(chars[i]);
                        i += 1;
                    }
                }
                if i < len { i += 1; }
            }
            '$' if i + 1 < len && chars[i + 1] == '(' => {
                current.push('$');
                current.push('(');
                i += 2;
                let mut depth = 1u32;
                while i < len && depth > 0 {
                    match chars[i] {
                        '(' => { depth += 1; current.push('('); i += 1; }
                        ')' => { depth -= 1; current.push(')'); i += 1; }
                        '\'' => { current.push('\''); i += 1; while i < len && chars[i] != '\'' { current.push(chars[i]); i += 1; } if i < len { current.push('\''); i += 1; } }
                        '"' => { current.push('"'); i += 1; while i < len && chars[i] != '"' { if chars[i] == '\\' && i+1 < len { current.push('\\'); current.push(chars[i+1]); i += 2; } else { current.push(chars[i]); i += 1; } } if i < len { current.push('"'); i += 1; } }
                        '\\' => { current.push('\\'); i += 1; if i < len { current.push(chars[i]); i += 1; } }
                        c => { current.push(c); i += 1; }
                    }
                }
            }
            '\\' => {
                i += 1;
                if i < len { current.push(chars[i]); i += 1; }
            }
            c => { current.push(c); i += 1; }
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// If the last word in `words` is a plain integer that would be a file
/// descriptor number (like `2` in `2>file`), pop it and return it as an i32.
///
/// IMPORTANT: The integer is only treated as an fd if it appeared immediately
/// adjacent to the redirect operator in the original input (no whitespace).
/// For example, `2>file` means redirect fd 2, but `-c 2 > file` means
/// argument `2` to `-c` followed by a stdout redirect.
///
/// The caller passes `adjacent: true` when the prescan detected the integer
/// and redirect operator as a combined token (e.g. `3>>`). When false
/// (separate tokens with whitespace between them), the integer stays.
fn pop_fd_if_present(words: &mut Vec<String>, adjacent: bool) -> Option<i32> {
    if !adjacent {
        return None;
    }
    match words.last() {
        Some(w) if !w.is_empty()
            && w.chars().all(|c| c.is_ascii_digit()) =>
        {
            words.pop().and_then(|s| s.parse().ok())
        }
        _ => None,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Recursive-descent parser
// ──────────────────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<SToken>,
    pos: usize,
    search_path: String,
    /// Per-token adjacency flag: true for redirect operators that were
    /// immediately preceded by a digit (so the digit should be popped as fd).
    adjacency: Vec<bool>,
}

impl Parser {
    fn new(tokens: Vec<SToken>, search_path: String, adjacency: Vec<bool>) -> Self {
        Self {
            tokens,
            pos: 0,
            search_path,
            adjacency,
        }
    }

    /// Returns true if the token at self.pos is a redirect AND was adjacent
    /// to a digit in the original input (digit should be popped as fd).
    fn fd_is_adjacent(&self) -> bool {
        self.pos < self.adjacency.len() && self.adjacency[self.pos]
    }

    fn peek(&self) -> Option<&SToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&SToken> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    // ── Grammar rules (lowest precedence first) ────────────────────────────

    /// logical_expr := pipeline ( (';' | '&&' | '||') pipeline )* ['&']
    ///
    /// `&` at the end wraps the accumulated left side in `Background`.
    /// `cmd1 ; cmd2 &` backgrounds only cmd2 (matches bash behaviour).
    fn parse_logical(&mut self) -> Result<CommandNode, ParseError> {
        self.skip_separators();
        let mut left = self.parse_pipeline()?;

        loop {
            match self.peek() {
                Some(SToken::Newline) => {
                    self.advance();
                    continue;
                }
                Some(SToken::Bg) => {
                    // `&` backgrounds the node built so far, then continues
                    // parsing in case there is a subsequent `;` or `&&`.
                    self.advance();
                    left = CommandNode::Background(Box::new(left));
                    // After `&` there may be more commands (e.g. `:& ; ls`).
                    if self.peek().is_none() {
                        break;
                    }
                    // Expect a sequential separator before the next command.
                    match self.peek() {
                        Some(SToken::Semi) | Some(SToken::And) | Some(SToken::Or) => {}
                        _ => break,
                    }
                }
                Some(SToken::Semi) | Some(SToken::And) | Some(SToken::Or) => {
                    let op = match self.peek() {
                        Some(SToken::Semi) => LogicOp::Seq,
                        Some(SToken::And) => LogicOp::And,
                        Some(SToken::Or) => LogicOp::Or,
                        _ => unreachable!(),
                    };
                    self.advance();
                    if self.peek().is_none() {
                        break;
                    }
                    let right = self.parse_pipeline()?;
                    left = CommandNode::Logical {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// and_or_bg := pipeline ( ('&&' | '||') pipeline )* ['&']
    ///
    /// Like `parse_logical` but does NOT consume `;`.  Used inside compound
    /// commands so that `parse_compound` can handle `;` as a statement separator.
    fn parse_and_or_bg(&mut self) -> Result<CommandNode, ParseError> {
        let mut left = self.parse_pipeline()?;

        loop {
            match self.peek() {
                Some(SToken::Bg) => {
                    self.advance();
                    left = CommandNode::Background(Box::new(left));
                    break;
                }
                Some(SToken::And) => {
                    self.advance();
                    if self.peek().is_none() {
                        break;
                    }
                    let right = self.parse_pipeline()?;
                    left = CommandNode::Logical {
                        left: Box::new(left),
                        op: LogicOp::And,
                        right: Box::new(right),
                    };
                }
                Some(SToken::Or) => {
                    self.advance();
                    if self.peek().is_none() {
                        break;
                    }
                    let right = self.parse_pipeline()?;
                    left = CommandNode::Logical {
                        left: Box::new(left),
                        op: LogicOp::Or,
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    // ── Statement list helpers ──────────────────────────────────────────

    /// Skip `;` and `\n` separator tokens.  Used between statements inside
    /// control structure bodies.
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(SToken::Semi | SToken::Newline)) {
            self.advance();
        }
    }

    /// Parse a list of commands (separated by `;`, `\n`, `&&`, `||`) until
    /// one of the given closing keyword strings is seen as a Word token.
    /// Returns the list of parsed CommandNodes.
    fn parse_list_until(&mut self, closing: &[&str]) -> Result<Vec<CommandNode>, ParseError> {
        let mut list: Vec<CommandNode> = Vec::new();
        self.skip_separators();
        loop {
            match self.peek() {
                Some(SToken::Word(w)) if closing.contains(&w.as_str()) => break,
                None => break,
                _ => {}
            }
            list.push(self.parse_and_or_bg()?);
            // Expect a separator or closing keyword.
            match self.peek() {
                Some(SToken::Semi | SToken::Newline) => { self.advance(); self.skip_separators(); }
                Some(SToken::Word(w)) if closing.contains(&w.as_str()) => break,
                Some(SToken::DoubleSemi) => break, // case terminator
                None => break,
                _ => {
                    return Err(ParseError::Lex(format!(
                        "expected separator or closing keyword, got {:?}",
                        self.peek()
                    )));
                }
            }
        }
        Ok(list)
    }

    /// Peek at a Word token value without advancing.
    fn peek_word(&self) -> Option<String> {
        match self.peek() {
            Some(SToken::Word(w)) => Some(w.clone()),
            _ => None,
        }
    }

    // ── Control structure parsers ───────────────────────────────────────

    /// `if pipeline; then list; [elif pipeline; then list;]* [else list;] fi`
    fn parse_if(&mut self) -> Result<CommandNode, ParseError> {
        self.advance(); // consume 'if'
        let cond = self.parse_and_or_bg()?;
        self.expect_word("then")?;
        let then_body = self.parse_list_until(&["elif", "else", "fi"])?;

        let mut elifs: Vec<(CommandNode, Vec<CommandNode>)> = Vec::new();
        while self.peek_word().as_deref() == Some("elif") {
            self.advance(); // consume 'elif'
            let elif_cond = self.parse_and_or_bg()?;
            self.expect_word("then")?;
            let elif_body = self.parse_list_until(&["elif", "else", "fi"])?;
            elifs.push((elif_cond, elif_body));
        }

        let mut else_body: Vec<CommandNode> = Vec::new();
        if self.peek_word().as_deref() == Some("else") {
            self.advance(); // consume 'else'
            else_body = self.parse_list_until(&["fi"])?;
        }

        self.expect_word("fi")?;
        Ok(CommandNode::If { cond: Box::new(cond), then_body, elifs, else_body })
    }

    /// `for VAR [in WORDS...]; do list; done`
    /// `for VAR; do list; done`  (iterates over "$@")
    fn parse_for(&mut self) -> Result<CommandNode, ParseError> {
        self.advance(); // consume 'for'
        let var = self.expect_ident("variable name after 'for'")?;
        let mut words: Vec<String> = Vec::new();

        // Optional `in WORDS...`
        if self.peek_word().as_deref() == Some("in") {
            self.advance(); // consume 'in'
            loop {
                match self.peek() {
                    Some(SToken::Word(w)) if w != "do" && w != ";" && w != "\n" => {
                        words.push(w.clone());
                        self.advance();
                    }
                    _ => break,
                }
            }
        }

        self.skip_separators();
        self.expect_word("do")?;
        let body = self.parse_list_until(&["done"])?;
        self.expect_word("done")?;
        Ok(CommandNode::For { var, words, body })
    }

    /// `while pipeline; do list; done`
    /// `until pipeline; do list; done`
    fn parse_while_until(&mut self, is_until: bool) -> Result<CommandNode, ParseError> {
        self.advance(); // consume 'while' or 'until'
        let cond = self.parse_and_or_bg()?;
        self.expect_word("do")?;
        let body = self.parse_list_until(&["done"])?;
        self.expect_word("done")?;

        let mut redirections: Vec<Redirection> = Vec::new();
        loop {
            match self.peek().cloned() {
                Some(SToken::RedirectIn) => {
                    self.advance();
                    let target = match self.peek() {
                        Some(SToken::Word(t)) => { let t = t.clone(); self.advance(); t }
                        _ => break,
                    };
                    redirections.push(Redirection { fd: Some(0), kind: RedirectKind::Input, target });
                }
                Some(SToken::RedirectOut) => {
                    self.advance();
                    let target = match self.peek() {
                        Some(SToken::Word(t)) => { let t = t.clone(); self.advance(); t }
                        _ => break,
                    };
                    redirections.push(Redirection { fd: Some(1), kind: RedirectKind::Output, target });
                }
                Some(SToken::RedirectAppend) => {
                    self.advance();
                    let target = match self.peek() {
                        Some(SToken::Word(t)) => { let t = t.clone(); self.advance(); t }
                        _ => break,
                    };
                    redirections.push(Redirection { fd: Some(1), kind: RedirectKind::Append, target });
                }
                _ => break,
            }
        }

        if is_until {
            Ok(CommandNode::Until { cond: Box::new(cond), body, redirections })
        } else {
            Ok(CommandNode::While { cond: Box::new(cond), body, redirections })
        }
    }

    /// `case WORD in [(PATTERN [| PATTERN]*) list ;;]* esac`
    fn parse_case(&mut self) -> Result<CommandNode, ParseError> {
        self.advance(); // consume 'case'
        let word = self.expect_ident_or_word("word after 'case'")?;
        self.expect_word("in")?;
        self.skip_separators();

        let mut arms: Vec<(Vec<String>, Vec<CommandNode>)> = Vec::new();
        loop {
            match self.peek() {
                Some(SToken::Word(w)) if w == "esac" => break,
                None => break,
                _ => {}
            }

            // Parse one or more patterns separated by `|`.
            let mut patterns: Vec<String> = Vec::new();
            loop {
                match self.peek() {
                    Some(SToken::Word(p)) if p != "|" && p != ")" && p != "esac" => {
                        patterns.push(p.clone());
                        self.advance();
                    }
                    Some(SToken::Word(p)) if p == "|" => {
                        self.advance(); // skip `|`
                        continue;
                    }
                    _ => break,
                }
            }
            // Expect `)` after pattern(s).
            match self.peek() {
                Some(SToken::Word(w)) if w == ")" => { self.advance(); }
                _ => {
                    return Err(ParseError::Lex(
                        "expected ')' after case pattern".to_string()
                    ));
                }
            }
            self.skip_separators();

            // Parse the arm body until `;;`.
            let body = self.parse_list_until(&["esac"])?;
            // Expect `;;` or `esac` (last arm can omit `;;`).
            match self.peek() {
                Some(SToken::DoubleSemi) => { self.advance(); }
                Some(SToken::Word(w)) if w == "esac" => {}
                _ => {
                    return Err(ParseError::Lex(
                        "expected ';;' or 'esac' after case arm".to_string()
                    ));
                }
            }
            self.skip_separators();
            arms.push((patterns, body));
        }

        self.expect_word("esac")?;
        Ok(CommandNode::Case { word, arms })
    }

    // ── Expect helpers ──────────────────────────────────────────────────

    /// Expect and consume a Word token matching `expected`. Skips leading `;`
    /// and `\n` separators. Returns error if not found.
    fn expect_word(&mut self, expected: &str) -> Result<(), ParseError> {
        // Skip leading separators (`;`, `\n`) — e.g. `if true; then`.
        self.skip_separators();
        match self.peek() {
            Some(SToken::Word(w)) if w == expected => {
                self.advance();
                // Consume trailing separator.
                if matches!(self.peek(), Some(SToken::Semi | SToken::Newline)) {
                    self.advance();
                }
                Ok(())
            }
            other => Err(ParseError::Lex(format!(
                "expected '{}', got {:?}",
                expected, other
            ))),
        }
    }

    /// Expect and consume a Word token that is a valid identifier.
    fn expect_ident(&mut self, context: &str) -> Result<String, ParseError> {
        match self.peek().cloned() {
            Some(SToken::Word(w)) if is_valid_ident(&w) => {
                self.advance();
                Ok(w)
            }
            other => Err(ParseError::Lex(format!(
                "expected {}; got {:?}",
                context, other
            ))),
        }
    }

    /// Expect and consume any Word token (or fail).
    fn expect_ident_or_word(&mut self, context: &str) -> Result<String, ParseError> {
        match self.peek().cloned() {
            Some(SToken::Word(w)) => { self.advance(); Ok(w) }
            other => Err(ParseError::Lex(format!(
                "expected {}; got {:?}", context, other
            ))),
        }
    }

    /// compound := '{' logical_list '}'
    ///
    /// A brace group groups multiple commands into one node.
    fn parse_compound(&mut self) -> Result<CommandNode, ParseError> {
        // Consume '{'
        self.advance();
        let mut body: Vec<CommandNode> = Vec::new();

        loop {
            while matches!(self.peek(), Some(SToken::Semi) | Some(SToken::Newline)) {
                self.advance();
            }
            if self.peek() == Some(&SToken::RBrace) || self.peek().is_none() {
                break;
            }
            body.push(self.parse_and_or_bg()?);
        }

        if self.peek() != Some(&SToken::RBrace) {
            return Err(ParseError::Lex(
                "syntax error: missing `}' after compound command".to_string(),
            ));
        }
        self.advance(); // consume '}'

        match body.len() {
            0 => Err(ParseError::Lex(
                "syntax error: empty compound command `{}'".to_string(),
            )),
            1 => Ok(body.remove(0)),
            _ => Ok(CommandNode::Compound(body)),
        }
    }

    /// Try to parse a function definition: `NAME ( ) COMPOUND`
    ///
    /// Called when `parse_simple` sees that the first word is followed by
    /// `(` `)`.  The NAME token has already been consumed into `words[0]`.
    fn try_parse_funcdef(&mut self, name: String) -> Result<CommandNode, ParseError> {
        // Consume '(' and ')' — already verified by caller.
        self.advance(); // '('
        self.advance(); // ')'

        // Optional whitespace is already handled by token boundaries.
        // Expect a compound command body `{ ... }`.
        if self.peek() != Some(&SToken::LBrace) {
            return Err(ParseError::Lex(format!(
                "syntax error near `{}': expected `{{' after function definition",
                name
            )));
        }
        let body = self.parse_compound()?;
        Ok(CommandNode::FunctionDef {
            name,
            body: Box::new(body),
        })
    }

    /// pipeline := ( funcdef | compound | simple_cmd ) ( '|' ( simple_cmd ) )*
    ///
    /// Function definitions and compound commands can only appear as the
    /// first (and only) stage of a pipeline — they cannot be piped from/to.
    fn parse_pipeline(&mut self) -> Result<CommandNode, ParseError> {
        // ── Control structure keywords ────────────────────────────────────
        match self.peek_word().as_deref() {
            Some("if") => return self.parse_if(),
            Some("for") => return self.parse_for(),
            Some("while") => return self.parse_while_until(false),
            Some("until") => return self.parse_while_until(true),
            Some("case") => return self.parse_case(),
            _ => {}
        }

        // ── Function definition: WORD '(' ')' '{' ... '}' ────────────────
        if let Some(SToken::Word(name)) = self.peek().cloned() {
            let is_open = self.tokens.get(self.pos + 1) == Some(&SToken::Word("(".to_string()));
            let is_close = self.tokens.get(self.pos + 2) == Some(&SToken::Word(")".to_string()));
            let has_body = self.tokens.get(self.pos + 3) == Some(&SToken::LBrace);
            if is_open && is_close && has_body {
                self.advance(); // consume name word
                return self.try_parse_funcdef(name);
            }
        }

        // ── Compound command starting with '{' ────────────────────────────
        if self.peek() == Some(&SToken::LBrace) {
            return self.parse_compound();
        }

        // ── Normal pipeline ───────────────────────────────────────────────
        let first = if self.peek() == Some(&SToken::LBrace) {
            let compound = self.parse_compound()?;
            return Ok(compound);
        } else {
            self.parse_simple()?
        };
        let mut cmds = vec![first];

        while self.peek() == Some(&SToken::Pipe) {
            self.advance();
            if self.peek() == Some(&SToken::LBrace) {
                let compound = self.parse_compound()?;
                let self_exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::path::PathBuf::from("/proc/self/exe"))
                    .to_string_lossy().into_owned();
                let sc = SimpleCommand {
                    program: self_exe.clone(),
                    argv: vec![self_exe, "-c".to_string(), compound_to_script(&compound)],
                    is_builtin: false,
                    raw: "{ ... }".to_string(),
                    redirections: vec![],
                };
                cmds.push(sc);
            } else {
                cmds.push(self.parse_simple()?);
            }
        }

        if cmds.len() == 1 {
            Ok(CommandNode::Simple(cmds.remove(0)))
        } else {
            Ok(CommandNode::Pipeline(cmds))
        }
    }

    /// simple_cmd := (WORD | IO_REDIRECT)*
    ///
    /// I/O redirects appear interleaved with argument words.  A redirect consists
    /// of an optional leading fd digit (e.g. `2` in `2>file`), a redirect operator
    /// (`>`, `>>`, `<`, `>&`, `<&`, `<>`, `<<`, `<<-`), and a target word.
    /// The leading fd, if present, is recognised by checking whether the last word
    /// token before the redirect operator is a plain integer.
    fn parse_simple(&mut self) -> Result<SimpleCommand, ParseError> {
        let mut words: Vec<String> = Vec::new();
        let mut redirections: Vec<Redirection> = Vec::new();

        loop {
            match self.peek().cloned() {
                Some(SToken::Word(w)) => {
                    words.push(w);
                    self.advance();
                }
                Some(tok @ (SToken::RedirectOut
                          | SToken::RedirectAppend
                          | SToken::RedirectIn
                          | SToken::RedirectDupIn
                          | SToken::RedirectDupOut
                          | SToken::RedirectRW)) => {
                    // Save adjacency BEFORE advancing past the redirect token.
                    let fd_adj = self.fd_is_adjacent();

                    let kind = match tok {
                        SToken::RedirectOut => {
                            self.advance();
                            RedirectKind::Output
                        }
                        SToken::RedirectAppend => { self.advance(); RedirectKind::Append }
                        SToken::RedirectIn => { self.advance(); RedirectKind::Input }
                        SToken::RedirectDupIn => { self.advance(); RedirectKind::DupIn }
                        SToken::RedirectDupOut => { self.advance(); RedirectKind::DupOut }
                        SToken::RedirectRW => { self.advance(); RedirectKind::ReadWrite }
                        _ => unreachable!(),
                    };

                    let fd = pop_fd_if_present(&mut words, fd_adj);

                    let target = match self.peek() {
                        Some(SToken::Word(t)) => {
                            let t = t.clone();
                            self.advance();
                            t
                        }
                        Some(_) => {
                            return Err(ParseError::Lex(format!(
                                "expected filename or fd after redirect, got operator"
                            )));
                        }
                        None => {
                            return Err(ParseError::Lex(format!(
                                "expected filename or fd after redirect, got end of input"
                            )));
                        }
                    };

                    redirections.push(Redirection { fd, kind, target });
                }
                Some(SToken::Heredoc) | Some(SToken::HeredocStrip) => {
                    let strip_tabs = matches!(self.peek(), Some(SToken::HeredocStrip));
                    self.advance();

                    let fd = pop_fd_if_present(&mut words, self.fd_is_adjacent());

                    let delimiter = match self.peek() {
                        Some(SToken::Word(d)) => {
                            let d = d.clone();
                            self.advance();
                            d
                        }
                        _ => {
                            return Err(ParseError::Lex(format!(
                                "expected heredoc delimiter after {}",
                                if strip_tabs { "<<-" } else { "<<" }
                            )));
                        }
                    };

                    // The heredoc body is NOT parsed from the token stream —
                    // it is collected in the REPL / script reader layer before
                    // parsing.  Here we store the delimiter as the target; the
                    // executor reads the body from a temp file or pipe that the
                    // caller has already prepared.
                    redirections.push(Redirection {
                        fd,
                        kind: if strip_tabs {
                            RedirectKind::HeredocStrip
                        } else {
                            RedirectKind::Heredoc
                        },
                        target: delimiter,
                    });
                }
                _ => break,
            }
        }

        if words.is_empty() && redirections.is_empty() {
            return Err(ParseError::Lex(
                "expected a command, got operator or end of input".to_string(),
            ));
        }

        // A command consisting of only redirections (no command word) is valid
        // in POSIX shell: the redirections are applied to the current shell.
        // We simulate that by using ":" (true) as the command.
        if words.is_empty() {
            words.push(":".to_string());
        }

        let cmd_token = &words[0];

        if is_builtin(cmd_token) {
            let raw = words.join(" ");
            return Ok(SimpleCommand {
                program: cmd_token.clone(),
                argv: words,
                is_builtin: true,
                raw,
                redirections,
            });
        }

        let (program, is_builtin_flag) = match resolve_binary(cmd_token, &self.search_path) {
            Some(resolved) => (resolved.to_string_lossy().into_owned(), false),
            None => (cmd_token.clone(), false),
        };

        let raw = words.join(" ");
        Ok(SimpleCommand {
            program,
            argv: words,
            is_builtin: is_builtin_flag,
            raw,
            redirections,
        })
    }
}

fn compound_to_script(node: &CommandNode) -> String {
    match node {
        CommandNode::Simple(sc) => sc.raw.clone(),
        CommandNode::Compound(nodes) => {
            nodes.iter().map(|n| compound_to_script(n)).collect::<Vec<_>>().join("; ")
        }
        CommandNode::Logical { left, op, right } => {
            let op_str = match op {
                LogicOp::Seq => ";",
                LogicOp::And => "&&",
                LogicOp::Or => "||",
            };
            format!("{} {} {}", compound_to_script(left), op_str, compound_to_script(right))
        }
        _ => String::new(),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ──────────────────────────────────────────────────────────────────────────────

/// Parse `input` into a `CommandNode` AST.
///
/// Steps:
///   1. `prescan`              — split on metacharacters respecting quotes
///   2. `segments_to_stokens` — shlex each word-fragment; emit operator tokens
///   3. Recursive-descent parse → `CommandNode`
///   4. Binary paths are resolved to absolute paths during parsing
pub fn parse_command_line(input: &str) -> Result<CommandNode, ParseError> {
    if input.trim().is_empty() {
        return Err(ParseError::Empty);
    }

    let segments = prescan(input);
    if segments.is_empty() {
        return Err(ParseError::Empty);
    }

    let (stokens, adjacency) = segments_to_stokens(segments)?;
    if stokens.is_empty() {
        return Err(ParseError::Empty);
    }

    let search_path = env::var("PATH").unwrap_or_default();
    let mut parser = Parser::new(stokens, search_path, adjacency);

    let node = parser.parse_logical()?;

    // Skip trailing newlines/semicolons
    while parser.pos < parser.tokens.len()
        && matches!(parser.tokens[parser.pos], SToken::Semi | SToken::Newline)
    {
        parser.pos += 1;
    }

    // Ensure all input was consumed.
    if parser.pos < parser.tokens.len() {
        return Err(ParseError::Lex(format!(
            "unexpected token: {:?}",
            parser.tokens[parser.pos]
        )));
    }

    Ok(node)
}

/// Collect all `SimpleCommand` leaves from a `CommandNode` (used by the
/// security pipeline to audit every command in a complex expression).
pub fn flatten_commands(node: &CommandNode) -> Vec<&SimpleCommand> {
    match node {
        CommandNode::Simple(sc) => vec![sc],
        CommandNode::Pipeline(cmds) => cmds.iter().collect(),
        CommandNode::Logical { left, right, .. } => {
            let mut v = flatten_commands(left);
            v.extend(flatten_commands(right));
            v
        }
        CommandNode::Background(inner) => flatten_commands(inner),
        CommandNode::Compound(nodes) => nodes.iter().flat_map(flatten_commands).collect(),
        CommandNode::FunctionDef { body, .. } => flatten_commands(body),
        CommandNode::If { cond, then_body, elifs, else_body } => {
            let mut v = flatten_commands(cond);
            for node in then_body { v.extend(flatten_commands(node)); }
            for (c, b) in elifs {
                v.extend(flatten_commands(c));
                for node in b { v.extend(flatten_commands(node)); }
            }
            for node in else_body { v.extend(flatten_commands(node)); }
            v
        }
        CommandNode::For { body, .. } => body.iter().flat_map(flatten_commands).collect(),
        CommandNode::While { cond, body, .. } | CommandNode::Until { cond, body, .. } => {
            let mut v = flatten_commands(cond);
            for node in body { v.extend(flatten_commands(node)); }
            v
        }
        CommandNode::Case { arms, .. } => arms.iter()
            .flat_map(|(_, body)| body.iter().flat_map(flatten_commands))
            .collect(),
    }
}
