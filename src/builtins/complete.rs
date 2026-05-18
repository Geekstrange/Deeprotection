use crate::shell::DpShell;

pub fn builtin_complete(args: &[String], _shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        return 0;
    }
    let mut print_mode = false;
    let mut remove_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-p" => print_mode = true,
            "-r" => remove_mode = true,
            "-F" | "-C" | "-G" | "-W" | "-X" | "-P" | "-S" | "-o" | "-A" => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if print_mode || remove_mode {
        return 0;
    }
    0
}

pub fn builtin_compgen(args: &[String], _shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        return 1;
    }
    let mut action: Option<&str> = None;
    let mut word = "";
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-A" => {
                i += 1;
                action = args.get(i).map(String::as_str);
            }
            "-W" => {
                i += 1;
            }
            other if !other.starts_with('-') => {
                word = other;
            }
            _ => {}
        }
        i += 1;
    }
    match action {
        Some("command") => {
            let path = std::env::var("PATH").unwrap_or_default();
            for dir in path.split(':') {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if word.is_empty() || name.starts_with(word) {
                            println!("{}", name);
                        }
                    }
                }
            }
        }
        Some("file") | Some("default") => {
            if let Ok(entries) = std::fs::read_dir(".") {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if word.is_empty() || name.starts_with(word) {
                        println!("{}", name);
                    }
                }
            }
        }
        Some("directory") => {
            if let Ok(entries) = std::fs::read_dir(".") {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if word.is_empty() || name.starts_with(word) {
                            println!("{}", name);
                        }
                    }
                }
            }
        }
        Some("builtin") => {
            for name in crate::builtins::helpers::ALL_BUILTINS {
                if word.is_empty() || name.starts_with(word) {
                    println!("{}", name);
                }
            }
        }
        _ => {}
    }
    0
}

pub fn builtin_compopt(_args: &[String], _shell: &mut DpShell) -> i32 {
    0
}
