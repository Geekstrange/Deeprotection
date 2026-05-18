use super::helpers::shell_quote;
use crate::shell::DpShell;
use nix::unistd::Pid;

pub fn builtin_trap(args: &[String], state: &mut DpShell) -> i32 {
    if args.is_empty() {
        let mut entries: Vec<(&String, &String)> = state.traps.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        for (sig, action) in entries {
            println!("trap -- {} {}", shell_quote(action), sig);
        }
        return 0;
    }

    if args.len() < 2 {
        eprintln!("dpshell: trap: usage: trap [ACTION] SIGNAL...");
        return 1;
    }

    let action = &args[0];
    for sig in &args[1..] {
        if action == "-" {
            state.traps.remove(sig);
        } else {
            state.traps.insert(sig.clone(), action.clone());
        }
    }
    0
}

pub fn builtin_wait(args: &[String]) -> i32 {
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

    if args.is_empty() {
        loop {
            match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) | Err(nix::Error::ECHILD) => break,
                _ => {}
            }
        }
        return 0;
    }

    let mut last_status = 0i32;
    for pid_str in args {
        match pid_str.parse::<i32>() {
            Ok(pid) => loop {
                match waitpid(Pid::from_raw(pid), None) {
                    Ok(WaitStatus::Exited(_, code)) => {
                        last_status = code;
                        break;
                    }
                    Ok(WaitStatus::Signaled(_, s, _)) => {
                        last_status = 128 + s as i32;
                        break;
                    }
                    Err(nix::Error::EINTR) => continue,
                    Err(e) => {
                        eprintln!("dpshell: wait: {}: {}", pid, e);
                        last_status = 127;
                        break;
                    }
                    _ => continue,
                }
            },
            Err(_) => {
                eprintln!("dpshell: wait: {}: not a valid PID", pid_str);
                last_status = 1;
            }
        }
    }
    last_status
}

pub fn builtin_break(args: &[String]) -> i32 {
    let _n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    eprintln!("dpshell: break: only meaningful inside a loop");
    0
}

pub fn builtin_continue(args: &[String]) -> i32 {
    let _n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    eprintln!("dpshell: continue: only meaningful inside a loop");
    0
}

pub fn builtin_shift(_args: &[String]) -> i32 {
    0
}
