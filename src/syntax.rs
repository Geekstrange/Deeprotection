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
}

impl SimpleCommand {
    pub fn name(&self) -> &str {
        std::path::Path::new(&self.program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.program)
    }

    pub fn args(&self) -> &[String] {
        if self.argv.is_empty() { &[] } else { &self.argv[1..] }
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
        left:  Box<CommandNode>,
        op:    LogicOp,
        right: Box<CommandNode>,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in registry (kept in sync with parser.rs)
// ──────────────────────────────────────────────────────────────────────────────

const BUILTINS: &[&str] = &["cd", "exit", "export", "unset", "fg", "bg", "jobs", "history"];

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
    Pipe,   // |
    And,    // &&
    Or,     // ||
    Semi,   // ;
}

/// Intermediate segment produced by the character-level pre-scanner.
#[derive(Debug)]
enum RawSegment {
    /// A fragment of raw text (may contain quotes/escapes — shlex will handle them).
    Text(String),
    Op(SToken),
}

/// Scan `input` outside of any quoted region, splitting on `&&`, `||`, `|`, `;`.
/// Quoted regions (single-quote, double-quote, backslash) are passed through
/// verbatim so that `"foo|bar"` is never treated as a pipe.
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
            // ── Backslash escape (outside quotes) ─────────────────────────
            '\\' => {
                current.push('\\');
                i += 1;
                if i < len { current.push(chars[i]); i += 1; }
            }

            // ── Single-quoted region: no escapes, ends at next ' ──────────
            '\'' => {
                current.push('\'');
                i += 1;
                while i < len && chars[i] != '\'' {
                    current.push(chars[i]);
                    i += 1;
                }
                if i < len { current.push('\''); i += 1; } // closing '
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
                if i < len { current.push('"'); i += 1; } // closing "
            }

            // ── Shell metacharacters (outside quotes) ─────────────────────
            '&' if i + 1 < len && chars[i + 1] == '&' => {
                flush!(); segments.push(RawSegment::Op(SToken::And)); i += 2;
            }
            '|' if i + 1 < len && chars[i + 1] == '|' => {
                flush!(); segments.push(RawSegment::Op(SToken::Or));  i += 2;
            }
            '|' => {
                flush!(); segments.push(RawSegment::Op(SToken::Pipe)); i += 1;
            }
            ';' => {
                flush!(); segments.push(RawSegment::Op(SToken::Semi)); i += 1;
            }

            // ── Ordinary character ────────────────────────────────────────
            c => { current.push(c); i += 1; }
        }
    }
    flush!();
    segments
}

/// Convert pre-scanned segments into STokens, running shlex on each Text fragment.
///
/// Each Text segment may contain multiple words (e.g. `"grep -i foo"`), so we
/// shlex-split it and emit one `SToken::Word` per resulting token.
fn segments_to_stokens(segments: Vec<RawSegment>) -> Result<Vec<SToken>, ParseError> {
    let mut out = Vec::new();
    for seg in segments {
        match seg {
            RawSegment::Op(op) => out.push(op),
            RawSegment::Text(text) => {
                let words = shlex::split(&text).ok_or_else(|| {
                    ParseError::Lex(format!("unterminated quote in: {}", text))
                })?;
                for w in words {
                    out.push(SToken::Word(w));
                }
            }
        }
    }
    Ok(out)
}

// ──────────────────────────────────────────────────────────────────────────────
// Recursive-descent parser
// ──────────────────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<SToken>,
    pos: usize,
    search_path: String,
}

impl Parser {
    fn new(tokens: Vec<SToken>, search_path: String) -> Self {
        Self { tokens, pos: 0, search_path }
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

    /// logical_expr := pipeline ( (';' | '&&' | '||') pipeline )*
    fn parse_logical(&mut self) -> Result<CommandNode, ParseError> {
        let mut left = self.parse_pipeline()?;

        loop {
            let op = match self.peek() {
                Some(SToken::Semi) => LogicOp::Seq,
                Some(SToken::And)  => LogicOp::And,
                Some(SToken::Or)   => LogicOp::Or,
                _                  => break,
            };
            self.advance();
            // Trailing operator with nothing after it — treat as Seq/noop.
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

        Ok(left)
    }

    /// pipeline := simple_cmd ( '|' simple_cmd )*
    fn parse_pipeline(&mut self) -> Result<CommandNode, ParseError> {
        let mut cmds = vec![self.parse_simple()?];

        while self.peek() == Some(&SToken::Pipe) {
            self.advance();
            cmds.push(self.parse_simple()?);
        }

        if cmds.len() == 1 {
            Ok(CommandNode::Simple(cmds.remove(0)))
        } else {
            Ok(CommandNode::Pipeline(cmds))
        }
    }

    /// simple_cmd := WORD+
    fn parse_simple(&mut self) -> Result<SimpleCommand, ParseError> {
        let mut words: Vec<String> = Vec::new();

        while let Some(SToken::Word(w)) = self.peek() {
            words.push(w.clone());
            self.advance();
        }

        if words.is_empty() {
            return Err(ParseError::Lex(
                "expected a command, got operator or end of input".to_string(),
            ));
        }

        let cmd_token = &words[0];

        if is_builtin(cmd_token) {
            let raw = words.join(" ");
            return Ok(SimpleCommand {
                program: cmd_token.clone(),
                argv: words,
                is_builtin: true,
                raw,
            });
        }

        let resolved = resolve_binary(cmd_token, &self.search_path)
            .ok_or_else(|| ParseError::NotFound(cmd_token.clone()))?;

        let raw = words.join(" ");
        Ok(SimpleCommand {
            program: resolved.to_string_lossy().into_owned(),
            argv: words,
            is_builtin: false,
            raw,
        })
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

    let stokens = segments_to_stokens(segments)?;
    if stokens.is_empty() {
        return Err(ParseError::Empty);
    }

    let search_path = env::var("PATH").unwrap_or_default();
    let mut parser = Parser::new(stokens, search_path);

    let node = parser.parse_logical()?;

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
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pipe_len(node: &CommandNode) -> usize {
        match node {
            CommandNode::Pipeline(v) => v.len(),
            _ => panic!("expected pipeline, got {:?}", node),
        }
    }

    #[test]
    fn simple_command_parses() {
        let node = parse_command_line("echo hello").unwrap();
        match node {
            CommandNode::Simple(sc) => assert_eq!(sc.argv, vec!["echo", "hello"]),
            _ => panic!("expected Simple"),
        }
    }

    #[test]
    fn pipeline_with_spaces_parses() {
        let node = parse_command_line("ls | cat").unwrap();
        assert_eq!(pipe_len(&node), 2);
    }

    /// Core regression test: pipe without surrounding whitespace must work.
    #[test]
    fn pipeline_no_spaces_parses() {
        let node = parse_command_line("ls|cat").unwrap();
        assert_eq!(pipe_len(&node), 2);
    }

    /// Mixed spacing: one side has space, other doesn't.
    #[test]
    fn pipeline_mixed_spacing() {
        let node = parse_command_line("ls |cat").unwrap();
        assert_eq!(pipe_len(&node), 2);
    }

    #[test]
    fn logical_and_parses() {
        let node = parse_command_line("true && false").unwrap();
        match node {
            CommandNode::Logical { op: LogicOp::And, .. } => {}
            _ => panic!("expected Logical And"),
        }
    }

    #[test]
    fn logical_and_no_spaces() {
        let node = parse_command_line("true&&false").unwrap();
        match node {
            CommandNode::Logical { op: LogicOp::And, .. } => {}
            _ => panic!("expected Logical And"),
        }
    }

    #[test]
    fn logical_or_parses() {
        let node = parse_command_line("false || echo hi").unwrap();
        match node {
            CommandNode::Logical { op: LogicOp::Or, .. } => {}
            _ => panic!("expected Logical Or"),
        }
    }

    #[test]
    fn semicolon_no_spaces() {
        let node = parse_command_line("echo a;echo b").unwrap();
        match node {
            CommandNode::Logical { op: LogicOp::Seq, .. } => {}
            _ => panic!("expected Logical Seq"),
        }
    }

    /// Quoted pipe must NOT become an operator.
    #[test]
    fn quoted_pipe_is_word_not_operator() {
        let node = parse_command_line(r#"echo "foo|bar""#).unwrap();
        match &node {
            CommandNode::Simple(sc) => assert_eq!(sc.argv[1], "foo|bar"),
            _ => panic!("expected Simple, got {:?}", node),
        }
    }

    /// Single-quoted pipe must NOT become an operator.
    #[test]
    fn single_quoted_pipe_is_word() {
        let node = parse_command_line("echo 'foo|bar'").unwrap();
        match &node {
            CommandNode::Simple(sc) => assert_eq!(sc.argv[1], "foo|bar"),
            _ => panic!("expected Simple, got {:?}", node),
        }
    }

    #[test]
    fn flatten_collects_all_leaves() {
        let node = parse_command_line("echo a ; echo b").unwrap();
        assert_eq!(flatten_commands(&node).len(), 2);
    }

    #[test]
    fn three_stage_pipeline() {
        let node = parse_command_line("ls | cat | cat").unwrap();
        assert_eq!(pipe_len(&node), 3);
    }
}