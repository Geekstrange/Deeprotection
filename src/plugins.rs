// plugins.rs - Plugin loading and execution
//
// SECURITY-CRITICAL CHANGES vs. original:
//
//   1. wait_with_timeout() now actually kills the child on timeout (was a
//      no-op despite the comment claiming "best-effort kill") and joins the
//      stdout-reader thread before returning.  The previous code spawned a
//      `wait_thread` that blocked on `child.wait()` and was NEVER joined when
//      the timeout fired, leaking both the thread and the child process.
//
//      Consequence of the original bug:
//        - A plugin that sleeps forever would persist after every invocation,
//          eventually exhausting PIDs / memory / file descriptors.
//        - The leaked thread held shared mutexes that compromised subsequent
//          fork() safety in executor.rs (only async-signal-safe functions may
//          run between fork and exec in a multi-threaded program).
//
//   2. The watchdog no longer needs a separate `wait_thread` at all — it
//      polls with `try_wait()`.  Removing the thread also removes the Arc/Mutex
//      around the result slot.

use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PLUGINS_DIR: &str = "/etc/deeprotection/plugins";
const PLUGIN_TIMEOUT_SECS: u64 = 5;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub enabled: bool,
    pub entrypoint: String,
}

#[derive(Debug)]
pub enum PluginDecision {
    Allow(String),
    Block,
}

pub fn load_plugins() -> Vec<PluginMeta> {
    let dir = Path::new(PLUGINS_DIR);
    if !dir.exists() {
        return Vec::new();
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("dpshell: plugins: cannot read {}: {}", PLUGINS_DIR, e);
            return Vec::new();
        }
    };

    let mut plugins = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("plugin.json");
        let data = match fs::read_to_string(&meta_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        match serde_json::from_str::<PluginMeta>(&data) {
            Ok(meta) if meta.enabled => plugins.push(meta),
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "dpshell: plugins: invalid plugin.json at {}: {}",
                    meta_path.display(),
                    e
                );
            }
        }
    }
    plugins
}

fn resolve_entrypoint(meta: &PluginMeta) -> PathBuf {
    let ep = Path::new(&meta.entrypoint);
    if ep.is_absolute() {
        ep.to_path_buf()
    } else {
        Path::new(PLUGINS_DIR).join(&meta.id).join(ep)
    }
}

pub fn invoke_plugin(meta: &PluginMeta, command: &str) -> PluginDecision {
    let entrypoint = resolve_entrypoint(meta);

    if !entrypoint.exists() {
        eprintln!(
            "dpshell: plugin '{}': entrypoint not found: {}",
            meta.id,
            entrypoint.display()
        );
        return PluginDecision::Allow(command.to_string());
    }

    let child = Command::new(&entrypoint)
        .env("DPSHELL_COMMAND", command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dpshell: plugin '{}': failed to spawn: {}", meta.id, e);
            return PluginDecision::Allow(command.to_string());
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(command.as_bytes());
        // stdin closed when `stdin` drops — plugin sees EOF.
    }

    let timeout = Duration::from_secs(PLUGIN_TIMEOUT_SECS);
    let output = wait_with_timeout(child, timeout);

    match output {
        None => {
            eprintln!(
                "dpshell: plugin '{}': timed out after {}s — killed and skipping",
                meta.id, PLUGIN_TIMEOUT_SECS
            );
            PluginDecision::Allow(command.to_string())
        }
        Some(Err(e)) => {
            eprintln!("dpshell: plugin '{}': wait error: {}", meta.id, e);
            PluginDecision::Allow(command.to_string())
        }
        Some(Ok((status, stdout_bytes))) => {
            let code = status.code().unwrap_or(-1);
            match code {
                0 => PluginDecision::Allow(command.to_string()),
                1 => PluginDecision::Block,
                2 => {
                    let replacement = sanitize_replacement(
                        &String::from_utf8_lossy(&stdout_bytes)
                    );
                    if replacement.is_empty() {
                        eprintln!(
                            "dpshell: plugin '{}': exit 2 but empty stdout — allowing original",
                            meta.id
                        );
                        PluginDecision::Allow(command.to_string())
                    } else {
                        PluginDecision::Allow(replacement)
                    }
                }
                other => {
                    eprintln!(
                        "dpshell: plugin '{}': unexpected exit code {} — allowing",
                        meta.id, other
                    );
                    PluginDecision::Allow(command.to_string())
                }
            }
        }
    }
}

/// A plugin's stdout replacement is treated as a single command line.  We
/// reject newlines and NUL bytes so a malicious plugin cannot inject extra
/// commands that the parser would tokenize into a sequence.  The shell-level
/// metacharacters (`;`, `&&`, `||`, `|`) are still permitted because that's
/// how legitimate plugin replacements form pipelines.
fn sanitize_replacement(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|&c| c != '\n' && c != '\r' && c != '\0')
        .collect()
}

/// Wait for `child` up to `timeout`.  Polls via try_wait(); kills + reaps the
/// child on timeout.  Always joins the stdout-reader thread before returning,
/// so no thread is ever leaked.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<Result<(std::process::ExitStatus, Vec<u8>), std::io::Error>> {
    use std::io::Read;
    use std::thread;

    // Drain stdout in a background thread so the child cannot deadlock filling
    // its stdout pipe buffer.  The thread exits when stdout reaches EOF, which
    // happens either on the child's exit or when we kill it.
    let stdout_handle = child.stdout.take().map(|mut stdout| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        })
    });

    let start = Instant::now();
    let poll = Duration::from_millis(50);

    let exit_result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // Best-effort kill (SIGKILL on Unix), then reap.
                    let _ = child.kill();
                    // Reap so the kernel does not leave a zombie.  This also
                    // closes the stdout pipe and lets the reader thread exit.
                    let _ = child.wait();
                    // Drain & join the reader so we do not leak the thread.
                    if let Some(h) = stdout_handle { let _ = h.join(); }
                    return None;
                }
                thread::sleep(poll);
            }
            Err(e) => {
                if let Some(h) = stdout_handle { let _ = h.join(); }
                return Some(Err(e));
            }
        }
    };

    // Normal exit path: status known, drain the reader thread.
    let stdout_bytes = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    Some(exit_result.map(|s| (s, stdout_bytes)))
}

pub fn run_plugins(plugins: &[PluginMeta], command: &str) -> Option<String> {
    let mut current = command.to_string();

    for plugin in plugins {
        match invoke_plugin(plugin, &current) {
            PluginDecision::Block => {
                eprintln!("\x1b[31;5m[!]\x1b[0m Blocked by plugin: {}", plugin.id);
                return None;
            }
            PluginDecision::Allow(new_cmd) => {
                if new_cmd != current {
                    eprintln!(
                        "\x1b[33;5m<!>\x1b[0m Replaced by plugin '{}': {} → {}",
                        plugin.id, current, new_cmd
                    );
                }
                current = new_cmd;
            }
        }
    }

    Some(current)
}

pub fn plugin_dirs_for_path(plugins: &[PluginMeta]) -> Vec<PathBuf> {
    plugins
        .iter()
        .map(|p| Path::new(PLUGINS_DIR).join(&p.id))
        .collect()
}
