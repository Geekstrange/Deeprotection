use nix::sys::signal::Signal;
use nix::unistd::Pid;

pub fn builtin_kill(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("dpshell: kill: usage: kill [-SIGNAL] PID...");
        return 1;
    }

    let (signal, pid_args) = parse_kill_signal(args);
    let signal = match signal {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dpshell: kill: {}", e);
            return 1;
        }
    };

    let mut rc = 0;
    for pid_str in pid_args {
        match pid_str.parse::<i32>() {
            Ok(pid) => {
                if let Err(e) = nix::sys::signal::kill(Pid::from_raw(pid), signal) {
                    eprintln!("dpshell: kill: ({}) - {}", pid, e);
                    rc = 1;
                }
            }
            Err(_) => {
                eprintln!(
                    "dpshell: kill: {}: arguments must be process or job IDs",
                    pid_str
                );
                rc = 1;
            }
        }
    }
    rc
}

fn parse_kill_signal<'a>(args: &'a [String]) -> (Result<Signal, String>, &'a [String]) {
    if args[0].starts_with('-') {
        let sig_str = args[0].trim_start_matches('-').trim_start_matches("SIG");
        let signal = sig_str
            .parse::<i32>()
            .ok()
            .and_then(|n| Signal::try_from(n).ok())
            .or_else(|| signal_from_name(sig_str));
        match signal {
            Some(s) => (Ok(s), &args[1..]),
            None => (
                Err(format!("{}: invalid signal specification", args[0])),
                &args[1..],
            ),
        }
    } else {
        (Ok(Signal::SIGTERM), args)
    }
}

fn signal_from_name(name: &str) -> Option<Signal> {
    match name.to_ascii_uppercase().as_str() {
        "HUP" | "1" => Some(Signal::SIGHUP),
        "INT" | "2" => Some(Signal::SIGINT),
        "QUIT" | "3" => Some(Signal::SIGQUIT),
        "KILL" | "9" => Some(Signal::SIGKILL),
        "TERM" | "15" => Some(Signal::SIGTERM),
        "STOP" | "19" => Some(Signal::SIGSTOP),
        "CONT" | "18" => Some(Signal::SIGCONT),
        "USR1" | "10" => Some(Signal::SIGUSR1),
        "USR2" | "12" => Some(Signal::SIGUSR2),
        _ => None,
    }
}
