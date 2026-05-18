use crate::shell::DpShell;
use std::collections::HashMap;

pub type BuiltinFn = fn(&[String], &mut DpShell) -> i32;

#[allow(dead_code)]
pub struct Registration {
    pub execute: BuiltinFn,
    pub help: &'static str,
    pub special: bool,
}

pub fn default_builtins() -> HashMap<String, Registration> {
    let mut m = HashMap::new();

    use super::*;

    // ── Basic operations ──────────────────────────────────────────────
    m.insert(
        ":".into(),
        Registration {
            execute: |args, _| builtin_colon(args),
            help: "Null command; always exits 0.",
            special: true,
        },
    );
    m.insert(
        "history".into(),
        Registration {
            execute: |args, s| builtin_history(args, s),
            help: "history [n]  Display command history.",
            special: false,
        },
    );
    m.insert(
        "kill".into(),
        Registration {
            execute: |args, _| builtin_kill(args),
            help: "kill [-SIG] pid...  Send a signal to processes.",
            special: false,
        },
    );

    // ── Variables & Environment ───────────────────────────────────────
    m.insert(
        "export".into(),
        Registration {
            execute: |args, s| builtin_export(args, s),
            help: "export [name[=value]...]  Mark names for export.",
            special: true,
        },
    );
    m.insert(
        "readonly".into(),
        Registration {
            execute: |args, s| builtin_readonly(args, s),
            help: "readonly [name[=value]...]  Mark variables read-only.",
            special: true,
        },
    );
    m.insert(
        "set".into(),
        Registration {
            execute: |args, s| builtin_set(args, s),
            help: "set [name=value...]  Set shell variables.",
            special: true,
        },
    );
    m.insert(
        "unset".into(),
        Registration {
            execute: |args, s| builtin_unset(args, s),
            help: "unset [name...]  Unset variables.",
            special: true,
        },
    );
    m.insert(
        "local".into(),
        Registration {
            execute: |args, s| builtin_local(args, s),
            help: "local [name[=value]...]  Declare local variables.",
            special: false,
        },
    );

    // ── Execution & Debugging ────────────────────────────────────────
    m.insert(
        "trap".into(),
        Registration {
            execute: |args, s| builtin_trap(args, s),
            help: "trap [action signal...]  Manage signal handlers.",
            special: true,
        },
    );
    m.insert(
        "wait".into(),
        Registration {
            execute: |args, _| builtin_wait(args),
            help: "wait [pid...]  Wait for background processes.",
            special: false,
        },
    );

    // ── Script Control ───────────────────────────────────────────────
    m.insert(
        "break".into(),
        Registration {
            execute: |args, _| builtin_break(args),
            help: "break [n]  Exit from a loop.",
            special: true,
        },
    );
    m.insert(
        "continue".into(),
        Registration {
            execute: |args, _| builtin_continue(args),
            help: "continue [n]  Skip to the next loop iteration.",
            special: true,
        },
    );
    m.insert(
        "shift".into(),
        Registration {
            execute: |args, _| builtin_shift(args),
            help: "shift [n]  Shift positional parameters.",
            special: true,
        },
    );
    m.insert(
        "test".into(),
        Registration {
            execute: |args, _| builtin_test(args),
            help: "test expr  Evaluate a conditional expression.",
            special: false,
        },
    );
    m.insert(
        "[".into(),
        Registration {
            execute: |args, _| builtin_test(args),
            help: "[ expr ]  Evaluate expr (same as test).",
            special: false,
        },
    );

    // ── Directory Stack ──────────────────────────────────────────────
    m.insert(
        "dirs".into(),
        Registration {
            execute: |args, s| builtin_dirs(args, s),
            help: "dirs [-v]  Display the directory stack.",
            special: false,
        },
    );
    m.insert(
        "pushd".into(),
        Registration {
            execute: |args, s| builtin_pushd(args, s),
            help: "pushd [dir]  Push dir onto the directory stack.",
            special: false,
        },
    );
    m.insert(
        "popd".into(),
        Registration {
            execute: |args, s| builtin_popd(args, s),
            help: "popd  Remove top directory from the stack and cd.",
            special: false,
        },
    );
    m.insert(
        "umask".into(),
        Registration {
            execute: |args, _| builtin_umask(args),
            help: "umask [mask]  Set/print the file creation mask.",
            special: false,
        },
    );

    // ── Alias ────────────────────────────────────────────────────────
    m.insert(
        "alias".into(),
        Registration {
            execute: |args, s| builtin_alias(args, s),
            help: "alias [name[=value]...]  Define or display aliases.",
            special: false,
        },
    );
    m.insert(
        "unalias".into(),
        Registration {
            execute: |args, s| builtin_unalias(args, s),
            help: "unalias [-a] [name...]  Remove aliases.",
            special: false,
        },
    );

    // ── Information ──────────────────────────────────────────────────
    m.insert(
        "help".into(),
        Registration {
            execute: |args, _| builtin_help(args),
            help: "help [pattern]  Show help for built-in commands.",
            special: false,
        },
    );
    m.insert(
        "type".into(),
        Registration {
            execute: |args, s| builtin_type(args, s),
            help: "type name...  Describe how each name would be used.",
            special: false,
        },
    );

    // ── Ported from brush-builtins ──────────────────────────────────
    m.insert(
        "echo".into(),
        Registration {
            execute: |args, s| builtin_echo(args, s),
            help: "echo [-neE] [args...]  Display a line of text.",
            special: false,
        },
    );
    m.insert(
        "pwd".into(),
        Registration {
            execute: |args, s| builtin_pwd(args, s),
            help: "pwd [-LP]  Print the current working directory.",
            special: false,
        },
    );
    m.insert(
        "read".into(),
        Registration {
            execute: |args, s| builtin_read(args, s),
            help: "read [-rpsan] [name...]  Read a line from stdin.",
            special: false,
        },
    );

    // ── Newly ported builtins ───────────────────────────────────────
    m.insert(
        "true".into(),
        Registration {
            execute: builtin_true,
            help: "true  Return successful result.",
            special: false,
        },
    );
    m.insert(
        "false".into(),
        Registration {
            execute: builtin_false,
            help: "false  Return unsuccessful result.",
            special: false,
        },
    );
    m.insert(
        "suspend".into(),
        Registration {
            execute: builtin_suspend,
            help: "suspend  Suspend shell execution.",
            special: false,
        },
    );
    m.insert(
        "times".into(),
        Registration {
            execute: builtin_times,
            help: "times  Display process times.",
            special: true,
        },
    );
    m.insert(
        "caller".into(),
        Registration {
            execute: builtin_caller,
            help: "caller [expr]  Return calling context.",
            special: false,
        },
    );
    m.insert(
        "printf".into(),
        Registration {
            execute: builtin_printf,
            help: "printf FORMAT [ARGS...]  Format and print.",
            special: false,
        },
    );
    m.insert(
        "declare".into(),
        Registration {
            execute: builtin_declare,
            help: "declare [-aAfFgiIlnrtux] [name[=value]...]  Declare variables.",
            special: false,
        },
    );
    m.insert(
        "typeset".into(),
        Registration {
            execute: builtin_declare,
            help: "typeset [-aAfFgiIlnrtux] [name[=value]...]  Declare variables.",
            special: false,
        },
    );
    m.insert(
        "let".into(),
        Registration {
            execute: builtin_let,
            help: "let EXPRESSION  Evaluate arithmetic expression.",
            special: false,
        },
    );
    m.insert(
        "getopts".into(),
        Registration {
            execute: builtin_getopts,
            help: "getopts OPTSTRING NAME [ARGS]  Parse option arguments.",
            special: false,
        },
    );
    m.insert(
        "hash".into(),
        Registration {
            execute: builtin_hash,
            help: "hash [-r] [name...]  Remember command locations.",
            special: false,
        },
    );
    m.insert(
        "enable".into(),
        Registration {
            execute: builtin_enable,
            help: "enable [-n] [name...]  Enable/disable builtins.",
            special: false,
        },
    );
    m.insert(
        "shopt".into(),
        Registration {
            execute: builtin_shopt,
            help: "shopt [-su] [optname...]  Set/unset shell options.",
            special: false,
        },
    );
    m.insert(
        "ulimit".into(),
        Registration {
            execute: builtin_ulimit,
            help: "ulimit [-SH] [-a] [-cdfmnstuv] [limit]  Resource limits.",
            special: false,
        },
    );
    m.insert(
        "mapfile".into(),
        Registration {
            execute: builtin_mapfile,
            help: "mapfile [-tn] [array]  Read lines into array.",
            special: false,
        },
    );
    m.insert(
        "readarray".into(),
        Registration {
            execute: builtin_readarray,
            help: "readarray [-tn] [array]  Read lines into array.",
            special: false,
        },
    );
    m.insert(
        "fc".into(),
        Registration {
            execute: builtin_fc,
            help: "fc [-lnr] [first] [last]  Display/edit history.",
            special: false,
        },
    );
    m.insert(
        "complete".into(),
        Registration {
            execute: builtin_complete,
            help: "complete [options] [name]  Programmable completion.",
            special: false,
        },
    );
    m.insert(
        "compgen".into(),
        Registration {
            execute: builtin_compgen,
            help: "compgen [options] [word]  Generate completions.",
            special: false,
        },
    );
    m.insert(
        "compopt".into(),
        Registration {
            execute: builtin_compopt,
            help: "compopt [options] [name]  Modify completion options.",
            special: false,
        },
    );
    m.insert(
        "bind".into(),
        Registration {
            execute: builtin_bind,
            help: "bind [-lpvsPSVX] [keyseq:function]  Key bindings.",
            special: false,
        },
    );

    m
}
