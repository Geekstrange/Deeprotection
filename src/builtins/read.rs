use crate::shell::DpShell;
use clap::Parser;
use std::io::{BufRead, Read};

/// Read a line from standard input and split into variables.
#[derive(Parser)]
#[clap(disable_help_flag = true)]
struct ReadArgs {
    /// Do not treat backslash as an escape character.
    #[arg(short = 'r')]
    raw: bool,

    /// Use PROMPT as the prompt string.
    #[arg(short = 'p', value_name = "PROMPT")]
    prompt: Option<String>,

    /// Read into array variable.
    #[arg(short = 'a', value_name = "ARRAY")]
    array_var: Option<String>,

    /// Only read N characters.
    #[arg(short = 'n', value_name = "NCHARS")]
    nchars: Option<usize>,

    /// Silent mode (don't echo input — for passwords).
    #[arg(short = 's')]
    silent: bool,

    /// Variable names to read into.
    #[arg(trailing_var_arg = true)]
    variables: Vec<String>,
}

pub fn builtin_read(args: &[String], state: &mut DpShell) -> i32 {
    let all_args = std::iter::once("read".to_string()).chain(args.iter().cloned());

    let parsed = match ReadArgs::try_parse_from(all_args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dpshell: read: {}", e);
            return 2;
        }
    };

    if let Some(ref prompt) = parsed.prompt {
        eprint!("{}", prompt);
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }

    let line = if let Some(nchars) = parsed.nchars {
        let mut buf = vec![0u8; nchars];
        let stdin = std::io::stdin();
        match stdin.lock().read(&mut buf) {
            Ok(n) => String::from_utf8_lossy(&buf[..n]).to_string(),
            Err(_) => return 1,
        }
    } else {
        let mut line = String::new();
        let stdin = std::io::stdin();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => return 1, // EOF
            Ok(_) => {}
            Err(_) => return 1,
        }
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }

        if !parsed.raw {
            line = line.replace("\\\n", "");
        }
        line
    };

    let var_names = if parsed.variables.is_empty() {
        vec!["REPLY".to_string()]
    } else {
        parsed.variables.clone()
    };

    if var_names.len() == 1 {
        state.shell_vars.insert(var_names[0].clone(), line);
    } else {
        let ifs = std::env::var("IFS").unwrap_or_else(|_| " \t\n".to_string());
        let parts: Vec<&str> = if ifs.is_empty() {
            vec![line.as_str()]
        } else {
            line.splitn(var_names.len(), |c: char| ifs.contains(c))
                .collect()
        };

        for (i, name) in var_names.iter().enumerate() {
            let value = if i < parts.len() {
                if i == var_names.len() - 1 && parts.len() > var_names.len() {
                    parts[i..].join(&ifs[..1.min(ifs.len())])
                } else {
                    parts[i].to_string()
                }
            } else {
                String::new()
            };
            state.shell_vars.insert(name.clone(), value);
        }
    }

    0
}
