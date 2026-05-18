use crate::shell::DpShell;
use std::io::BufRead;

pub fn builtin_mapfile(args: &[String], shell: &mut DpShell) -> i32 {
    let mut array_name = "MAPFILE".to_string();
    let mut skip_count: usize = 0;
    let mut max_count: usize = 0;
    let mut strip_trailing = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-t" => strip_trailing = true,
            "-n" => {
                i += 1;
                max_count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "-s" => {
                i += 1;
                skip_count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            other if !other.starts_with('-') => {
                array_name = other.to_string();
            }
            _ => {}
        }
        i += 1;
    }

    let stdin = std::io::stdin();
    let mut lines: Vec<String> = Vec::new();
    let mut skipped = 0;
    let mut counted = 0;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if skipped < skip_count {
            skipped += 1;
            continue;
        }

        let line = if strip_trailing {
            line.trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string()
        } else {
            line
        };

        lines.push(line);
        counted += 1;

        if max_count > 0 && counted >= max_count {
            break;
        }
    }

    for (idx, line) in lines.iter().enumerate() {
        let key = format!("{}_{}", array_name, idx);
        shell.shell_vars.insert(key, line.clone());
    }
    shell
        .shell_vars
        .insert(format!("{}_len", array_name), lines.len().to_string());

    0
}

pub fn builtin_readarray(args: &[String], shell: &mut DpShell) -> i32 {
    builtin_mapfile(args, shell)
}
