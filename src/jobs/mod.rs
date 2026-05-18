// jobs.rs - Job Control Manager
//
// Implements the job table and the `fg`, `bg`, `jobs` built-in commands.
//
// POSIX job control overview
// ──────────────────────────
// Every interactive shell maintains a *job table*.  A "job" is a pipeline
// (one or more processes sharing a process group).  Key invariants:
//
//   • Each job has a unique PGID (process group ID).  For a simple command
//     the PGID equals the first child's PID.
//   • The terminal has exactly one *foreground* process group at a time
//     (`tcsetpgrp`).  All other jobs are in the background.
//   • Stopped jobs (SIGTSTP) stay in the table until foregrounded or killed.
//   • Background jobs (& — not yet implemented in the parser, but the table
//     supports them) run without terminal ownership.
//
// Signal safety
// ─────────────
// All `tcsetpgrp` calls in the parent must be preceded by ignoring SIGTTOU,
// because if the shell's own pgrp is not the terminal foreground at the time
// of the call, the kernel would raise SIGTTOU.  After the call we restore
// SIGTTOU to SIG_IGN (our normal disposition).
//
// JobManager is NOT Send/Sync — it is owned by the single-threaded REPL and
// never shared across threads.  The raw pointer games with job IDs are safe
// because mutation only happens from the REPL thread.

use nix::sys::signal::{kill, signal, SigHandler, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::BTreeMap;
use std::os::unix::io::RawFd;

const STDIN_FD: RawFd = 0;

// ──────────────────────────────────────────────────────────────────────────────
// Job table types
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    /// Running in the foreground (transient — removed when wait returns).
    Foreground,
    /// Running in the background (not yet implemented in the parser,
    /// but the table accepts it for future `cmd &` support).
    Background,
    /// Stopped by SIGTSTP.
    Stopped,
    /// Exited — kept briefly for status reporting, then removed.
    Done(i32),
}

#[derive(Debug, Clone)]
pub struct Job {
    /// Shell job number (1-based, matches what `jobs` prints).
    pub id: usize,
    /// Process group ID of the job.
    pub pgid: Pid,
    /// Human-readable command string (for `jobs` display).
    pub command: String,
    /// Current status.
    pub status: JobStatus,
}

// ──────────────────────────────────────────────────────────────────────────────
// JobManager
// ──────────────────────────────────────────────────────────────────────────────

pub struct JobManager {
    /// The shell's own process group ID — needed to reclaim the terminal.
    shell_pgid: Pid,
    /// Active jobs keyed by job ID.
    jobs: BTreeMap<usize, Job>,
    /// Monotonically increasing job ID counter.
    next_id: usize,
}

impl JobManager {
    /// Create a new job manager.  `shell_pgid` must be the shell's own PGID.
    pub fn new(shell_pgid: Pid) -> Self {
        Self {
            shell_pgid,
            jobs: BTreeMap::new(),
            next_id: 1,
        }
    }

    // ── Registration ──────────────────────────────────────────────────────

    /// Register a newly forked job and return its job ID.
    pub fn add(&mut self, pgid: Pid, command: impl Into<String>, status: JobStatus) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.insert(
            id,
            Job {
                id,
                pgid,
                command: command.into(),
                status,
            },
        );
        id
    }

    /// Update the status of a job by PGID.  Returns `false` if not found.
    pub fn update_by_pgid(&mut self, pgid: Pid, status: JobStatus) -> bool {
        for job in self.jobs.values_mut() {
            if job.pgid == pgid {
                job.status = status;
                return true;
            }
        }
        false
    }

    /// Remove completed jobs from the table (called before printing the prompt).
    pub fn reap_done(&mut self) {
        self.jobs
            .retain(|_, j| !matches!(j.status, JobStatus::Done(_)));
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    /// Look up a job by its shell job number.
    /// Reserved for `fg N` / `bg N` by explicit job id.
    #[allow(dead_code)]
    pub fn get_by_id(&self, id: usize) -> Option<&Job> {
        self.jobs.get(&id)
    }

    /// Return the most-recently stopped or backgrounded job (used by `fg`/`bg`
    /// with no argument).
    pub fn last_job(&self) -> Option<&Job> {
        self.jobs
            .values()
            .rev()
            .find(|j| matches!(j.status, JobStatus::Stopped | JobStatus::Background))
    }

    // ── Built-in: jobs ────────────────────────────────────────────────────

    /// Print the job table to stdout, matching the classic `[N] Status  cmd` format.
    pub fn builtin_jobs(&mut self) {
        // Reap any asynchronously completed background jobs first.
        self.poll_background();

        if self.jobs.is_empty() {
            return;
        }
        for job in self.jobs.values() {
            let status_str = match &job.status {
                JobStatus::Foreground => "Running (fg)".to_string(),
                JobStatus::Background => "Running".to_string(),
                JobStatus::Stopped => "Stopped".to_string(),
                JobStatus::Done(code) => format!("Done({})", code),
            };
            println!("[{}] {:<14} {}", job.id, status_str, job.command);
        }
        self.reap_done();
    }

    // ── Built-in: fg ──────────────────────────────────────────────────────

    /// Bring job `id` (or the last job if `id` is None) to the foreground.
    ///
    /// Steps:
    ///   1. Find the job and get its PGID.
    ///   2. Give the terminal to the job's PGID (`tcsetpgrp`).
    ///   3. Send SIGCONT to the job's PGID (wakes it if stopped).
    ///   4. Wait for the job with WUNTRACED (same as initial execution).
    ///   5. Reclaim the terminal for the shell.
    ///
    /// # Safety of tcsetpgrp
    /// We are in the shell (background relative to the new fg pgrp), so the
    /// kernel would raise SIGTTOU when we call tcsetpgrp — unless we ignore it
    /// first.  We ignore it, make the call, then restore SIG_IGN (the shell's
    /// normal disposition for SIGTTOU).
    pub fn builtin_fg(&mut self, id: Option<usize>) {
        let job = match self.resolve_job(id) {
            Some(j) => j.clone(),
            None => {
                eprintln!("dpshell: fg: no such job");
                return;
            }
        };

        println!("{}", job.command);

        // ① Transfer terminal ownership to the job.
        unsafe {
            let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
            libc::tcsetpgrp(STDIN_FD, job.pgid.as_raw());
        }

        // ② Wake the process group (no-op if already running).
        let _ = kill(Pid::from_raw(-job.pgid.as_raw()), Signal::SIGCONT);

        // Update status to Foreground before waiting.
        self.update_by_pgid(job.pgid, JobStatus::Foreground);

        // ③ Wait loop — same logic as the initial foreground wait.
        self.wait_foreground(job.pgid, &job.command);
    }

    // ── Built-in: bg ──────────────────────────────────────────────────────

    /// Resume job `id` (or the last stopped job) in the background.
    ///
    /// Unlike `fg`, we do NOT transfer terminal ownership — the job runs
    /// without stdin access.  If it tries to read stdin, it will receive
    /// SIGTTIN and stop again.
    ///
    /// We only send SIGCONT to the process group.
    pub fn builtin_bg(&mut self, id: Option<usize>) {
        let job = match self.resolve_job(id) {
            Some(j) => j.clone(),
            None => {
                eprintln!("dpshell: bg: no such job");
                return;
            }
        };

        if !matches!(job.status, JobStatus::Stopped) {
            eprintln!("dpshell: bg: job {} is not stopped", job.id);
            return;
        }

        println!("[{}] {} &", job.id, job.command);

        // Resume without terminal transfer.
        let _ = kill(Pid::from_raw(-job.pgid.as_raw()), Signal::SIGCONT);
        self.update_by_pgid(job.pgid, JobStatus::Background);
    }

    // ── Foreground wait loop ───────────────────────────────────────────────

    /// Wait for the process group `pgid` to stop or exit, then reclaim the
    /// terminal.  Equivalent to `executor::parent_wait` but integrated with
    /// the job table.
    pub fn wait_foreground(&mut self, pgid: Pid, cmd_display: &str) {
        loop {
            // Wait for any process in the group.
            match waitpid(
                Pid::from_raw(-pgid.as_raw()),
                Some(WaitPidFlag::WUNTRACED | WaitPidFlag::WNOHANG),
            ) {
                Ok(WaitStatus::StillAlive) => {
                    // No change yet — yield and retry.
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                Ok(WaitStatus::Stopped(_, sig)) => {
                    self.reclaim_terminal();
                    eprintln!("\n[?] stopped: {} (signal {})", cmd_display, sig as i32);
                    self.update_by_pgid(pgid, JobStatus::Stopped);
                    break;
                }
                Ok(WaitStatus::Exited(_, code)) => {
                    self.reclaim_terminal();
                    self.update_by_pgid(pgid, JobStatus::Done(code));
                    self.reap_done();
                    break;
                }
                Ok(WaitStatus::Signaled(_, sig, _)) => {
                    self.reclaim_terminal();
                    if sig != Signal::SIGINT {
                        eprintln!("\n[killed: {}]", sig);
                    }
                    self.update_by_pgid(pgid, JobStatus::Done(-1));
                    self.reap_done();
                    break;
                }
                Err(nix::Error::EINTR) => continue,
                Err(nix::Error::ECHILD) => {
                    // No more children in the group — already reaped elsewhere.
                    self.reclaim_terminal();
                    self.update_by_pgid(pgid, JobStatus::Done(0));
                    self.reap_done();
                    break;
                }
                Err(e) => {
                    eprintln!("dpshell: waitpid error: {}", e);
                    self.reclaim_terminal();
                    break;
                }
                Ok(_) => continue,
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    fn reclaim_terminal(&self) {
        unsafe {
            let _ = signal(Signal::SIGTTOU, SigHandler::SigIgn);
            libc::tcsetpgrp(STDIN_FD, self.shell_pgid.as_raw());
        }
    }

    fn resolve_job(&self, id: Option<usize>) -> Option<&Job> {
        match id {
            Some(n) => self.jobs.get(&n),
            None => self.last_job(),
        }
    }

    /// Non-blocking poll of background jobs to collect exit statuses before
    /// printing the prompt.  This is how bash prints "[1] Done  cmd".
    pub fn poll_background(&mut self) {
        let pgids: Vec<Pid> = self
            .jobs
            .values()
            .filter(|j| matches!(j.status, JobStatus::Background))
            .map(|j| j.pgid)
            .collect();

        for pgid in pgids {
            #[allow(clippy::never_loop)]
            loop {
                match waitpid(
                    Pid::from_raw(-pgid.as_raw()),
                    Some(WaitPidFlag::WNOHANG | WaitPidFlag::WUNTRACED),
                ) {
                    Ok(WaitStatus::StillAlive) => break,
                    Ok(WaitStatus::Exited(_, code)) => {
                        self.update_by_pgid(pgid, JobStatus::Done(code));
                        break;
                    }
                    Ok(WaitStatus::Signaled(_, _, _)) => {
                        self.update_by_pgid(pgid, JobStatus::Done(-1));
                        break;
                    }
                    Ok(WaitStatus::Stopped(_, _)) => {
                        self.update_by_pgid(pgid, JobStatus::Stopped);
                        break;
                    }
                    Err(_) | Ok(_) => break,
                }
            }
        }
    }
}
