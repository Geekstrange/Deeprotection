use crate::shell::DpShell;

pub fn builtin_enable(args: &[String], _shell: &mut DpShell) -> i32 {
    if args.is_empty() {
        for name in crate::builtins::helpers::ALL_BUILTINS {
            println!("enable {}", name);
        }
        return 0;
    }

    for arg in args {
        if arg == "-n" || arg == "-a" || arg == "-p" {
            continue;
        }
        if !crate::builtins::helpers::ALL_BUILTINS.contains(&arg.as_str()) {
            eprintln!("dpshell: enable: {}: not a shell builtin", arg);
            return 1;
        }
    }
    0
}

pub fn builtin_shopt(args: &[String], _shell: &mut DpShell) -> i32 {
    static KNOWN_OPTS: &[&str] = &[
        "autocd",
        "cdable_vars",
        "cdspell",
        "checkhash",
        "checkjobs",
        "checkwinsize",
        "cmdhist",
        "compat31",
        "compat32",
        "compat40",
        "compat41",
        "compat42",
        "compat43",
        "compat44",
        "complete_fullquote",
        "direxpand",
        "dirspell",
        "dotglob",
        "execfail",
        "expand_aliases",
        "extdebug",
        "extglob",
        "extquote",
        "failglob",
        "force_fignore",
        "globasciiranges",
        "globstar",
        "gnu_errfmt",
        "histappend",
        "histreedit",
        "histverify",
        "hostcomplete",
        "huponexit",
        "inherit_errexit",
        "interactive_comments",
        "lastpipe",
        "lithist",
        "localvar_inherit",
        "localvar_unset",
        "login_shell",
        "mailwarn",
        "no_empty_cmd_completion",
        "nocaseglob",
        "nocasematch",
        "nullglob",
        "progcomp",
        "progcomp_alias",
        "promptvars",
        "restricted_shell",
        "shift_verbose",
        "sourcepath",
        "xpg_echo",
    ];

    if args.is_empty() {
        for opt in KNOWN_OPTS {
            println!("{:30} off", opt);
        }
        return 0;
    }

    let mut set_mode = true;
    let mut rc = 0;

    for arg in args {
        match arg.as_str() {
            "-s" => set_mode = true,
            "-u" => set_mode = false,
            "-p" | "-q" => {}
            name => {
                if KNOWN_OPTS.contains(&name) {
                    println!("{:30} {}", name, if set_mode { "on" } else { "off" });
                } else {
                    eprintln!("dpshell: shopt: {}: invalid shell option name", name);
                    rc = 1;
                }
            }
        }
    }
    rc
}
