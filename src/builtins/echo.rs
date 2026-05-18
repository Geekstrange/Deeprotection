use crate::shell::DpShell;
use clap::Parser;

/// Echo text to standard output.
#[derive(Parser)]
#[clap(disable_help_flag = true, disable_version_flag = true)]
struct EchoArgs {
    /// Suppress the trailing newline from the output.
    #[arg(short = 'n')]
    no_trailing_newline: bool,

    /// Interpret backslash escapes in the provided text.
    #[arg(short = 'e')]
    interpret_escapes: bool,

    /// Do not interpret backslash escapes in the provided text.
    #[arg(short = 'E')]
    no_interpret_escapes: bool,

    /// Tokens to echo to standard output.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

pub fn builtin_echo(args: &[String], _state: &mut DpShell) -> i32 {
    let all_args = std::iter::once("echo".to_string()).chain(args.iter().cloned());

    let parsed = match EchoArgs::try_parse_from(all_args) {
        Ok(p) => p,
        Err(_) => {
            // echo never fails on bad flags — just prints them literally
            let output = args.join(" ");
            println!("{}", output);
            return 0;
        }
    };

    let mut trailing_newline = !parsed.no_trailing_newline;

    let text = if parsed.interpret_escapes {
        let mut result = String::new();
        for (i, arg) in parsed.args.iter().enumerate() {
            if i > 0 {
                result.push(' ');
            }
            let (expanded, stop) = expand_echo_escapes(arg);
            result.push_str(&expanded);
            if stop {
                trailing_newline = false;
                break;
            }
        }
        result
    } else {
        parsed.args.join(" ")
    };

    if trailing_newline {
        println!("{}", text);
    } else {
        print!("{}", text);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }

    0
}

fn expand_echo_escapes(s: &str) -> (String, bool) {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            None => {
                result.push('\\');
            }
            Some('\\') => result.push('\\'),
            Some('a') => result.push('\x07'),
            Some('b') => result.push('\x08'),
            Some('c') => return (result, true), // stop output
            Some('e') | Some('E') => result.push('\x1b'),
            Some('f') => result.push('\x0c'),
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some('v') => result.push('\x0b'),
            Some('0') => {
                let mut val: u32 = 0;
                for _ in 0..3 {
                    match chars.peek() {
                        Some(&d) if d >= '0' && d <= '7' => {
                            val = val * 8 + (d as u32 - '0' as u32);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if let Some(ch) = char::from_u32(val) {
                    result.push(ch);
                }
            }
            Some('x') => {
                let mut val: u32 = 0;
                let mut count = 0;
                while count < 2 {
                    match chars.peek() {
                        Some(&d) if d.is_ascii_hexdigit() => {
                            val = val * 16 + d.to_digit(16).unwrap();
                            chars.next();
                            count += 1;
                        }
                        _ => break,
                    }
                }
                if count > 0 {
                    if let Some(ch) = char::from_u32(val) {
                        result.push(ch);
                    }
                } else {
                    result.push_str("\\x");
                }
            }
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
        }
    }

    (result, false)
}
