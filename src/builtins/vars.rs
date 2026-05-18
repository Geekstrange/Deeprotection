use super::helpers::shell_quote;
use crate::shell::DpShell;
use std::env;

pub fn builtin_export(args: &[String], state: &DpShell) -> i32 {
    if args.is_empty() {
        let mut pairs: Vec<(String, String)> = env::vars().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in pairs {
            println!("declare -x {}=\"{}\"", k, v.replace('"', "\\\""));
        }
        return 0;
    }

    let mut rc = 0;
    for arg in args {
        if let Some((k, v)) = arg.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: export: {}: readonly variable", k);
                rc = 1;
                continue;
            }
            unsafe { env::set_var(k, v) };
        }
    }
    rc
}

pub fn builtin_readonly(args: &[String], state: &mut DpShell) -> i32 {
    if args.is_empty() {
        let mut names: Vec<&String> = state.readonly_vars.iter().collect();
        names.sort();
        for name in names {
            let val = env::var(name)
                .or_else(|_| state.shell_vars.get(name).cloned().ok_or(()))
                .unwrap_or_default();
            println!("declare -r {}=\"{}\"", name, val);
        }
        return 0;
    }

    for arg in args {
        let name = if let Some((k, v)) = arg.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: readonly: {}: readonly variable", k);
                continue;
            }
            unsafe { env::set_var(k, v) };
            k.to_string()
        } else {
            arg.clone()
        };
        state.readonly_vars.insert(name);
    }
    0
}

pub fn builtin_set(args: &[String], state: &mut DpShell) -> i32 {
    if args.is_empty() {
        let mut pairs: Vec<(String, String)> = env::vars().collect();
        for (k, v) in &state.shell_vars {
            if !pairs.iter().any(|(ek, _)| ek == k) {
                pairs.push((k.clone(), v.clone()));
            }
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in pairs {
            println!("{}={}", k, shell_quote(&v));
        }
        return 0;
    }

    let mut rc = 0;
    for arg in args {
        if arg.starts_with('-') || arg.starts_with('+') {
            continue;
        }
        if let Some((k, v)) = arg.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: set: {}: readonly variable", k);
                rc = 1;
                continue;
            }
            state.shell_vars.insert(k.to_string(), v.to_string());
        }
    }
    rc
}

pub fn builtin_unset(args: &[String], state: &mut DpShell) -> i32 {
    let mut rc = 0;
    for name in args {
        if state.readonly_vars.contains(name.as_str()) {
            eprintln!("dpshell: unset: {}: cannot unset: readonly variable", name);
            rc = 1;
            continue;
        }
        unsafe { env::remove_var(name) };
        state.shell_vars.remove(name);
    }
    rc
}

pub fn builtin_local(args: &[String], state: &mut DpShell) -> i32 {
    builtin_set(args, state)
}
