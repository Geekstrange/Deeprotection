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
    /// A background command: `cmd &`
    Background(Box<CommandNode>),
    /// A brace-group compound command: `{ cmd1 ; cmd2 ; }`
    Compound(Vec<CommandNode>),
    /// A shell function definition: `name() { body }`
    FunctionDef {
        name: String,
        body: Box<CommandNode>,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in registry (kept in sync with parser.rs)
// ──────────────────────────────────────────────────────────────────────────────

const BUILTINS: &[&str] = &[
    // Already dispatched in main.rs / jobs.rs / cd.rs
    "cd", "exit", "export", "unset", "fg", "bg", "jobs",
    // All built-ins implemented in builtins.rs
    ":", "alias", "unalias", "bind",
    "history", "kill",
    "readonly", "set", "source", ".",
    "exec", "eval", "trap", "wait",
    "break", "continue", "return", "shift", "local", "test", "[",
    "dirs", "pushd", "popd", "umask",
    "help", "command", "builtin", "type",
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
    Pipe,   // |
    And,    // &&
    Or,     // ||
    Semi,   // ;
    Bg,     // &  (background)
    LBrace, // {
    RBrace, // }
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
            // ── Background operator & (must come AFTER the && arm) ────────
            '&' => {
                flush!(); segments.push(RawSegment::Op(SToken::Bg)); i += 1;
            }

            // ── Parentheses: word-break characters ───────────────────────────
            '(' => {
                flush!();
                segments.push(RawSegment::Text("(".to_string()));
                i += 1;
            }
            ')' => {
                flush!();
                segments.push(RawSegment::Text(")".to_string()));
                i += 1;
            }
            // ── Braces: compound command delimiters ───────────────────────
            '{' => {
                flush!();
                segments.push(RawSegment::Op(SToken::LBrace));
                i += 1;
            }
            '}' => {
                flush!();
                segments.push(RawSegment::Op(SToken::RBrace));
                i += 1;
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

    /// logical_expr := pipeline ( (';' | '&&' | '||') pipeline )* ['&']
    ///
    /// `&` at the end wraps the accumulated left side in `Background`.
    /// `cmd1 ; cmd2 &` backgrounds only cmd2 (matches bash behaviour).
    fn parse_logical(&mut self) -> Result<CommandNode, ParseError> {
        let mut left = self.parse_pipeline()?;

        loop {
            match self.peek() {
                Some(SToken::Bg) => {
                    // `&` backgrounds the node built so far, then continues
                    // parsing in case there is a subsequent `;` or `&&`.
                    self.advance();
                    left = CommandNode::Background(Box::new(left));
                    // After `&` there may be more commands (e.g. `:& ; ls`).
                    if self.peek().is_none() { break; }
                    // Expect a sequential separator before the next command.
                    match self.peek() {
                        Some(SToken::Semi) | Some(SToken::And) | Some(SToken::Or) => {}
                        _ => break,
                    }
                }
                Some(SToken::Semi) | Some(SToken::And) | Some(SToken::Or) => {
                    let op = match self.peek() {
                        Some(SToken::Semi) => LogicOp::Seq,
                        Some(SToken::And)  => LogicOp::And,
                        Some(SToken::Or)   => LogicOp::Or,
                        _ => unreachable!(),
                    };
                    self.advance();
                    if self.peek().is_none() { break; }
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


    /// compound := '{' logical_list '}'
    ///
    /// A brace group groups multiple commands into one node.
    fn parse_compound(&mut self) -> Result<CommandNode, ParseError> {
        // Consume '{'
        self.advance();
        let mut body: Vec<CommandNode> = Vec::new();

        loop {
            // Skip bare semicolons used as statement terminators inside braces.
            while self.peek() == Some(&SToken::Semi) {
                self.advance();
            }
            if self.peek() == Some(&SToken::RBrace) || self.peek().is_none() {
                break;
            }
            body.push(self.parse_logical()?);
        }

        if self.peek() != Some(&SToken::RBrace) {
            return Err(ParseError::Lex(
                "syntax error: missing `}' after compound command".to_string(),
            ));
        }
        self.advance(); // consume '}'

        match body.len() {
            0 => Err(ParseError::Lex("syntax error: empty compound command `{}'".to_string())),
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
        // ── Function definition: WORD '(' ')' '{' ... '}' ────────────────
        // Pattern: next token is Word(name), then Word("("), then Word(")")
        // (parentheses are emitted as words by the prescan / segments layer).
        if let Some(SToken::Word(name)) = self.peek().cloned() {
            let is_open  = self.tokens.get(self.pos + 1) == Some(&SToken::Word("(".to_string()));
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

    /// simple_cmd := WORD+ | NAME '(' ')' compound
    fn parse_simple(&mut self) -> Result<SimpleCommand, ParseError> {
        let mut words: Vec<String> = Vec::new();

        while let Some(SToken::Word(w)) = self.peek() {
            words.push(w.clone());
            self.advance();
        }

        if words.is_empty() {
            // Could be a compound command at the start of a pipeline stage.
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
        CommandNode::Background(inner) => flatten_commands(inner),
        CommandNode::Compound(nodes)   => nodes.iter().flat_map(flatten_commands).collect(),
        CommandNode::FunctionDef { body, .. } => flatten_commands(body),
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

    #[test]
    fn background_operator_parses() {
        // `:&` must become Background(Simple(:))
        let node = parse_command_line(":&").unwrap();
        match node {
            CommandNode::Background(inner) => match *inner {
                CommandNode::Simple(sc) => assert_eq!(sc.program, ":"),
                _ => panic!("expected Simple inside Background"),
            },
            _ => panic!("expected Background, got {:?}", node),
        }
    }

    #[test]
    fn pipe_then_background() {
        // `:|:&` → Background(Pipeline([:, :]))
        let node = parse_command_line(":|:&").unwrap();
        match node {
            CommandNode::Background(inner) => match *inner {
                CommandNode::Pipeline(cmds) => assert_eq!(cmds.len(), 2),
                _ => panic!("expected Pipeline inside Background"),
            },
            _ => panic!("expected Background, got {:?}", node),
        }
    }

    #[test]
    fn function_def_parses() {
        // `:() { :|:& }` — the classic fork bomb function definition
        let node = parse_command_line(":() { :|:& }").unwrap();
        match node {
            CommandNode::FunctionDef { name, body } => {
                assert_eq!(name, ":");
                match *body {
                    CommandNode::Background(_) => {}
                    _ => panic!("expected Background body, got {:?}", body),
                }
            }
            _ => panic!("expected FunctionDef, got {:?}", node),
        }
    }

    #[test]
    fn forkbomb_full_parse() {
        // `:() { :|:& };:` — define and immediately invoke
        let node = parse_command_line(":() { :|:& };:").unwrap();
        match node {
            CommandNode::Logical { op: LogicOp::Seq, left, right } => {
                assert!(matches!(*left, CommandNode::FunctionDef { .. }));
                // right is the invocation `:`
                match *right {
                    CommandNode::Simple(sc) => assert_eq!(sc.program, ":"),
                    _ => panic!("expected Simple invocation"),
                }
            }
            _ => panic!("expected Logical Seq, got {:?}", node),
        }
    }

    #[test]
    fn compound_command_parses() {
        let node = parse_command_line("{ echo a ; echo b }").unwrap();
        match node {
            CommandNode::Compound(cmds) => assert_eq!(cmds.len(), 2),
            _ => panic!("expected Compound"),
        }
    }

}