// plugins.rs - Plugin loading and execution
//
// Plugin directory layout:
//   /etc/deeprotection/plugins/{plugin-id}/plugin.json
//
// All plugins use a uniform invocation model via their `entrypoint` executable.
// The shell passes the current command via stdin and the DPSHELL_COMMAND env var.
// The plugin signals its decision through its exit code and stdout:
//   exit 0  → allow the command (stdout ignored)
//   exit 1  → block the command
//   exit 2  → replace the command (stdout contains the new command string)
//   other / timeout / spawn error → warn and allow (fail-open)
//
// Timeout: each plugin invocation is limited to PLUGIN_TIMEOUT_SECS seconds.

use libc;
use serde::Deserialize;
use std::fs;
use std::os::unix::process::CommandExt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const PLUGINS_DIR: &str = "/etc/deeprotection/plugins";
const PLUGIN_TIMEOUT_SECS: u64 = 5;

// ──────────────────────────────────────────────
// Data structures
// ──────────────────────────────────────────────

/// Deserialised contents of a `plugin.json` file.
/// Metadata fields (name, version, author, description) are retained for
/// compatibility with the web admin schema; the `type` field, if present in
/// the JSON, is silently ignored by serde's default unknown-field behaviour.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub enabled: bool,
    /// Absolute path (or path relative to the plugin directory) of the executable.
    pub entrypoint: String,
}

/// The outcome of running a single plugin.
#[derive(Debug)]
pub enum PluginDecision {
    /// Allow the command to proceed (possibly modified).
    Allow(String),
    /// Block the command; no further processing.
    Block,
}

// ──────────────────────────────────────────────
// Loading
// ──────────────────────────────────────────────

/// Scan `/etc/deeprotection/plugins/`, read each `plugin.json`, and return
/// all plugins whose `enabled` field is `true`.
/// Returns an empty Vec (not an error) if the directory does not exist.
/// Any unknown fields in `plugin.json` (including legacy `type`) are ignored.
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
            Err(_) => continue, // missing or unreadable plugin.json — skip silently
        };
        match serde_json::from_str::<PluginMeta>(&data) {
            Ok(meta) if meta.enabled => plugins.push(meta),
            Ok(_) => {}  // disabled plugin — skip
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

// ──────────────────────────────────────────────
// Execution helpers
// ──────────────────────────────────────────────

/// Resolve the entrypoint path.
/// If the path is relative, it is resolved against the plugin's own directory.
fn resolve_entrypoint(meta: &PluginMeta) -> PathBuf {
    let ep = Path::new(&meta.entrypoint);
    if ep.is_absolute() {
        ep.to_path_buf()
    } else {
        Path::new(PLUGINS_DIR)
            .join(&meta.id)
            .join(ep)
    }
}

/// Strip control characters that would break command boundaries or confuse
/// downstream processing (newline, carriage return, NUL) from a plugin
/// replacement command.
fn sanitize_replacement(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|&c| c != '\n' && c != '\r' && c != '\0')
        .collect()
}

/// Invoke a single plugin, passing `command` via stdin and the
/// `DPSHELL_COMMAND` environment variable (avoids shell-quoting issues).
///
/// Returns `PluginDecision::Allow(original_or_new_command)` or `PluginDecision::Block`.
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

    // Spawn child with stdin piped and stdout captured.  The plugin gets its
    // own process group so a timeout can kill the whole group (SIGKILL to
    // -pgid) — otherwise a plugin that forks children would leave them
    // running with stdout open, blocking the reader-thread join below.
    let child = Command::new(&entrypoint)
        .env("DPSHELL_COMMAND", command)
        .process_group(0)
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

    // Write command to stdin, then close it.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(command.as_bytes());
        // stdin closed when `stdin` drops
    }

    // Wait with timeout using a background thread.
    let timeout = Duration::from_secs(PLUGIN_TIMEOUT_SECS);
    let output = wait_with_timeout(child, timeout);

    match output {
        None => {
            eprintln!(
                "dpshell: plugin '{}': timed out after {}s — skipping",
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
                    let replacement = sanitize_replacement(&String::from_utf8_lossy(&stdout_bytes));
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

/// Wait for `child` up to `timeout`.  Polls via `try_wait`; kills + reaps the
/// child on timeout.  Always joins the stdout-reader thread before returning,
/// so no thread is ever leaked.
/// Returns `None` on timeout, `Some(Ok((status, stdout)))` on success,
/// `Some(Err(...))` on wait failure.
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

    let start = std::time::Instant::now();
    let poll = Duration::from_millis(50);

    let exit_result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // Kill the plugin's whole process group (SIGKILL) and reap
                    // it so the kernel does not leave a zombie.  Killing the
                    // group also closes the stdout pipe (forked children would
                    // otherwise keep it open) and lets the reader thread exit.
                    unsafe {
                        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                    }
                    let _ = child.wait();
                    // Drain & join the reader so we do not leak the thread.
                    if let Some(h) = stdout_handle {
                        let _ = h.join();
                    }
                    return None;
                }
                thread::sleep(poll);
            }
            Err(e) => {
                if let Some(h) = stdout_handle {
                    let _ = h.join();
                }
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

// ──────────────────────────────────────────────
// Pipeline entry-point (used by main.rs)
// ──────────────────────────────────────────────

/// Run all enabled plugins against `command` in order, synchronously.
///
/// Each plugin is invoked via its `entrypoint` executable; the decision is
/// determined solely by exit code and stdout (see module-level doc).
///
/// - Returns `Some(final_command)` if all plugins allowed the command
///   (with any replacements applied).
/// - Returns `None` if any plugin blocked the command.
pub fn run_plugins(plugins: &[PluginMeta], command: &str) -> Option<String> {
    let mut current = command.to_string();

    for plugin in plugins {
        match invoke_plugin(plugin, &current) {
            PluginDecision::Block => {
                println!("\x1b[31;5m[!]\x1b[0m Blocked by plugin: {}", plugin.id);
                return None;
            }
            PluginDecision::Allow(new_cmd) => {
                if new_cmd != current {
                    println!(
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

// ──────────────────────────────────────────────
// PATH helpers (used by main.rs at startup)
// ──────────────────────────────────────────────

/// Return the directory path for each loaded plugin (i.e. the directory that
/// contains its entrypoint).  main.rs prepends these to $PATH so that plugin
/// executables are reachable by name without a full path.
pub fn plugin_dirs_for_path(plugins: &[PluginMeta]) -> Vec<PathBuf> {
    plugins
        .iter()
        .map(|p| Path::new(PLUGINS_DIR).join(&p.id))
        .collect()
}