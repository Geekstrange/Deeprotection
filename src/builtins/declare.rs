use crate::shell::DpShell;
use std::env;

pub fn builtin_declare(args: &[String], shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        let mut pairs: Vec<(String, String)> = env::vars().collect();
        for (k, v) in &shell.shell_vars {
            if !pairs.iter().any(|(ek, _)| ek == k) {
                pairs.push((k.clone(), v.clone()));
            }
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in pairs {
            println!("declare -- {}=\"{}\"", k, v);
        }
        return 0;
    }

    let mut print_mode = false;
    let mut export_flag = false;
    let mut readonly_flag = false;
    let mut _integer_flag = false;
    let mut names_only = false;
    let mut remaining: Vec<&String> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg.starts_with('-') || arg.starts_with('+') {
            let negate = arg.starts_with('+');
            for ch in arg[1..].chars() {
                match ch {
                    'p' => print_mode = true,
                    'x' => export_flag = !negate,
                    'r' => readonly_flag = !negate,
                    'i' => _integer_flag = !negate,
                    'f' | 'F' => names_only = true,
                    'a' | 'A' | 'n' | 'l' | 'u' | 'c' | 't' | 'g' => {}
                    _ => {
                        eprintln!("dpshell: declare: -{}: invalid option", ch);
                        return 2;
                    }
                }
            }
        } else {
            remaining.push(arg);
        }
    }

    if print_mode && remaining.is_empty() {
        if names_only {
            for name in shell.functions.borrow().keys() {
                println!("declare -f {}", name);
            }
        } else {
            let mut pairs: Vec<(String, String)> = env::vars().collect();
            pairs.sort();
            for (k, v) in pairs {
                let flags = if shell.readonly_vars.contains(&k) {
                    "-r"
                } else {
                    "--"
                };
                println!("declare {} {}=\"{}\"", flags, k, v);
            }
        }
        return 0;
    }

    let mut rc = 0;
    for arg in remaining {
        if let Some((name, value)) = arg.split_once('=') {
            if shell.readonly_vars.contains(name) {
                eprintln!("dpshell: declare: {}: readonly variable", name);
                rc = 1;
                continue;
            }
            if export_flag {
                unsafe { env::set_var(name, value) };
            } else {
                shell.shell_vars.insert(name.to_string(), value.to_string());
            }
            if readonly_flag {
                shell.readonly_vars.insert(name.to_string());
            }
        } else {
            if print_mode {
                if let Some(val) = shell.shell_vars.get(arg.as_str()) {
                    println!("declare -- {}=\"{}\"", arg, val);
                } else if let Ok(val) = env::var(arg) {
                    println!("declare -x {}=\"{}\"", arg, val);
                } else {
                    eprintln!("dpshell: declare: {}: not found", arg);
                    rc = 1;
                }
            } else {
                shell
                    .shell_vars
                    .entry(arg.to_string())
                    .or_insert_with(String::new);
                if export_flag {
                    let val = shell
                        .shell_vars
                        .get(arg.as_str())
                        .cloned()
                        .unwrap_or_default();
                    unsafe { env::set_var(arg, &val) };
                }
                if readonly_flag {
                    shell.readonly_vars.insert(arg.to_string());
                }
            }
        }
    }
    rc
}

pub fn builtin_let(args: &[String], shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        eprintln!("dpshell: let: expression expected");
        return 1;
    }

    let mut last_result: i64 = 0;
    for expr in args {
        last_result = eval_arithmetic(expr, shell);
    }
    if last_result == 0 {
        1
    } else {
        0
    }
}

fn eval_arithmetic(expr: &str, shell: &mut DpShell) -> i64 {
    if let Some((name, rhs)) = expr.split_once('=') {
        let name = name.trim();
        let rhs = rhs.trim();
        let val = simple_arith_eval(rhs, shell);
        shell.shell_vars.insert(name.to_string(), val.to_string());
        val
    } else {
        simple_arith_eval(expr, shell)
    }
}

fn simple_arith_eval(expr: &str, shell: &DpShell) -> i64 {
    let expr = expr.trim();
    if let Ok(n) = expr.parse::<i64>() {
        return n;
    }
    if let Some(val) = shell.shell_vars.get(expr) {
        return val.parse::<i64>().unwrap_or(0);
    }
    if let Ok(val) = std::env::var(expr) {
        return val.parse::<i64>().unwrap_or(0);
    }

    if let Some(pos) = expr.rfind('+') {
        if pos > 0 {
            let l = simple_arith_eval(&expr[..pos], shell);
            let r = simple_arith_eval(&expr[pos + 1..], shell);
            return l + r;
        }
    }
    if let Some(pos) = expr.rfind('-') {
        if pos > 0 {
            let l = simple_arith_eval(&expr[..pos], shell);
            let r = simple_arith_eval(&expr[pos + 1..], shell);
            return l - r;
        }
    }
    if let Some(pos) = expr.rfind('*') {
        if pos > 0 {
            let l = simple_arith_eval(&expr[..pos], shell);
            let r = simple_arith_eval(&expr[pos + 1..], shell);
            return l * r;
        }
    }
    if let Some(pos) = expr.rfind('/') {
        if pos > 0 {
            let l = simple_arith_eval(&expr[..pos], shell);
            let r = simple_arith_eval(&expr[pos + 1..], shell);
            if r != 0 {
                return l / r;
            }
        }
    }
    0
}
