use reedline::{Highlighter, StyledText};
use std::env;

use super::style;
use super::BUILTIN_NAMES;

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Command,
    Builtin,
    Argument,
    Flag,
    String,
    Operator,
    Comment,
    Unknown,
}

#[derive(Debug, Clone)]
struct HighlightToken {
    start: usize,
    end: usize,
    kind: TokenKind,
}

fn lex_for_highlight(line: &str) -> Vec<HighlightToken> {
    let mut tokens: Vec<HighlightToken> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut command_expected = true;

    while i < len {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        let start = char_to_byte_offset(line, i);

        if chars[i] == '#' {
            tokens.push(HighlightToken {
                start,
                end: line.len(),
                kind: TokenKind::Comment,
            });
            break;
        }

        let op_end = try_scan_operator(&chars, i);
        if op_end > i {
            let end = char_to_byte_offset(line, op_end);
            tokens.push(HighlightToken {
                start,
                end,
                kind: TokenKind::Operator,
            });
            command_expected = true;
            i = op_end;
            continue;
        }

        if chars[i] == '\'' || chars[i] == '"' {
            let (end_idx, end_byte) = scan_string(&chars, line, i);
            tokens.push(HighlightToken {
                start,
                end: end_byte,
                kind: TokenKind::String,
            });
            command_expected = false;
            i = end_idx;
            continue;
        }

        let (end_idx, end_byte) = scan_word(&chars, line, i);
        let word = &line[start..end_byte];
        let kind = if command_expected {
            classify_command(word)
        } else if word.starts_with('-') {
            TokenKind::Flag
        } else {
            TokenKind::Argument
        };
        tokens.push(HighlightToken {
            start,
            end: end_byte,
            kind,
        });
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
    } else if matches!(word, "}" | "{" | "(" | ")") {
        TokenKind::Operator
    } else {
        TokenKind::Unknown
    }
}

fn try_scan_operator(chars: &[char], i: usize) -> usize {
    let len = chars.len();
    match chars[i] {
        '&' if i + 1 < len && chars[i + 1] == '&' => i + 2,
        '|' if i + 1 < len && chars[i + 1] == '|' => i + 2,
        '>' if i + 1 < len && chars[i + 1] == '>' => i + 2,
        '<' if i + 1 < len && chars[i + 1] == '<' => i + 2,
        '|' | ';' | '&' | '>' | '<' | '{' | '}' | '(' | ')' => i + 1,
        _ => i,
    }
}

fn scan_string(chars: &[char], line: &str, start: usize) -> (usize, usize) {
    let quote = chars[start];
    let mut i = start + 1;
    let len = chars.len();
    while i < len {
        if chars[i] == '\\' && i + 1 < len {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            i += 1;
            break;
        }
        i += 1;
    }
    (i, char_to_byte_offset(line, i))
}

fn scan_word(chars: &[char], line: &str, start: usize) -> (usize, usize) {
    let mut i = start;
    let len = chars.len();
    while i < len {
        let c = chars[i];
        if c.is_whitespace()
            || matches!(
                c,
                '|' | ';' | '&' | '<' | '>' | '\'' | '"' | '{' | '}' | '(' | ')' | '#'
            )
        {
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

pub struct DpHighlighter;

impl Highlighter for DpHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let tokens = lex_for_highlight(line);
        let mut styled = StyledText::new();
        let mut last = 0usize;

        for tok in &tokens {
            if tok.start > last {
                styled.push((
                    nu_ansi_term::Style::new(),
                    line[last..tok.start].to_string(),
                ));
            }
            let text = &line[tok.start..tok.end];
            let ansi_style = match tok.kind {
                TokenKind::Command => style::command(),
                TokenKind::Builtin => style::builtin(),
                TokenKind::Argument => style::argument(),
                TokenKind::Flag => style::flag(),
                TokenKind::String => style::string(),
                TokenKind::Operator => style::operator(),
                TokenKind::Comment => style::comment(),
                TokenKind::Unknown => style::error(),
            };
            styled.push((ansi_style, text.to_string()));
            last = tok.end;
        }

        if last < line.len() {
            styled.push((nu_ansi_term::Style::new(), line[last..].to_string()));
        }

        styled
    }
}
