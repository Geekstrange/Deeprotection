// main.rs - ARCHITECTURE.md §2 High-Level Architecture; Refactored_Plan.md §1
// Integrates: config, logger, plugins, rules, protection, executor, cd, utils.

mod cd;
mod config;
mod executor;
mod logger;
mod plugins;
mod protection;
mod rules;
mod utils;

use anyhow::Result;
use libc;
use cd::execute_cd;
use executor::execute_command;
use logger::{JsonLinesWriter, LogEntry};
use plugins::{load_plugins, plugin_dirs_for_path, run_plugins, PluginMeta};
use protection::{check_protected_operation, ProtectionResult};
use rules::{apply_rules, compile_rule, CompiledRule};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config as RlConfig, Editor, Helper};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ──────────────────────────────────────────────
// Rustyline tab-completion helper
// ──────────────────────────────────────────────

struct DpCompleter {
    filename: FilenameCompleter,
    commands: Vec<String>,
}

impl DpCompleter {
    fn new() -> Self {
        Self {
            filename: FilenameCompleter::new(),
            commands: vec![
                "cd".into(),
                "exit".into(),
                "ls".into(),
                "ll".into(),
                "la".into(),
                "rm".into(),
                "history".into(),
            ],
        }
    }
}

impl Completer for DpCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> std::result::Result<(usize, Vec<Pair>), ReadlineError> {
        // Command-name completion for the first token
        if !line.contains(' ') {
            let prefix = &line[..pos];
            let candidates: Vec<Pair> = self
                .commands
                .iter()
                .filter(|c| c.starts_with(prefix))
                .map(|c| Pair {
                    display: c.clone(),
                    replacement: c.clone(),
                })
                .collect();
            if !candidates.is_empty() {
                return Ok((0, candidates));
            }
        }
        // Path completion for everything else
        self.filename.complete(line, pos, ctx)
    }
}

impl Highlighter for DpCompleter {}
impl Hinter for DpCompleter {
    type Hint = String;
}
impl Validator for DpCompleter {}
impl Helper for DpCompleter {}

// ──────────────────────────────────────────────
// Password authentication
// ──────────────────────────────────────────────

/// Prompt for the admin password (up to 3 attempts).
/// Returns `true` if the correct password is entered, `false` after 3 failures.
/// Uses SHA-256 to verify against the stored hash from config.
fn authenticate(expected_hash: &str) -> bool {
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 0..MAX_ATTEMPTS {
        let remaining = MAX_ATTEMPTS - attempt;
        let password = match rpassword::prompt_password("Admin password: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("dpshell: failed to read password: {}", e);
                return false;
            }
        };
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        if hash == expected_hash {
            return true;
        }
        let still_left = remaining - 1;
        if still_left > 0 {
            println!("Authentication failed. {} attempt(s) remaining.", still_left);
        } else {
            println!("Authentication failed.");
        }
    }
    false
}

// ──────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────

fn main() -> Result<()> {
    // ── 1. Load configuration ──────────────────────────────────────────────
    // ARCHITECTURE.md §4: config file at /etc/deeprotection/config.toml
    let config = config::load_config().unwrap_or_else(|e| {
        eprintln!(
            "dpshell: warning: could not load config \
             (/etc/deeprotection/config.toml): {}. Using defaults.",
            e
        );
        config::Config {
            core: config::CoreConfig {
                mode: "permissive".to_string(),
            },
            auth: config::AuthConfig::default(),
            paths: config::PathsConfig::default(),
            rules: vec![],
        }
    });

    let mode = config.core.mode.clone();
    let protect_paths = config.paths.protect.clone();
    let allowlist = config.paths.allowlist.clone();
    let password_hash = config.auth.password_hash.clone();

    // Compile rules once at startup (ARCHITECTURE.md §3.2)
    let compiled_rules: Vec<CompiledRule> = config.rules.iter().filter_map(compile_rule).collect();

    // ── 2. Initialise logger ───────────────────────────────────────────────
    // Log file: /var/log/audit.log
    std::fs::create_dir_all("/var/log").map_err(|e| anyhow::anyhow!("Failed to create /var/log: {}", e))?;
    let logger = JsonLinesWriter::new("/var/log/audit.log")
        .map_err(|e| anyhow::anyhow!("Failed to open /var/log/audit.log (try running as root): {}", e))?;

    // ── 3. Load plugins ────────────────────────────────────────────────────
    // Scans /etc/deeprotection/plugins/; returns empty Vec if absent.
    let loaded_plugins: Vec<PluginMeta> = load_plugins();
    if !loaded_plugins.is_empty() {
        eprintln!(
            "dpshell: {} plugin(s) loaded: {}",
            loaded_plugins.len(),
            loaded_plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")
        );

        // Prepend each plugin's directory to $PATH so its executables are
        // reachable by name (e.g. `enls`) without needing a full path.
        let plugin_dirs = plugin_dirs_for_path(&loaded_plugins);
        let existing_path = env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = plugin_dirs
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect();
        parts.push(existing_path);
        let new_path = parts.join(":");
        unsafe { env::set_var("PATH", &new_path); }
    }

    // ── 4. Nesting level (ARCHITECTURE.md §3.7) ────────────────────────────
    let dpshell_level = env::var("DPSHELL_LEVEL")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0)
        + 1;

    // Safety: setting env vars is inherently unsafe in multi-threaded contexts,
    // but dpshell is single-threaded for the purpose of this env var.
    unsafe {
        env::set_var("DPSHELL_LEVEL", dpshell_level.to_string());
    }

    // ── 5. Command history in /tmp (ARCHITECTURE.md §3.7) ─────────────────
    let rand_suffix: u32 = {
        // Simple PRNG seed from process ID + time-ish
        let pid = std::process::id();
        pid ^ 0xDEAD_BEEF
    };
    let hist_path = PathBuf::from(format!("/tmp/dpshell_history.{:06X}", rand_suffix));
    let _ = File::create(&hist_path);

    // ── 6. Startup animation ───────────────────────────────────────────────
    // ARCHITECTURE.md §3.7; Refactored_Plan.md §5
    utils::start_animation(&mode);

    // Print entry hint
//    println!("(Enter exit or Ctrl+D to quit)");

    // ── 7. Signal handling ─────────────────────────────────────────────────
    // Ctrl-C (SIGINT): set a flag; the main loop prints a newline and continues.
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_clone = Arc::clone(&interrupted);
    ctrlc::set_handler(move || {
        interrupted_clone.store(true, Ordering::SeqCst);
    })?;

    // Ctrl-Z (SIGTSTP): ignore.
    // When dpshell is launched from another shell (bash, zsh, etc.) pressing
    // Ctrl+Z would otherwise send SIGTSTP to the foreground process group,
    // suspending dpshell and dropping the user back to the parent shell —
    // bypassing all protection.  SIG_IGN is inherited across execve, so child
    // commands spawned via `sh -c` will also ignore it; that matches standard
    // interactive-shell behaviour (bash/zsh ignore SIGTSTP for themselves too).
    unsafe {
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
    }

    // ── 8. Rustyline editor ────────────────────────────────────────────────
    let rl_config = RlConfig::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .build();
    let mut rl: Editor<DpCompleter, _> = Editor::with_config(rl_config)?;
    rl.set_helper(Some(DpCompleter::new()));
    let _ = rl.load_history(&hist_path);

    let prompt = utils::get_prompt(dpshell_level);

    // ── 9. Helper: try to exit, requiring auth in enforcing mode ───────────
    let try_exit = |logger: &JsonLinesWriter, user: &str, cwd_str: &str, pid: u32| -> bool {
        if mode == "enforcing" {
            println!("Authentication required to exit enforcing mode.");
            if !authenticate(&password_hash) {
                println!("Authentication failed. Staying in shell.");
                // Log the failed exit attempt
                let entry = LogEntry::new(
                    "WARN", user, &mode, "exit", cwd_str, pid,
                    "exit blocked: authentication failed",
                );
                let _ = logger.write_entry(&entry);
                let _ = logger.flush();
                return false;
            }
            // Log successful authenticated exit
            let entry = LogEntry::new(
                "INFO", user, &mode, "exit", cwd_str, pid,
                "exit authorized",
            );
            let _ = logger.write_entry(&entry);
            let _ = logger.flush();
        }
        true
    };

    // ── 10. Main read-eval loop ────────────────────────────────────────────
    loop {
        // Handle Ctrl-C: print newline and continue
        if interrupted.load(Ordering::SeqCst) {
            interrupted.store(false, Ordering::SeqCst);
            println!();
            continue;
        }

        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                println!();
                continue;
            }
            // Ctrl+D (EOF): require auth in enforcing mode before exiting
            Err(ReadlineError::Eof) => {
                println!(); // newline after ^D
                let user = utils::get_current_user();
                let cwd_str = utils::get_current_working_dir().to_string_lossy().to_string();
                let pid = std::process::id();
                if try_exit(&logger, &user, &cwd_str, pid) {
                    break;
                }
                continue;
            }
            Err(e) => {
                eprintln!("dpshell: readline error: {}", e);
                break;
            }
        };

        let cmd = line.trim();

        // Handle Ctrl-L clear screen
        if cmd == "\x0C" {
            print!("\x1b[2J\x1b[H");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            continue;
        }

        if cmd.is_empty() {
            continue;
        }

        // Save to history
        let _ = rl.add_history_entry(cmd);
        let _ = rl.save_history(&hist_path);

        // Context for logging (re-fetched each command for accuracy)
        let user = utils::get_current_user();
        let cwd = utils::get_current_working_dir();
        let pid = std::process::id();
        let cwd_str = cwd.to_string_lossy().to_string();

        // Exit command: require auth in enforcing mode
        if cmd == "exit" {
            if try_exit(&logger, &user, &cwd_str, pid) {
                break;
            }
            continue;
        }

        // Built-in cd (must be handled in-process for directory changes to persist)
        let args: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
        if args[0] == "cd" {
            let _ = execute_cd(&args[1..]);
            continue;
        }

        // ── 11. Mode dispatching (ARCHITECTURE.md §3.1) ────────────────────
        match mode.as_str() {
            // disable: execute unconditionally, log only — no rules, no plugins, no path protection
            "disable" => {
                execute_command(cmd);
                let entry = LogEntry::new(
                    "INFO",
                    &user,
                    &mode,
                    cmd,
                    &cwd_str,
                    pid,
                    "command executed (disable mode)",
                );
                if let Err(e) = logger.write_entry(&entry) {
                    eprintln!("dpshell: log write failed: {}", e);
                }
                if let Err(e) = logger.flush() {
                    eprintln!("dpshell: log flush failed: {}", e);
                }
            }

            // permissive: rules first, then plugins; no path protection
            "permissive" => {
                match apply_rules(cmd, &compiled_rules) {
                    None => {
                        // Blocked by rule — plugins skipped
                        let entry = LogEntry::new("WARN", &user, &mode, cmd, &cwd_str, pid, "blocked by rule");
                        if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                    }
                    Some(after_rules_cmd) => {
                        match run_plugins(&loaded_plugins, &after_rules_cmd) {
                            None => {
                                let entry = LogEntry::new("WARN", &user, &mode, cmd, &cwd_str, pid,
                                    &format!("blocked by plugin (after rules: {})", after_rules_cmd));
                                if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                            }
                            Some(to_execute) => {
                                let msg = if to_execute != cmd { format!("replaced to: {}", to_execute) } else { "no replacement".to_string() };
                                execute_command(&to_execute);
                                let entry = LogEntry::new("INFO", &user, &mode, cmd, &cwd_str, pid, &msg);
                                if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                            }
                        }
                    }
                }
                if let Err(e) = logger.flush() { eprintln!("dpshell: log flush failed: {}", e); }
            }

            // enforcing: rules → plugins → path protection
            "enforcing" => {
                match apply_rules(cmd, &compiled_rules) {
                    None => {
                        let entry = LogEntry::new("WARN", &user, &mode, cmd, &cwd_str, pid, "blocked by rule");
                        if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                    }
                    Some(after_rules_cmd) => {
                        match run_plugins(&loaded_plugins, &after_rules_cmd) {
                            None => {
                                let entry = LogEntry::new("WARN", &user, &mode, cmd, &cwd_str, pid,
                                    &format!("blocked by plugin (after rules: {})", after_rules_cmd));
                                if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                            }
                            Some(to_execute) => {
                                match check_protected_operation(&to_execute, &protect_paths, &allowlist) {
                                    ProtectionResult::Allowed => {
                                        // No protected path involved — execute normally
                                        let msg = if to_execute != cmd { format!("replaced to: {}", to_execute) } else { "no replacement".to_string() };
                                        execute_command(&to_execute);
                                        let entry = LogEntry::new("INFO", &user, &mode, cmd, &cwd_str, pid, &msg);
                                        if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                                    }
                                    ProtectionResult::Blocked => {
                                        // Command not in allowlist — reject outright
                                        println!("\x1b[31;5m[!]\x1b[0m Operation on protected path blocked (command not allowlisted).");
                                        let entry = LogEntry::new("WARN", &user, &mode, cmd, &cwd_str, pid,
                                            &format!("blocked: command not in allowlist (final: {})", to_execute));
                                        if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                                    }
                                    ProtectionResult::RequiresAuth => {
                                        // Command is allowlisted but targets a protected path — require password
                                        println!("\x1b[31;5m[!]\x1b[0m Protected path operation requires authorization.");
                                        if authenticate(&password_hash) {
                                            println!("Authorization granted. Executing...");
                                            let msg = format!("auth granted, executed on protected path (final: {})", to_execute);
                                            execute_command(&to_execute);
                                            let entry = LogEntry::new("INFO", &user, &mode, cmd, &cwd_str, pid, &msg);
                                            if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                                        } else {
                                            println!("Authorization denied. Operation cancelled.");
                                            let entry = LogEntry::new("WARN", &user, &mode, cmd, &cwd_str, pid,
                                                &format!("blocked: auth failed for protected path op (final: {})", to_execute));
                                            if let Err(e) = logger.write_entry(&entry) { eprintln!("dpshell: log write failed: {}", e); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Err(e) = logger.flush() { eprintln!("dpshell: log flush failed: {}", e); }
            }

            // Unknown mode: warn and fall back to permissive behaviour (no plugins)
            other => {
                eprintln!("dpshell: unknown mode '{}', treating as permissive", other);
                if let Some(to_execute) = apply_rules(cmd, &compiled_rules) {
                    execute_command(&to_execute);
                }
            }
        }
        println!(); // Blank line between commands for readability
    }

    // ── 12. Exit: print goodbye, clean up history ──────────────────────────
    // println!(
    //     "\x1b[32mExited\x1b[0m {}",
    //     chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    // );

    let _ = std::fs::remove_file(&hist_path);

    // Decrement nesting level
    let new_level = dpshell_level.saturating_sub(1);
    unsafe {
        env::set_var("DPSHELL_LEVEL", new_level.to_string());
    }

    let _ = logger.flush();
    Ok(())
}