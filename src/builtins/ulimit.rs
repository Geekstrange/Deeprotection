use crate::shell::DpShell;

pub fn builtin_ulimit(args: &[String], _shell: &mut DpShell) -> i32 {
    let mut resource = libc::RLIMIT_FSIZE;
    let mut hard = false;
    let mut set_val: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" => {
                print_all_limits(hard);
                return 0;
            }
            "-H" => hard = true,
            "-S" => hard = false,
            "-c" => resource = libc::RLIMIT_CORE,
            "-d" => resource = libc::RLIMIT_DATA,
            "-f" => resource = libc::RLIMIT_FSIZE,
            "-l" => resource = libc::RLIMIT_MEMLOCK,
            "-m" => resource = libc::RLIMIT_RSS,
            "-n" => resource = libc::RLIMIT_NOFILE,
            "-s" => resource = libc::RLIMIT_STACK,
            "-t" => resource = libc::RLIMIT_CPU,
            "-u" => resource = libc::RLIMIT_NPROC,
            "-v" => resource = libc::RLIMIT_AS,
            other => {
                set_val = Some(other);
            }
        }
        i += 1;
    }

    if let Some(val) = set_val {
        let new_limit = if val == "unlimited" {
            libc::RLIM_INFINITY
        } else {
            match val.parse::<u64>() {
                Ok(n) => n,
                Err(_) => {
                    eprintln!("dpshell: ulimit: {}: invalid limit", val);
                    return 1;
                }
            }
        };
        let mut rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        unsafe { libc::getrlimit(resource as _, &mut rlim) };
        if hard {
            rlim.rlim_max = new_limit;
        } else {
            rlim.rlim_cur = new_limit;
        }
        if unsafe { libc::setrlimit(resource as _, &rlim) } != 0 {
            eprintln!("dpshell: ulimit: cannot modify limit: Operation not permitted");
            return 1;
        }
        return 0;
    }

    let mut rlim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    unsafe { libc::getrlimit(resource as _, &mut rlim) };
    let val = if hard { rlim.rlim_max } else { rlim.rlim_cur };
    if val == libc::RLIM_INFINITY {
        println!("unlimited");
    } else {
        println!("{}", val);
    }
    0
}

fn print_all_limits(hard: bool) {
    let resources: &[(&str, libc::__rlimit_resource_t)] = &[
        ("core file size          (blocks, -c) ", libc::RLIMIT_CORE),
        ("data seg size           (kbytes, -d) ", libc::RLIMIT_DATA),
        ("file size               (blocks, -f) ", libc::RLIMIT_FSIZE),
        (
            "max locked memory       (kbytes, -l) ",
            libc::RLIMIT_MEMLOCK,
        ),
        ("max memory size         (kbytes, -m) ", libc::RLIMIT_RSS),
        ("open files                      (-n) ", libc::RLIMIT_NOFILE),
        ("stack size              (kbytes, -s) ", libc::RLIMIT_STACK),
        ("cpu time               (seconds, -t) ", libc::RLIMIT_CPU),
        ("max user processes              (-u) ", libc::RLIMIT_NPROC),
        ("virtual memory          (kbytes, -v) ", libc::RLIMIT_AS),
    ];
    for (label, res) in resources {
        let mut rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        unsafe { libc::getrlimit(*res as _, &mut rlim) };
        let val = if hard { rlim.rlim_max } else { rlim.rlim_cur };
        if val == libc::RLIM_INFINITY {
            println!("{}unlimited", label);
        } else {
            println!("{}{}", label, val);
        }
    }
}
