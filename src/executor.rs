// executor.rs - ARCHITECTURE.md §3.1 Command Execution
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Execute `cmd` via `sh -c`, with proper job-control handling.
///
/// ## Why this is non-trivial
///
/// dpshell ignores SIGTSTP (so Ctrl+Z cannot suspend the shell itself when
/// launched from bash/zsh).  Naive `Command::status()` leaves the child in
/// dpshell's process group, which creates two problems:
///
/// 1. `Command::status()` calls `waitpid` *without* `WUNTRACED`, so if the
///    child stops it never returns — dpshell hangs with a frozen terminal.
/// 2. Even with WUNTRACED, after the child stops the terminal's foreground
///    process group is still dpshell's pgid, so no prompt reappears.
///
/// ## Solution — minimal job control
///
/// For each command:
///   • Give the child its own process group (`setpgid(0,0)` in `pre_exec`).
///   • Hand the terminal to the child's pgid (`tcsetpgrp`) so it becomes the
///     real terminal foreground and Ctrl+Z reaches it normally.
///   • Wait with `WUNTRACED` so we detect stops.
///   • On stop: reclaim the terminal for dpshell's pgid and return to prompt.
///   • Restore default signal dispositions in the child before exec.
pub fn execute_command(cmd: &str) {
    // ── Spawn ────────────────────────────────────────────────────────────────
    let child = unsafe {
        Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .pre_exec(|| {
                // ① New process group: child pgid = child pid.
                libc::setpgid(0, 0);

                // ② Make child's pgrp the terminal foreground.
                //    Ignore SIGTTOU first so tcsetpgrp doesn't raise it while
                //    we are still a background process (race window).
                libc::signal(libc::SIGTTOU, libc::SIG_IGN);
                libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpid());

                // ③ Restore default dispositions so the child behaves normally.
                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                libc::signal(libc::SIGTTOU, libc::SIG_DFL);
                libc::signal(libc::SIGTTIN, libc::SIG_DFL);
                libc::signal(libc::SIGINT,  libc::SIG_DFL);
                Ok(())
            })
            .spawn()
    };

    let child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dpshell: failed to execute '{}': {}", cmd, e);
            return;
        }
    };

    let child_pid = child.id() as libc::pid_t;
    let child_pgid = child_pid; // setpgid(0,0) makes pgid == pid

    // ── Race-safe: also set pgid from parent side ────────────────────────────
    // Both parent and child call setpgid; whichever runs first wins; the
    // second call is a harmless no-op.  This closes the race window where
    // a signal could arrive before the child calls setpgid.
    unsafe { libc::setpgid(child_pid, child_pgid); }

    // ── Give terminal to child (parent side, for the same race window) ───────
    // Ignore SIGTTOU so this tcsetpgrp doesn't raise it for dpshell.
    let dpshell_pgid = unsafe { libc::getpgrp() };
    unsafe {
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
        libc::tcsetpgrp(libc::STDIN_FILENO, child_pgid);
    }

    // ── Wait with WUNTRACED so stops are visible ─────────────────────────────
    loop {
        let mut status: libc::c_int = 0;
        let ret = unsafe { libc::waitpid(child_pid, &mut status, libc::WUNTRACED) };

        if ret < 0 {
            // EINTR: a signal interrupted the wait — retry.
            let errno = unsafe { *libc::__errno_location() };
            if errno == libc::EINTR {
                continue;
            }
            eprintln!("dpshell: waitpid error: {}", errno);
            break;
        }

        if libc::WIFSTOPPED(status) {
            // Child was suspended by Ctrl+Z (or another stop signal).
            // Reclaim the terminal for dpshell and return to the prompt.
            unsafe { libc::tcsetpgrp(libc::STDIN_FILENO, dpshell_pgid); }
            let sig = libc::WSTOPSIG(status);
            eprintln!("\n[{}] stopped (signal {})", cmd, sig);
            break;
        }

        if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
            // Child exited or was killed — reclaim terminal and we're done.
            unsafe { libc::tcsetpgrp(libc::STDIN_FILENO, dpshell_pgid); }
            break;
        }
    }

    // Restore SIGTTOU to ignored (dpshell's own disposition for tcsetpgrp calls).
    unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN); }
}