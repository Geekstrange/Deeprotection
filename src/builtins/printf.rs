use crate::shell::DpShell;
use std::io::Write;

pub fn builtin_printf(args: &[String], _shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        eprintln!("dpshell: printf: usage: printf FORMAT [ARGUMENTS...]");
        return 1;
    }

    let format_str = &args[0];
    let fmt_args = &args[1..];
    let mut arg_idx = 0;

    let mut output = String::new();
    let mut chars = format_str.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => output.push('\n'),
                Some('t') => output.push('\t'),
                Some('r') => output.push('\r'),
                Some('\\') => output.push('\\'),
                Some('a') => output.push('\x07'),
                Some('b') => output.push('\x08'),
                Some('f') => output.push('\x0c'),
                Some('v') => output.push('\x0b'),
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
                        output.push(ch);
                    }
                }
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => output.push('\\'),
            }
        } else if c == '%' {
            match chars.peek() {
                Some('%') => {
                    chars.next();
                    output.push('%');
                }
                _ => {
                    let mut flags = String::new();
                    while let Some(&f) = chars.peek() {
                        if "-+ #0".contains(f) {
                            flags.push(f);
                            chars.next();
                        } else {
                            break;
                        }
                    }

                    let mut width = String::new();
                    while let Some(&d) = chars.peek() {
                        if d.is_ascii_digit() || d == '*' {
                            width.push(d);
                            chars.next();
                        } else {
                            break;
                        }
                    }

                    if chars.peek() == Some(&'.') {
                        chars.next();
                        while let Some(&d) = chars.peek() {
                            if d.is_ascii_digit() || d == '*' {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }

                    let spec = chars.next().unwrap_or('s');
                    let arg = fmt_args.get(arg_idx).map(String::as_str).unwrap_or("");
                    arg_idx += 1;

                    match spec {
                        's' => output.push_str(arg),
                        'd' | 'i' => {
                            let n: i64 = arg.parse().unwrap_or(0);
                            output.push_str(&format!("{}", n));
                        }
                        'u' => {
                            let n: u64 = arg.parse().unwrap_or(0);
                            output.push_str(&format!("{}", n));
                        }
                        'o' => {
                            let n: u64 = arg.parse().unwrap_or(0);
                            output.push_str(&format!("{:o}", n));
                        }
                        'x' => {
                            let n: u64 = arg.parse().unwrap_or(0);
                            output.push_str(&format!("{:x}", n));
                        }
                        'X' => {
                            let n: u64 = arg.parse().unwrap_or(0);
                            output.push_str(&format!("{:X}", n));
                        }
                        'f' | 'g' | 'e' => {
                            let n: f64 = arg.parse().unwrap_or(0.0);
                            output.push_str(&format!("{}", n));
                        }
                        'c' => {
                            if let Some(ch) = arg.chars().next() {
                                output.push(ch);
                            }
                        }
                        'q' => {
                            output.push('\'');
                            output.push_str(&arg.replace('\'', "'\\''"));
                            output.push('\'');
                        }
                        _ => {
                            output.push('%');
                            output.push_str(&flags);
                            output.push_str(&width);
                            output.push(spec);
                        }
                    }
                }
            }
        } else {
            output.push(c);
        }
    }

    print!("{}", output);
    let _ = std::io::stdout().flush();
    0
}
