// main.rs - ARCHITECTURE.md §2 (Phase 3: Pipelines + Job Control + Tree Audit)
//
// Execution lifecycle (per line):
//
//   raw input (rustyline)
//       │
//       ▼
//   syntax::parse_command_line()     ← shlex + metachar split + AST build
//       │  CommandNode (Simple / Pipeline / Logical)
//       ▼
//   Built-in dispatch?               ← cd/exit/export/unset/fg/bg/jobs
//       │ no
//       ▼
//   rules::apply_rules_to_node()     ← audit every leaf, first-match-wins
//       │  Some(node) | None (blocked)
//       ▼
//   plugins::run_plugins()           ← string-based; receives raw input
//       │  Some(str) | None (blocked)
//       ▼
//   syntax::parse_command_line()     ← re-parse plugin output if changed
//       │
//       ▼
//   protection::check_node()         ← enforcing mode: audit every leaf
//       │
//       ▼
//   executor::execute_node()         ← fork/execve, pipelines, logical ops
//       │  exit code
//       ▼
//   jobs::JobManager                 ← tracks stopped/background jobs

mod cd;
mod config;
mod expand;
mod executor;
mod jobs;
mod logger;
mod parser;
mod plugins;
mod protection;
mod rules;
mod syntax;
mod utils;

use anyhow::Result;
use cd::execute_cd;
use executor::{execute_node, ExecContext};
use jobs::JobManager;
use logger::{JsonLinesWriter, LogEntry};
use nix::unistd::Pid;
use parser::ParseError;
use plugins::{load_plugins, plugin_dirs_for_path, run_plugins, PluginMeta};
use protection::{check_node, ProtectionResult};
use rules::{apply_rules_to_node, compile_rule, CompiledRule};
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
use syntax::{parse_command_line, CommandNode};

// ──────────────────────────────────────────────────────────────────────────────
// Rustyline helper (unchanged)
// ──────────────────────────────────────────────────────────────────────────────

struct DpCompleter {
    filename: FilenameCompleter,
    commands: Vec<String>,
}

impl DpCompleter {
    fn new() -> Self {
        Self {
            filename: FilenameCompleter::new(),
            commands: vec![
                "cd".into(), "exit".into(), "export".into(), "unset".into(),
                "fg".into(), "bg".into(), "jobs".into(), "history".into(),
                "ls".into(), "grep".into(), "rm".into(), "cat".into(),
            ],
        }
    }
}

impl Completer for DpCompleter {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, ctx: &rustyline::Context<'_>)
        -> std::result::Result<(usize, Vec<Pair>), ReadlineError>
    {
        if !line.contains(' ') {
            let prefix = &line[..pos];
            let candidates: Vec<Pair> = self.commands.iter()
                .filter(|c| c.starts_with(prefix))
                .map(|c| Pair { display: c.clone(), replacement: c.clone() })
                .collect();
            if !candidates.is_empty() { return Ok((0, candidates)); }
        }
        self.filename.complete(line, pos, ctx)
    }
}

impl Highlighter for DpCompleter {}
impl Hinter for DpCompleter { type Hint = String; }
impl Validator for DpCompleter {}
impl Helper for DpCompleter {}

// ──────────────────────────────────────────────────────────────────────────────
// Authentication (unchanged)
// ──────────────────────────────────────────────────────────────────────────────

fn authenticate(expected_hash: &str) -> bool {
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 0..MAX_ATTEMPTS {
        let remaining = MAX_ATTEMPTS - attempt;
        let password = match rpassword::prompt_password("Admin password: ") {
            Ok(p) => p,
            Err(e) => { eprintln!("dpshell: failed to read password: {}", e); return false; }
        };
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        if hash == expected_hash { return true; }
        let still_left = remaining - 1;
        if still_left > 0 {
            println!("Authentication failed. {} attempt(s) remaining.", still_left);
        } else {
            println!("Authentication failed.");
        }
    }
    false
}

// ──────────────────────────────────────────────────────────────────────────────
// Built-in helpers
// ──────────────────────────────────────────────────────────────────────────────

fn handle_export(args: &[String]) {
    for arg in args {
        if let Some((k, v)) = arg.split_once('=') {
            unsafe { env::set_var(k, v) };
        }
    }
}

fn handle_unset(args: &[String]) {
    for var in args { unsafe { env::remove_var(var) }; }
}

/// Re-parse a string back to a CommandNode, printing any error and returning None.
fn reparse(s: &str) -> Option<CommandNode> {
    match parse_command_line(s) {
        Ok(n) => Some(n),
        Err(ParseError::Empty) => None,
        Err(ParseError::NotFound(name)) => {
            eprintln!("dpshell: {}: command not found", name);
            None
        }
        Err(ParseError::Lex(msg)) => {
            eprintln!("dpshell: parse error: {}", msg);
            None
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    // ── 1. Configuration ───────────────────────────────────────────────────
    let config = config::load_config().unwrap_or_else(|e| {
        eprintln!("dpshell: warning: could not load config: {}. Using defaults.", e);
        config::Config {
            core:  config::CoreConfig { mode: "permissive".to_string() },
            auth:  config::AuthConfig::default(),
            paths: config::PathsConfig::default(),
            rules: vec![],
        }
    });

    let mode          = config.core.mode.clone();
    let protect_paths = config.paths.protect.clone();
    let allowlist     = config.paths.allowlist.clone();
    let password_hash = config.auth.password_hash.clone();

    let compiled_rules: Vec<CompiledRule> =
        config.rules.iter().filter_map(compile_rule).collect();

    // ── 2. Logger ──────────────────────────────────────────────────────────
    std::fs::create_dir_all("/var/log")
        .map_err(|e| anyhow::anyhow!("Failed to create /var/log: {}", e))?;
    let logger = JsonLinesWriter::new("/var/log/audit.log")
        .map_err(|e| anyhow::anyhow!("Failed to open /var/log/audit.log: {}", e))?;

    // ── 3. Plugins ─────────────────────────────────────────────────────────
    let loaded_plugins: Vec<PluginMeta> = load_plugins();
    if !loaded_plugins.is_empty() {
        eprintln!("dpshell: {} plugin(s) loaded: {}",
            loaded_plugins.len(),
            loaded_plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", "));
        let plugin_dirs = plugin_dirs_for_path(&loaded_plugins);
        let existing = env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = plugin_dirs.iter()
            .map(|d| d.to_string_lossy().into_owned()).collect();
        parts.push(existing);
        unsafe { env::set_var("PATH", parts.join(":")); }
    }

    // ── 4. Nesting level ───────────────────────────────────────────────────
    let dpshell_level = env::var("DPSHELL_LEVEL").ok()
        .and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) + 1;
    unsafe { env::set_var("DPSHELL_LEVEL", dpshell_level.to_string()); }

    // ── 5. History ─────────────────────────────────────────────────────────
    let rand_suffix: u32 = std::process::id() ^ 0xDEAD_BEEF;
    let hist_path = PathBuf::from(format!("/tmp/dpshell_history.{:06X}", rand_suffix));
    let _ = File::create(&hist_path);

    // ── 6. Startup animation ───────────────────────────────────────────────
    utils::start_animation(&mode);

    // ── 7. Signal handling ─────────────────────────────────────────────────
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_clone = Arc::clone(&interrupted);
    ctrlc::set_handler(move || { interrupted_clone.store(true, Ordering::SeqCst); })?;
    unsafe { libc::signal(libc::SIGTSTP, libc::SIG_IGN); }

    // ── 8. Job manager ─────────────────────────────────────────────────────
    let shell_pgid = Pid::from_raw(unsafe { libc::getpgrp() });
    let mut jobs = JobManager::new(shell_pgid);

    // ── 9. Rustyline ───────────────────────────────────────────────────────
    let rl_config = RlConfig::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .build();
    let mut rl: Editor<DpCompleter, _> = Editor::with_config(rl_config)?;
    rl.set_helper(Some(DpCompleter::new()));
    let _ = rl.load_history(&hist_path);
    let prompt = utils::get_prompt(dpshell_level);

    // ── 10. try_exit helper ────────────────────────────────────────────────
    let try_exit = |logger: &JsonLinesWriter, user: &str, cwd_str: &str, pid: u32| -> bool {
        if mode == "enforcing" {
            println!("Authentication required to exit enforcing mode.");
            if !authenticate(&password_hash) {
                println!("Authentication failed. Staying in shell.");
                let entry = LogEntry::new("WARN", user, &mode, "exit", cwd_str, pid,
                    "exit blocked: authentication failed");
                let _ = logger.write_entry(&entry); let _ = logger.flush();
                return false;
            }
            let entry = LogEntry::new("INFO", user, &mode, "exit", cwd_str, pid, "exit authorized");
            let _ = logger.write_entry(&entry); let _ = logger.flush();
        }
        true
    };

    // ── 11. REPL ───────────────────────────────────────────────────────────
    'repl: loop {
        // Poll background jobs and print completion notices before the prompt.
        jobs.poll_background();

        if interrupted.load(Ordering::SeqCst) {
            interrupted.store(false, Ordering::SeqCst);
            continue;
        }

        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => { continue; }
            Err(ReadlineError::Eof) => {
                let user = utils::get_current_user();
                let cwd_str = utils::get_current_working_dir().to_string_lossy().to_string();
                let pid = std::process::id();
                if try_exit(&logger, &user, &cwd_str, pid) { break; }
                continue;
            }
            Err(e) => { eprintln!("dpshell: readline error: {}", e); break; }
        };

        let raw_input = line.trim();

        if raw_input == "\x0C" {
            print!("\x1b[2J\x1b[H");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            continue;
        }
        if raw_input.is_empty() { continue; }

        let _ = rl.add_history_entry(raw_input);
        let _ = rl.save_history(&hist_path);

        // ── Parse ──────────────────────────────────────────────────────────
        let node = match parse_command_line(raw_input) {
            Ok(n) => n,
            Err(ParseError::Empty) => continue,
            Err(ParseError::NotFound(name)) => {
                eprintln!("dpshell: {}: command not found", name);
                continue;
            }
            Err(ParseError::Lex(msg)) => {
                eprintln!("dpshell: parse error: {}", msg);
                continue;
            }
        };

        let user    = utils::get_current_user();
        let cwd     = utils::get_current_working_dir();
        let pid     = std::process::id();
        let cwd_str = cwd.to_string_lossy().to_string();

        // ── Built-in dispatch ─────────────────────────────────────────────
        // Only Simple nodes can be built-ins; compound expressions never are.
        if let CommandNode::Simple(ref sc) = node {
            match sc.program.as_str() {
                "exit" => {
                    if try_exit(&logger, &user, &cwd_str, pid) { break 'repl; }
                    continue;
                }
                "export" => { handle_export(sc.args()); continue; }
                "unset"  => { handle_unset(sc.args());  continue; }
                "cd"     => {
                    let _ = execute_cd(&sc.args().to_vec());
                    continue;
                }
                "fg" => {
                    let id = sc.args().first().and_then(|s| s.parse::<usize>().ok());
                    jobs.builtin_fg(id);
                    continue;
                }
                "bg" => {
                    let id = sc.args().first().and_then(|s| s.parse::<usize>().ok());
                    jobs.builtin_bg(id);
                    continue;
                }
                "jobs" => { jobs.builtin_jobs(); continue; }
                _ => {}
            }
        }

        // ── Mode dispatch ──────────────────────────────────────────────────
        match mode.as_str() {

            "disable" => {
                execute_node(&node, &mut jobs, &ExecContext::permissive());
                let entry = LogEntry::new("INFO", &user, &mode, raw_input, &cwd_str, pid,
                    "command executed (disable mode)");
                let _ = logger.write_entry(&entry);
                let _ = logger.flush();
            }

            "permissive" => {
                // Rules — operate on the AST.
                let node = match apply_rules_to_node(node, &compiled_rules) {
                    None => {
                        let entry = LogEntry::new("WARN", &user, &mode, raw_input, &cwd_str, pid,
                            "blocked by rule");
                        let _ = logger.write_entry(&entry); let _ = logger.flush();
                        continue;
                    }
                    Some(n) => n,
                };

                // Plugins — receive raw string representation of the (possibly
                // rule-modified) command; re-parse their output.
                let raw_for_plugins = raw_input.to_string(); // original for plugins
                match run_plugins(&loaded_plugins, &raw_for_plugins) {
                    None => {
                        let entry = LogEntry::new("WARN", &user, &mode, raw_input, &cwd_str, pid,
                            "blocked by plugin");
                        let _ = logger.write_entry(&entry); let _ = logger.flush();
                    }
                    Some(final_str) => {
                        let final_node = if final_str != raw_for_plugins {
                            match reparse(&final_str) {
                                Some(n) => n,
                                None => { continue; }
                            }
                        } else {
                            node
                        };
                        let msg = if final_str != raw_input {
                            format!("replaced to: {}", final_str)
                        } else { "no replacement".to_string() };
                        execute_node(&final_node, &mut jobs, &ExecContext::permissive());
                        let entry = LogEntry::new("INFO", &user, &mode, raw_input,
                            &cwd_str, pid, &msg);
                        let _ = logger.write_entry(&entry);
                        let _ = logger.flush();
                    }
                }
            }

            "enforcing" => {
                let node = match apply_rules_to_node(node, &compiled_rules) {
                    None => {
                        let entry = LogEntry::new("WARN", &user, &mode, raw_input, &cwd_str, pid,
                            "blocked by rule");
                        let _ = logger.write_entry(&entry); let _ = logger.flush();
                        continue;
                    }
                    Some(n) => n,
                };

                match run_plugins(&loaded_plugins, raw_input) {
                    None => {
                        let entry = LogEntry::new("WARN", &user, &mode, raw_input, &cwd_str, pid,
                            "blocked by plugin");
                        let _ = logger.write_entry(&entry); let _ = logger.flush();
                    }
                    Some(final_str) => {
                        let final_node = if final_str != raw_input {
                            match reparse(&final_str) {
                                Some(n) => n,
                                None => { continue; }
                            }
                        } else {
                            node
                        };

                        // Phase 3: audit the full tree — every leaf is checked.
                        match check_node(&final_node, &protect_paths, &allowlist) {
                            ProtectionResult::Allowed => {
                                let msg = if final_str != raw_input {
                                    format!("replaced to: {}", final_str)
                                } else { "no replacement".to_string() };
                                let ctx = ExecContext {
                                    protected_paths: &protect_paths,
                                    allowlist:       &allowlist,
                                    password_hash:   &password_hash,
                                    enforce:         true,
                                };
                                execute_node(&final_node, &mut jobs, &ctx);
                                let entry = LogEntry::new("INFO", &user, &mode, raw_input,
                                    &cwd_str, pid, &msg);
                                let _ = logger.write_entry(&entry);
                                let _ = logger.flush();
                            }
                            ProtectionResult::Blocked(offender) => {
                                println!("\x1b[31;5m[!]\x1b[0m Blocked: '{}' targets a \
                                    protected path and is not allowlisted.", offender);
                                let entry = LogEntry::new("WARN", &user, &mode, raw_input,
                                    &cwd_str, pid, &format!("blocked: {}", offender));
                                let _ = logger.write_entry(&entry);
                                let _ = logger.flush();
                            }
                            ProtectionResult::RequiresAuth(offender) => {
                                println!("\x1b[31;5m[!]\x1b[0m '{}' requires authorization.",
                                    offender);
                                if authenticate(&password_hash) {
                                    println!("Authorization granted. Executing...");
                                    let ctx = ExecContext {
                                        protected_paths: &protect_paths,
                                        allowlist:       &allowlist,
                                        password_hash:   &password_hash,
                                        enforce:         true,
                                    };
                                    execute_node(&final_node, &mut jobs, &ctx);
                                    let entry = LogEntry::new("INFO", &user, &mode, raw_input,
                                        &cwd_str, pid,
                                        &format!("auth granted for: {}", offender));
                                    let _ = logger.write_entry(&entry);
                                    let _ = logger.flush();
                                } else {
                                    println!("Authorization denied.");
                                    let entry = LogEntry::new("WARN", &user, &mode, raw_input,
                                        &cwd_str, pid,
                                        &format!("auth failed for: {}", offender));
                                    let _ = logger.write_entry(&entry);
                                    let _ = logger.flush();
                                }
                            }
                        }
                    }
                }
            }

            other => {
                eprintln!("dpshell: unknown mode '{}', treating as permissive", other);
                if let Some(node) = apply_rules_to_node(node, &compiled_rules) {
                    execute_node(&node, &mut jobs, &ExecContext::permissive());
                }
            }
        }
    }

    // ── Cleanup ────────────────────────────────────────────────────────────
    let _ = std::fs::remove_file(&hist_path);
    let new_level = dpshell_level.saturating_sub(1);
    unsafe { env::set_var("DPSHELL_LEVEL", new_level.to_string()); }
    let _ = logger.flush();
    Ok(())
}