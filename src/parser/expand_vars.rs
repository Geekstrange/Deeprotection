use std::collections::HashMap;

pub const ESCAPED_DOLLAR: char = '\u{FFFD}';

pub fn unescape_dollars(s: &str) -> String {
    s.replace(ESCAPED_DOLLAR, "$")
}

/// Expand `$VAR`, `${VAR}`, and `$?`/`$$`/`$!` references in a single word token.
/// Looks up in `shell_vars` first, then `std::env`.
pub fn expand_word(word: &str, shell_vars: &HashMap<String, String>, last_exit: i32) -> String {
    expand_word_with_params(word, shell_vars, last_exit, &[])
}

pub fn expand_word_with_params(word: &str, shell_vars: &HashMap<String, String>, last_exit: i32, positional_params: &[String]) -> String {
    let mut result = String::with_capacity(word.len());
    let chars: Vec<char> = word.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '\\' && i + 1 < len && chars[i + 1] == '$' {
            result.push('\u{FFFD}');
            i += 2;
            continue;
        }
        if chars[i] != '$' {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        i += 1; // consume '$'

        if i >= len {
            result.push('$');
            break;
        }

        match chars[i] {
            '$' => {
                result.push_str(&std::process::id().to_string());
                i += 1;
            }
            '?' => {
                result.push_str(&last_exit.to_string());
                i += 1;
            }
            '!' => {
                i += 1;
            }
            '#' => {
                if !positional_params.is_empty() {
                    result.push_str(&positional_params.len().to_string());
                } else {
                    result.push_str("$#");
                }
                i += 1;
            }
            '@' | '*' => {
                if !positional_params.is_empty() {
                    result.push_str(&positional_params.join(" "));
                } else {
                    result.push('$');
                    result.push(chars[i]);
                }
                i += 1;
            }
            // $((expr)) — arithmetic expansion
            '(' if i + 1 < len && chars[i + 1] == '(' => {
                i += 2; // skip "(("
                let mut depth: u32 = 1;
                let mut expr = String::new();
                while i < len && depth > 0 {
                    if chars[i] == '(' && i + 1 < len && chars[i + 1] == '(' {
                        depth += 1; expr.push('('); expr.push('('); i += 2;
                    } else if chars[i] == ')' && i + 1 < len && chars[i + 1] == ')' {
                        depth -= 1;
                        if depth > 0 { expr.push(')'); expr.push(')'); i += 2; }
                        else { i += 2; }
                    } else {
                        expr.push(chars[i]); i += 1;
                    }
                }
                let val = crate::executor::eval_arithmetic_expr(&expr, shell_vars);
                result.push_str(&val.to_string());
            }
            // $(cmd) — command substitution, preserve as-is for later expansion
            '(' => {
                result.push('$');
                result.push('(');
                i += 1;
                let mut depth = 1u32;
                while i < len && depth > 0 {
                    match chars[i] {
                        '(' => { depth += 1; }
                        ')' => { depth -= 1; }
                        _ => {}
                    }
                    result.push(chars[i]);
                    i += 1;
                }
            }
            // ${VAR} or ${VAR:-default} or ${VAR%pattern} etc.
            '{' => {
                i += 1; // consume '{'
                let mut name = String::new();
                while i < len && chars[i] != '}' && chars[i] != ':' && chars[i] != '-'
                    && chars[i] != '+' && chars[i] != '=' && chars[i] != '?'
                    && chars[i] != '%' && chars[i] != '#'
                {
                    name.push(chars[i]);
                    i += 1;
                }

                if i < len && chars[i] == '}' {
                    // Simple ${VAR}
                    i += 1;
                    let val = lookup_var_with_params(&name, shell_vars, positional_params);
                    result.push_str(&val);
                } else if i < len {
                    // Collect the operator and the word
                    let mut op = String::new();
                    // Operators: :-, :+, :=, :?, -, +, =, ?, %, %%, #, ##
                    while i < len && (chars[i] == ':' || chars[i] == '-' || chars[i] == '+'
                        || chars[i] == '=' || chars[i] == '?' || chars[i] == '%' || chars[i] == '#')
                    {
                        op.push(chars[i]);
                        i += 1;
                        // For %% and ##, allow double
                        if op == "%" || op == "#" {
                            if i < len && chars[i] == op.chars().last().unwrap() {
                                op.push(chars[i]);
                                i += 1;
                            }
                            break;
                        }
                        if op.len() >= 2 { break; }
                    }
                    // Collect the word (everything until '}')
                    let mut modifier_word = String::new();
                    let mut brace_depth = 1i32;
                    while i < len && brace_depth > 0 {
                        if chars[i] == '{' { brace_depth += 1; }
                        if chars[i] == '}' {
                            brace_depth -= 1;
                            if brace_depth == 0 { break; }
                        }
                        modifier_word.push(chars[i]);
                        i += 1;
                    }
                    if i < len && chars[i] == '}' {
                        i += 1;
                    }

                    let val = lookup_var(&name, shell_vars);
                    let val_is_set = shell_vars.contains_key(&name) || std::env::var(&name).is_ok();

                    match op.as_str() {
                        ":-" => {
                            if val.is_empty() {
                                result.push_str(&modifier_word);
                            } else {
                                result.push_str(&val);
                            }
                        }
                        "-" => {
                            if !val_is_set {
                                result.push_str(&modifier_word);
                            } else {
                                result.push_str(&val);
                            }
                        }
                        ":+" => {
                            if !val.is_empty() {
                                result.push_str(&modifier_word);
                            }
                        }
                        "+" => {
                            if val_is_set {
                                result.push_str(&modifier_word);
                            }
                        }
                        ":=" => {
                            if val.is_empty() {
                                result.push_str(&modifier_word);
                            } else {
                                result.push_str(&val);
                            }
                        }
                        "%" => {
                            // Remove shortest suffix matching pattern
                            result.push_str(&strip_suffix(&val, &modifier_word, false));
                        }
                        "%%" => {
                            // Remove longest suffix matching pattern
                            result.push_str(&strip_suffix(&val, &modifier_word, true));
                        }
                        "#" => {
                            // Remove shortest prefix matching pattern
                            result.push_str(&strip_prefix(&val, &modifier_word, false));
                        }
                        "##" => {
                            // Remove longest prefix matching pattern
                            result.push_str(&strip_prefix(&val, &modifier_word, true));
                        }
                        _ => {
                            result.push_str(&val);
                        }
                    }
                } else {
                    // No closing brace found
                    let val = lookup_var(&name, shell_vars);
                    result.push_str(&val);
                }
            }
            // $0, $1, ... positional params (must come before the alphanumeric arm)
            c if c.is_ascii_digit() => {
                let idx = (c as u32 - '0' as u32) as usize;
                if idx == 0 {
                    result.push_str("dpshell");
                } else if !positional_params.is_empty() && idx <= positional_params.len() {
                    result.push_str(&positional_params[idx - 1]);
                } else if positional_params.is_empty() {
                    result.push('$');
                    result.push(c);
                }
                i += 1;
            }
            // $VAR — identifier chars
            c if c.is_ascii_alphanumeric() || c == '_' => {
                let mut name = String::new();
                while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    name.push(chars[i]);
                    i += 1;
                }
                let val = lookup_var(&name, shell_vars);
                result.push_str(&val);
            }
            // Not a valid var reference — emit literally
            _ => {
                result.push('$');
            }
        }
    }

    result
}

fn lookup_var(name: &str, shell_vars: &HashMap<String, String>) -> String {
    std::env::var(name).ok()
        .or_else(|| shell_vars.get(name).cloned())
        .unwrap_or_default()
}

fn lookup_var_with_params(name: &str, shell_vars: &HashMap<String, String>, positional_params: &[String]) -> String {
    if let Ok(idx) = name.parse::<usize>() {
        if idx == 0 {
            return "dpshell".to_string();
        }
        if idx <= positional_params.len() {
            return positional_params[idx - 1].clone();
        }
        return String::new();
    }
    lookup_var(name, shell_vars)
}

pub fn simple_glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_inner(&p, &t, 0, 0)
}

fn glob_match_inner(p: &[char], t: &[char], pi: usize, ti: usize) -> bool {
    let (mut pi, mut ti) = (pi, ti);
    while pi < p.len() {
        match p[pi] {
            '*' => {
                pi += 1;
                if pi == p.len() { return true; }
                for k in ti..=t.len() {
                    if glob_match_inner(p, t, pi, k) { return true; }
                }
                return false;
            }
            '?' => {
                if ti >= t.len() { return false; }
                pi += 1; ti += 1;
            }
            '[' => {
                if ti >= t.len() { return false; }
                pi += 1;
                let negate = pi < p.len() && (p[pi] == '!' || p[pi] == '^');
                if negate { pi += 1; }
                let mut matched = false;
                while pi < p.len() && p[pi] != ']' {
                    if pi + 2 < p.len() && p[pi + 1] == '-' {
                        if t[ti] >= p[pi] && t[ti] <= p[pi + 2] { matched = true; }
                        pi += 3;
                    } else {
                        if t[ti] == p[pi] { matched = true; }
                        pi += 1;
                    }
                }
                if pi < p.len() { pi += 1; }
                if matched == negate { return false; }
                ti += 1;
            }
            c => {
                if ti >= t.len() || t[ti] != c { return false; }
                pi += 1; ti += 1;
            }
        }
    }
    ti == t.len()
}

fn strip_suffix(val: &str, pattern: &str, longest: bool) -> String {
    if longest {
        for i in 0..=val.len() {
            if simple_glob_match(pattern, &val[i..]) {
                return val[..i].to_string();
            }
        }
    } else {
        for i in (0..=val.len()).rev() {
            if simple_glob_match(pattern, &val[i..]) {
                return val[..i].to_string();
            }
        }
    }
    val.to_string()
}

fn strip_prefix(val: &str, pattern: &str, longest: bool) -> String {
    if longest {
        for i in (0..=val.len()).rev() {
            if simple_glob_match(pattern, &val[..i]) {
                return val[i..].to_string();
            }
        }
    } else {
        for i in 0..=val.len() {
            if simple_glob_match(pattern, &val[..i]) {
                return val[i..].to_string();
            }
        }
    }
    val.to_string()
}

/// Expand all `$VAR` references in every token of `argv` (skips argv[0]).
/// argv[0] is the command name and is never expanded.
pub fn expand_argv(
    argv: &[String],
    shell_vars: &HashMap<String, String>,
    last_exit: i32,
) -> Vec<String> {
    expand_argv_with_params(argv, shell_vars, last_exit, &[])
}

pub fn expand_argv_with_params(
    argv: &[String],
    shell_vars: &HashMap<String, String>,
    last_exit: i32,
    positional_params: &[String],
) -> Vec<String> {
    if argv.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(argv.len());
    out.push(expand_word_with_params(&argv[0], shell_vars, last_exit, positional_params));
    for word in &argv[1..] {
        out.push(expand_word_with_params(word, shell_vars, last_exit, positional_params));
    }
    out
}

/// Expand variables in a full command string (before parsing).
/// Used for alias-expanded lines and `eval`.
pub fn expand_line(line: &str, shell_vars: &HashMap<String, String>, last_exit: i32) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            // Single-quoted strings: no expansion
            '\'' => {
                result.push('\'');
                i += 1;
                while i < len && chars[i] != '\'' {
                    result.push(chars[i]);
                    i += 1;
                }
                if i < len {
                    result.push('\'');
                    i += 1;
                }
            }
            // Double-quoted strings: expand $VAR inside
            '"' => {
                result.push('"');
                i += 1;
                let mut inner = String::new();
                while i < len && chars[i] != '"' {
                    inner.push(chars[i]);
                    i += 1;
                }
                result.push_str(&expand_word(&inner, shell_vars, last_exit));
                if i < len {
                    result.push('"');
                    i += 1;
                }
            }
            // Backslash escape: pass through verbatim
            '\\' => {
                result.push('\\');
                i += 1;
                if i < len {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            '$' => {
                // Collect the $... token by peeking ahead to a word boundary
                let start = i;
                let mut fake = String::from("$");
                i += 1;
                if i < len {
                    match chars[i] {
                        '{' => {
                            fake.push('{');
                            i += 1;
                            while i < len && chars[i] != '}' {
                                fake.push(chars[i]);
                                i += 1;
                            }
                            if i < len {
                                fake.push('}');
                                i += 1;
                            }
                        }
                        c if c.is_ascii_alphanumeric()
                            || c == '_'
                            || c == '?'
                            || c == '$'
                            || c == '!' =>
                        {
                            fake.push(c);
                            i += 1;
                            if c.is_ascii_alphanumeric() || c == '_' {
                                while i < len
                                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_')
                                {
                                    fake.push(chars[i]);
                                    i += 1;
                                }
                            }
                        }
                        _ => {
                            // bare $ not followed by a valid char
                            result.push('$');
                            continue;
                        }
                    }
                }
                let _ = start;
                result.push_str(&expand_word(&fake, shell_vars, last_exit));
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }

    result
}

/// Expand aliases in a command line. Only the first word is subject to alias
/// substitution (matching bash behaviour). Returns the expanded line.
pub fn expand_alias<'a>(
    line: &'a str,
    aliases: &HashMap<String, String>,
) -> std::borrow::Cow<'a, str> {
    let trimmed = line.trim_start();
    // Find the first word boundary
    let first_word_end = trimmed
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(trimmed.len());
    let first_word = &trimmed[..first_word_end];
    let rest = &trimmed[first_word_end..];

    if let Some(expansion) = aliases.get(first_word) {
        std::borrow::Cow::Owned(format!("{}{}", expansion, rest))
    } else {
        std::borrow::Cow::Borrowed(line)
    }
}
