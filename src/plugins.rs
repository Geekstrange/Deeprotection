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

use serde::Deserialize;
use std::fs;
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

    // Spawn child with stdin piped and stdout captured.
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
                    let replacement = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
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

/// Spawn a watchdog thread that kills the child after `timeout`.
/// Returns `None` on timeout, `Some(Ok((status, stdout)))` on success,
/// `Some(Err(...))` on wait failure.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<Result<(std::process::ExitStatus, Vec<u8>), std::io::Error>> {
    use std::sync::{Arc, Mutex};
    use std::thread;

    // Collect stdout in a background thread.
    let stdout_handle = child.stdout.take().map(|stdout| {
        use std::io::Read;
        thread::spawn(move || {
            let mut buf = Vec::new();
            let mut reader = stdout;
            let _ = reader.read_to_end(&mut buf);
            buf
        })
    });

    // Shared result slot.
    let result: Arc<Mutex<Option<Result<std::process::ExitStatus, std::io::Error>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result);

    // Wait thread.
    let wait_thread = thread::spawn(move || {
        let r = child.wait();
        *result_clone.lock().unwrap() = Some(r);
    });

    // Poll until timeout.
    let start = std::time::Instant::now();
    let poll = Duration::from_millis(50);
    loop {
        if start.elapsed() >= timeout {
            // Timeout — best-effort kill (child may already be gone).
            return None;
        }
        {
            let guard = result.lock().unwrap();
            if guard.is_some() {
                break;
            }
        }
        thread::sleep(poll);
    }

    let _ = wait_thread.join();
    let stdout_bytes = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    let status_result = result.lock().unwrap().take().unwrap();
    Some(status_result.map(|s| (s, stdout_bytes)))
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