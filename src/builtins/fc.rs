use crate::shell::DpShell;

pub fn builtin_fc(args: &[String], shell: &mut DpShell) -> i32 {
    if shell.history.is_empty() {
        eprintln!("dpshell: fc: history is empty");
        return 1;
    }

    let mut list_mode = false;
    let mut reverse = false;
    let mut suppress_numbers = false;
    let mut first: Option<String> = None;
    let mut last: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" => list_mode = true,
            "-r" => reverse = true,
            "-n" => suppress_numbers = true,
            "-s" => {
                let cmd = if i + 1 < args.len() {
                    args[i + 1].clone()
                } else {
                    shell.history.last().cloned().unwrap_or_default()
                };
                println!("{}", cmd);
                return 0;
            }
            "-e" => {
                i += 1;
            }
            other => {
                if first.is_none() {
                    first = Some(other.to_string());
                } else if last.is_none() {
                    last = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    let len = shell.history.len();

    let start = match &first {
        Some(s) => match s.parse::<isize>() {
            Ok(n) if n < 0 => (len as isize + n).max(0) as usize,
            Ok(n) if n > 0 => ((n as usize) - 1).min(len - 1),
            _ => len.saturating_sub(16),
        },
        None => len.saturating_sub(16),
    };

    let end = match &last {
        Some(s) => match s.parse::<isize>() {
            Ok(n) if n < 0 => ((len as isize + n) as usize).min(len),
            Ok(n) if n > 0 => (n as usize).min(len),
            _ => len,
        },
        None => len,
    };

    if list_mode {
        let range: Vec<usize> = if reverse {
            (start..end).rev().collect()
        } else {
            (start..end).collect()
        };
        for idx in range {
            if suppress_numbers {
                println!("\t{}", shell.history[idx]);
            } else {
                println!("{}\t{}", idx + 1, shell.history[idx]);
            }
        }
        return 0;
    }

    if let Some(cmd) = shell.history.last().cloned() {
        println!("{}", cmd);
    }
    0
}
