use crate::shell::DpShell;

pub fn builtin_getopts(args: &[String], shell: &mut DpShell) -> i32 {
    if args.len() < 2 {
        eprintln!("dpshell: getopts: usage: getopts optstring name [arg ...]");
        return 2;
    }

    let optstring = &args[0];
    let varname = &args[1];

    let optind: usize = shell
        .shell_vars
        .get("OPTIND")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let scan_args: Vec<&str> = if args.len() > 2 {
        args[2..].iter().map(String::as_str).collect()
    } else {
        Vec::new()
    };

    if optind < 1 || optind > scan_args.len() {
        shell.shell_vars.insert(varname.clone(), "?".to_string());
        return 1;
    }

    let current = scan_args[optind - 1];
    if !current.starts_with('-') || current == "-" || current == "--" {
        shell.shell_vars.insert(varname.clone(), "?".to_string());
        return 1;
    }

    let opt_char = current.chars().nth(1).unwrap_or('?');
    let opt_pos = optstring.find(opt_char);

    match opt_pos {
        Some(pos) => {
            let needs_arg = optstring.chars().nth(pos + 1) == Some(':');
            shell
                .shell_vars
                .insert(varname.clone(), opt_char.to_string());

            if needs_arg {
                if current.len() > 2 {
                    shell
                        .shell_vars
                        .insert("OPTARG".to_string(), current[2..].to_string());
                } else if optind < scan_args.len() {
                    shell
                        .shell_vars
                        .insert("OPTARG".to_string(), scan_args[optind].to_string());
                    shell
                        .shell_vars
                        .insert("OPTIND".to_string(), (optind + 2).to_string());
                    return 0;
                } else {
                    eprintln!(
                        "dpshell: getopts: option requires an argument -- '{}'",
                        opt_char
                    );
                    shell.shell_vars.insert(varname.clone(), "?".to_string());
                    shell
                        .shell_vars
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    return 1;
                }
            }
            shell
                .shell_vars
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            0
        }
        None => {
            eprintln!("dpshell: getopts: illegal option -- '{}'", opt_char);
            shell.shell_vars.insert(varname.clone(), "?".to_string());
            shell
                .shell_vars
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            1
        }
    }
}
