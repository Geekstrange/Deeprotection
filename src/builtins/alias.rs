use super::helpers::shell_quote;
use crate::shell::DpShell;

pub fn builtin_alias(args: &[String], state: &mut DpShell) -> i32 {
    if args.is_empty() {
        let mut entries: Vec<(&String, &String)> = state.aliases.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        for (name, val) in entries {
            println!("alias {}={}", name, shell_quote(val));
        }
        return 0;
    }

    for arg in args {
        if let Some((name, val)) = arg.split_once('=') {
            state.aliases.insert(name.to_string(), val.to_string());
        } else {
            match state.aliases.get(arg.as_str()) {
                Some(val) => println!("alias {}={}", arg, shell_quote(val)),
                None => {
                    eprintln!("dpshell: alias: {}: not found", arg);
                }
            }
        }
    }
    0
}

pub fn builtin_unalias(args: &[String], state: &mut DpShell) -> i32 {
    if args.first().map(String::as_str) == Some("-a") {
        state.aliases.clear();
        return 0;
    }
    let mut rc = 0;
    for name in args {
        if state.aliases.remove(name).is_none() {
            eprintln!("dpshell: unalias: {}: not found", name);
            rc = 1;
        }
    }
    rc
}
