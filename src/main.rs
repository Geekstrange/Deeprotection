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

/// Stored hash format: `<salt_hex>$<iterations>$<digest_hex>`.
/// Salt is 16 random bytes (32 hex chars); the digest is SHA-256 iterated
/// `iterations` times over (salt || password), hex-encoded.  Iterating makes
/// brute-force/dictionary attacks against a stolen config file far costlier.
const HASH_ITERATIONS: u64 = 100_000;
const SALT_BYTES: usize = 16;
const HASH_ITER_CAP: u64 = 10_000_000;

/// Compare two hex strings in constant time (no early exit on first mismatch).
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Iterated SHA-256: h0 = SHA-256(salt_hex || password), hi = SHA-256(hi-1).
fn iterated_hash(password: &[u8], salt_hex: &str, iterations: u64) -> String {
    let mut h = Sha256::new();
    h.update(salt_hex.as_bytes());
    h.update(password);
    let mut digest = h.finalize();
    for _ in 1..iterations {
        let mut hasher = Sha256::new();
        hasher.update(digest);
        digest = hasher.finalize();
    }
    format!("{:x}", digest)
}

/// Verify `password` against the stored hash string from config.
///
/// - Empty string → always false (auth disabled; enforcing mode becomes
///   un-exitable by design — set a hash before enabling enforcing).
/// - `salt$iterations$digest` → salted, iterated verification.
/// - Legacy bare SHA-256 (no `$`) → still verified, but a warning tells the
///   admin to regenerate with `dpshell --hash-password`.
/// - Anything else → false with a warning.
fn verify_password(password: &str, stored: &str) -> bool {
    if stored.is_empty() {
        return false;
    }
    if let Some((head, digest)) = stored.rsplit_once('$') {
        if let Some((salt, iters)) = head.split_once('$') {
            if salt.len() == SALT_BYTES * 2 && digest.len() == 64 {
                if let Ok(n) = iters.parse::<u64>() {
                    let n = n.clamp(1, HASH_ITER_CAP);
                    let expected = iterated_hash(password.as_bytes(), salt, n);
                    return constant_time_eq(&expected, digest);
                }
            }
        }
        eprintln!(
            "dpshell: warning: malformed password_hash in config \
             (expected salt$iterations$digest); authentication disabled"
        );
        return false;
    }
    // Legacy unsalted SHA-256 — keep working, but tell the admin to regenerate.
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    if constant_time_eq(&hash, stored) {
        eprintln!(
            "dpshell: warning: password_hash uses legacy unsalted SHA-256; \
             regenerate with `dpshell --hash-password`"
        );
        true
    } else {
        false
    }
}

/// Generate a new `salt$iterations$digest` hash and print it to stdout.
/// Used via the hidden `--hash-password` flag.
fn generate_password_hash() -> i32 {
    use std::io::Read;
    let mut salt_bytes = [0u8; SALT_BYTES];
    let rand_ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut salt_bytes))
        .is_ok();
    if !rand_ok {
        eprintln!("dpshell: cannot read /dev/urandom for salt generation");
        return 1;
    }
    let salt_hex: String = salt_bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let password = match rpassword::prompt_password("Password: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dpshell: failed to read password: {}", e);
            return 1;
        }
    };
    let digest = iterated_hash(password.as_bytes(), &salt_hex, HASH_ITERATIONS);
    println!("{}${}${}", salt_hex, HASH_ITERATIONS, digest);
    0
}

/// Prompt for the admin password (up to 3 attempts).
/// Returns `true` if the correct password is entered, `false` after 3 failures.
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
        if verify_password(&password, expected_hash) {
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
    // Hidden flag: generate a password hash for [auth] password_hash and exit.
    // Runs before any config/log/plugin initialisation so it needs no
    // privileges beyond reading the terminal.
    if std::env::args().any(|a| a == "--hash-password") {
        std::process::exit(generate_password_hash());
    }

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

    // Fail fast on an unknown mode instead of silently misbehaving per command.
    if !["disable", "permissive", "enforcing"].contains(&mode.as_str()) {
        eprintln!(
            "dpshell: invalid [core] mode '{}' in config; must be one of: disable, permissive, enforcing",
            mode
        );
        std::process::exit(2);
    }

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
    // Hardened: unpredictable 64-bit random suffix, O_NOFOLLOW (a pre-planted
    // symlink — the classic /tmp attack — fails closed), 0600 permissions.
    // On any failure, history stays in-memory only; the shell must not crash
    // or truncate arbitrary files because /tmp is hostile.
    let hist_path: Option<PathBuf> = {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;

        let mut rand_bytes = [0u8; 8];
        let rand_ok = std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut rand_bytes))
            .is_ok();
        let suffix = if rand_ok {
            u64::from_le_bytes(rand_bytes)
        } else {
            // Last-resort fallback (no /dev/urandom available).
            (std::process::id() as u64) ^ 0xDEAD_BEEF
        };
        let path = PathBuf::from(format!("/tmp/dpshell_history.{:016X}", suffix));
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(_) => Some(path),
            Err(e) => {
                eprintln!(
                    "dpshell: warning: cannot create history file {}: {} (history will be in-memory only)",
                    path.display(),
                    e
                );
                None
            }
        }
    };

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
    if let Some(p) = &hist_path {
        let _ = rl.load_history(p);
    }

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
        if let Some(p) = &hist_path {
            let _ = rl.save_history(p);
        }

        // Context for logging (re-fetched each command for accuracy)
        let user = utils::get_current_user();
        let cwd = utils::get_current_working_dir();
        let pid = std::process::id();
        let cwd_str = cwd.to_string_lossy().to_string();

        // Built-ins must be handled in-process (directory changes need to persist).
        let args: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();

        // Exit command (also `exit 0`, `exit 5`, ...): require auth in enforcing mode.
        if args[0] == "exit" {
            if try_exit(&logger, &user, &cwd_str, pid) {
                break;
            }
            continue;
        }

        // Built-in cd — logged like any other command.
        if args[0] == "cd" {
            let msg = match execute_cd(&args[1..]) {
                Ok(_) => "cd executed".to_string(),
                Err(e) => format!("cd failed: {}", e),
            };
            let entry = LogEntry::new("INFO", &user, &mode, cmd, &cwd_str, pid, &msg);
            if let Err(e) = logger.write_entry(&entry) {
                eprintln!("dpshell: log write failed: {}", e);
            }
            if let Err(e) = logger.flush() {
                eprintln!("dpshell: log flush failed: {}", e);
            }
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

            // Defensive: mode was validated at startup; this is unreachable.
            _ => {
                eprintln!("dpshell: internal error: unhandled mode '{}'", mode);
            }
        }
        println!(); // Blank line between commands for readability
    }

    // ── 12. Exit: print goodbye, clean up history ──────────────────────────
    // println!(
    //     "\x1b[32mExited\x1b[0m {}",
    //     chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    // );

    if let Some(p) = &hist_path {
        let _ = std::fs::remove_file(p);
    }

    // Decrement nesting level
    let new_level = dpshell_level.saturating_sub(1);
    unsafe {
        env::set_var("DPSHELL_LEVEL", new_level.to_string());
    }

    let _ = logger.flush();
    Ok(())
}