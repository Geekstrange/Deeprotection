#![allow(dead_code)]
use crate::shell::DpShell;

pub fn builtin_true(_args: &[String], _shell: &mut DpShell) -> i32 {
    0
}

pub fn builtin_false(_args: &[String], _shell: &mut DpShell) -> i32 {
    1
}

pub fn builtin_return(args: &[String], _shell: &mut DpShell) -> i32 {
    args.first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0)
}

pub fn builtin_exit(args: &[String], _shell: &mut DpShell) -> i32 {
    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}

pub fn builtin_suspend(_args: &[String], _shell: &mut DpShell) -> i32 {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    match kill(Pid::from_raw(0), Signal::SIGTSTP) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("dpshell: suspend: {}", e);
            1
        }
    }
}

pub fn builtin_times(_args: &[String], _shell: &mut DpShell) -> i32 {
    let mut buf: libc::tms = unsafe { std::mem::zeroed() };
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if unsafe { libc::times(&mut buf) } == -1 {
        eprintln!("dpshell: times: cannot get process times");
        return 1;
    }
    let fmt = |t: libc::clock_t| -> (u64, u64) {
        let secs = t as f64 / ticks;
        let m = (secs / 60.0) as u64;
        let s_frac = secs - (m as f64 * 60.0);
        (m, (s_frac * 1000.0) as u64)
    };
    let (um, us) = fmt(buf.tms_utime);
    let (sm, ss) = fmt(buf.tms_stime);
    println!(
        "{}m{}.{:03}s {}m{}.{:03}s",
        um,
        us / 1000,
        us % 1000,
        sm,
        ss / 1000,
        ss % 1000
    );
    let (cum, cus) = fmt(buf.tms_cutime);
    let (csm, css) = fmt(buf.tms_cstime);
    println!(
        "{}m{}.{:03}s {}m{}.{:03}s",
        cum,
        cus / 1000,
        cus % 1000,
        csm,
        css / 1000,
        css % 1000
    );
    0
}

pub fn builtin_caller(args: &[String], _shell: &mut DpShell) -> i32 {
    let _depth: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    eprintln!("dpshell: caller: not in a function call");
    1
}

pub fn builtin_unimp(args: &[String], _shell: &mut DpShell) -> i32 {
    let name = args.first().map(String::as_str).unwrap_or("unknown");
    eprintln!("dpshell: {}: not yet implemented", name);
    2
}
