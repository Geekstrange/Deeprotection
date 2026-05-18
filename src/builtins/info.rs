use super::helpers::ALL_BUILTINS;
use crate::shell::DpShell;
use std::env;
use std::path::PathBuf;

pub fn builtin_type(args: &[String], state: &DpShell) -> i32 {
    let mut rc = 0;
    for name in args {
        if ALL_BUILTINS.contains(&name.as_str()) {
            println!("{} is a shell builtin", name);
        } else if let Some(expansion) = state.aliases.get(name.as_str()) {
            println!("{} is aliased to `{}'", name, expansion);
        } else {
            let path = env::var("PATH").unwrap_or_default();
            match crate::parser::resolve_binary(name, &path) {
                Some(p) => println!("{} is {}", name, p.display()),
                None => {
                    eprintln!("dpshell: type: {}: not found", name);
                    rc = 1;
                }
            }
        }
    }
    rc
}

pub fn builtin_command_v(name: &str) -> Option<PathBuf> {
    let path = env::var("PATH").unwrap_or_default();
    crate::parser::resolve_binary(name, &path)
}

#[allow(dead_code)]
pub fn is_shell_builtin(name: &str) -> bool {
    ALL_BUILTINS.contains(&name)
}

pub fn builtin_help(args: &[String]) -> i32 {
    let filter = args.first().map(String::as_str).unwrap_or("");

    let topics: &[(&str, &str)] = &[
        (":", "Null command; always exits 0."),
        (
            "alias",
            "alias [name[=value]...]  Define or display aliases.",
        ),
        ("bg", "bg [jobspec]  Resume a job in the background."),
        ("break", "break [n]  Exit from a loop."),
        ("builtin", "builtin cmd [args]  Execute cmd as a built-in."),
        ("cd", "cd [-L|-P] [dir]  Change the working directory."),
        (
            "command",
            "command [-v] name  Execute name bypassing aliases.",
        ),
        ("continue", "continue [n]  Skip to the next loop iteration."),
        ("dirs", "dirs [-v]  Display the directory stack."),
        ("eval", "eval [args...]  Execute args as a command."),
        ("exec", "exec [cmd [args]]  Replace the shell with cmd."),
        ("exit", "exit [n]  Exit the shell with status n."),
        ("export", "export [name[=value]...]  Mark names for export."),
        ("fg", "fg [jobspec]  Resume a job in the foreground."),
        ("help", "help [pattern]  Show help for built-in commands."),
        ("history", "history [n]  Display command history."),
        ("jobs", "jobs  List active background/stopped jobs."),
        ("kill", "kill [-SIG] pid...  Send a signal to processes."),
        ("local", "local [name[=value]...]  Declare local variables."),
        ("popd", "popd  Remove top directory from the stack and cd."),
        ("pushd", "pushd [dir]  Push dir onto the directory stack."),
        (
            "readonly",
            "readonly [name[=value]...]  Mark variables read-only.",
        ),
        ("return", "return [n]  Exit from a function."),
        ("set", "set [name=value...]  Set shell variables."),
        ("shift", "shift [n]  Shift positional parameters."),
        (
            "source",
            "source file [args]  Execute file in current shell.",
        ),
        ("test", "test expr  Evaluate a conditional expression."),
        ("trap", "trap [action signal...]  Manage signal handlers."),
        (
            "type",
            "type name...  Describe how each name would be used.",
        ),
        ("umask", "umask [mask]  Set/print the file creation mask."),
        ("unalias", "unalias [-a] [name...]  Remove aliases."),
        ("unset", "unset [name...]  Unset variables."),
        ("wait", "wait [pid...]  Wait for background processes."),
        (".", "source FILE  Execute FILE in the current shell."),
        ("[", "[ expr ]  Evaluate expr (same as test)."),
    ];

    let mut printed = 0;
    for (name, desc) in topics {
        if filter.is_empty() || name.contains(filter) || desc.contains(filter) {
            println!("{}", desc);
            printed += 1;
        }
    }

    if printed == 0 && !filter.is_empty() {
        eprintln!("dpshell: help: no help topics match `{}'", filter);
        return 1;
    }
    0
}
