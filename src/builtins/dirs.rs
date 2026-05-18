use super::helpers::tilde_collapse;
use crate::shell::DpShell;
use std::env;
use std::path::PathBuf;

pub fn builtin_dirs(args: &[String], state: &DpShell) -> i32 {
    let verbose = args.iter().any(|a| a == "-v");
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("?"));

    let mut stack = vec![cwd];
    stack.extend(state.dir_stack.iter().rev().cloned());

    if verbose {
        for (i, dir) in stack.iter().enumerate() {
            println!("{:2}  {}", i, tilde_collapse(dir));
        }
    } else {
        let parts: Vec<String> = stack.iter().map(|d| tilde_collapse(d)).collect();
        println!("{}", parts.join("  "));
    }
    0
}

pub fn builtin_pushd(args: &[String], state: &mut DpShell) -> i32 {
    let cwd = match env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dpshell: pushd: {}", e);
            return 1;
        }
    };

    match args.first().map(String::as_str) {
        None => {
            if let Some(top) = state.dir_stack.pop() {
                state.dir_stack.push(cwd.clone());
                if let Err(e) = env::set_current_dir(&top) {
                    eprintln!("dpshell: pushd: {}: {}", top.display(), e);
                    state.dir_stack.pop();
                    state.dir_stack.push(cwd);
                    return 1;
                }
            } else {
                eprintln!("dpshell: pushd: no other directory");
                return 1;
            }
        }
        Some(dir) => {
            state.dir_stack.push(cwd);
            if let Err(e) = env::set_current_dir(dir) {
                eprintln!("dpshell: pushd: {}: {}", dir, e);
                state.dir_stack.pop();
                return 1;
            }
        }
    }

    builtin_dirs(&[], state)
}

pub fn builtin_popd(_args: &[String], state: &mut DpShell) -> i32 {
    match state.dir_stack.pop() {
        None => {
            eprintln!("dpshell: popd: directory stack empty");
            1
        }
        Some(dir) => {
            if let Err(e) = env::set_current_dir(&dir) {
                eprintln!("dpshell: popd: {}: {}", dir.display(), e);
                state.dir_stack.push(dir);
                return 1;
            }
            builtin_dirs(&[], state)
        }
    }
}

pub fn builtin_umask(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None => {
            let mask = unsafe { libc::umask(0) };
            unsafe {
                libc::umask(mask);
            }
            println!("{:04o}", mask);
            0
        }
        Some(mask_str) => match u32::from_str_radix(mask_str.trim_start_matches('0'), 8) {
            Ok(mask) if mask <= 0o777 => {
                unsafe {
                    libc::umask(mask as libc::mode_t);
                }
                0
            }
            _ => {
                eprintln!("dpshell: umask: {}: invalid octal mask", mask_str);
                1
            }
        },
    }
}
