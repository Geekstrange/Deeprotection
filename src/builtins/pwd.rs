use crate::shell::DpShell;
use clap::Parser;
use std::env;

/// Display the current working directory.
#[derive(Parser)]
struct PwdArgs {
    /// Print the physical directory without any symlinks.
    #[arg(short = 'P', overrides_with = "allow_symlinks")]
    physical: bool,

    /// Print $PWD if it names the current working directory.
    #[arg(short = 'L', overrides_with = "physical")]
    allow_symlinks: bool,
}

pub fn builtin_pwd(args: &[String], _state: &mut DpShell) -> i32 {
    let all_args = std::iter::once("pwd".to_string()).chain(args.iter().cloned());

    let parsed = match PwdArgs::try_parse_from(all_args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 2;
        }
    };

    let cwd = match env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dpshell: pwd: {}", e);
            return 1;
        }
    };

    let display_path = if parsed.physical {
        match cwd.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("dpshell: pwd: {}", e);
                return 1;
            }
        }
    } else {
        cwd
    };

    println!("{}", display_path.display());
    0
}
