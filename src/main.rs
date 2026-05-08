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

mod builtins;
mod cd;
mod config;
mod interactive;
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
use builtins::{ShellState,
    builtin_colon, builtin_history, builtin_kill,
    builtin_export, builtin_readonly, builtin_set, builtin_unset, builtin_local,
    read_source_file, builtin_exec, builtin_eval, builtin_trap, builtin_wait,
    builtin_break, builtin_continue, builtin_shift, builtin_test,
    builtin_dirs, builtin_pushd, builtin_popd, builtin_umask,
    builtin_alias, builtin_unalias,
    builtin_help, builtin_type, builtin_command_v,
};
use cd::execute_cd;
use executor::{execute_node, ExecContext};
use jobs::JobManager;
use logger::{JsonLinesWriter, LogEntry};
use nix::unistd::Pid;
use parser::ParseError;
use plugins::{load_plugins, plugin_dirs_for_path, run_plugins, PluginMeta};
use protection::{check_node, ProtectionResult};
use rules::{apply_rules_to_node, check_raw_input, compile_rule, CompiledRule};
use reedline::{Signal, Prompt, PromptHistorySearch, PromptEditMode, Color};
use crate::interactive::{build_editor, FeatureFlags};
use sha2::{Digest, Sha256};
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use syntax::{parse_command_line, CommandNode};

// ──────────────────────────────────────────────────────────────────────────────
// Rustyline helper (unchanged)
// ──────────────────────────────────────────────────────────────────────────────



// ──────────────────────────────────────────────────────────────────────────────
// Reedline prompt adapter
// ──────────────────────────────────────────────────────────────────────────────

struct DpPrompt {
    text: String,
}

impl DpPrompt {
    fn new(dpshell_level: u32) -> Self {
        Self { text: utils::get_prompt(dpshell_level) }
    }
}

impl Prompt for DpPrompt {
    fn render_prompt_left(&self)  -> std::borrow::Cow<str> { std::borrow::Cow::Borrowed(&self.text) }
    fn render_prompt_right(&self) -> std::borrow::Cow<str> { std::borrow::Cow::Borrowed("") }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("")
    }
    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("> ")
    }
    fn render_prompt_history_search_indicator(&self, _: PromptHistorySearch) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("(search) ")
    }
    fn get_prompt_color(&self) -> Color { Color::Reset }
}

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

// handle_export and handle_unset replaced by builtins::builtin_export/unset

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
            core:     config::CoreConfig { mode: "permissive".to_string() },
            auth:     config::AuthConfig::default(),
            paths:    config::PathsConfig::default(),
            rules:    vec![],
            features: config::FeaturesConfig::default(),
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
    let mut state = ShellState::new();

    /// Build an ExecContext from the current state for permissive/disable modes.
    macro_rules! make_ctx {
        (permissive) => {
            ExecContext::permissive_with_fns(&state.functions)
        };
        (enforcing) => {
            ExecContext {
                protected_paths: &protect_paths,
                allowlist:       &allowlist,
                password_hash:   &password_hash,
                enforce:         true,
                functions:       &state.functions,
                call_depth:      0,
            }
        };
    }

    // ── 9. Rustyline ───────────────────────────────────────────────────────

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
    // ── 9. Reedline (fish-style interactive editor) ────────────────────────────
    let feature_flags = FeatureFlags {
        syntax_highlighting: config.features.syntax_highlighting,
        auto_suggest:        config.features.auto_suggest,
        tab_completion:      config.features.tab_completion,
    };
    let mut rl = build_editor(&hist_path, &feature_flags)?;
    let prompt = DpPrompt::new(dpshell_level);

    'repl: loop {
        // Poll background jobs and print completion notices before the prompt.
        jobs.poll_background();

        if interrupted.load(Ordering::SeqCst) {
            interrupted.store(false, Ordering::SeqCst);
            println!();
            continue;
        }

        let line = match rl.read_line(&prompt) {
            Ok(Signal::Success(l)) => l,
            Ok(Signal::CtrlC) => { println!(); continue; }
            Ok(Signal::CtrlD) => {
                println!();
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

        state.push_history(raw_input);

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

        // ── Pre-AST raw-input rule check (catches structural patterns like fork bombs)
        if mode != "disable" {
            if check_raw_input(raw_input, &compiled_rules).is_none() {
                let entry = LogEntry::new("WARN", &user, &mode, raw_input, &cwd_str, pid,
                    "blocked by rule (raw input match)");
                let _ = logger.write_entry(&entry); let _ = logger.flush();
                println!(); continue;
            }
        }

        // ── Built-in dispatch ─────────────────────────────────────────────
        // Only Simple nodes can be built-ins; compound expressions never are.
        if let CommandNode::Simple(ref sc) = node {
            match sc.program.as_str() {
                "exit" => {
                    if try_exit(&logger, &user, &cwd_str, pid) { break 'repl; }
                    continue;
                }
                "export" => { builtin_export(sc.args(), &state); continue; }
                "unset"  => { builtin_unset(sc.args(), &mut state); continue; }
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
                "jobs"     => { jobs.builtin_jobs(); continue; }
                "history"  => { builtin_history(sc.args(), &state); continue; }
                ":"        => { builtin_colon(sc.args()); continue; }
                "kill"     => { builtin_kill(sc.args()); continue; }
                "readonly" => { builtin_readonly(sc.args(), &mut state); continue; }
                "set"      => { builtin_set(sc.args(), &mut state); continue; }
                "source" | "." => {
                    match read_source_file(sc.args().first().map(String::as_str).unwrap_or("")) {
                        Ok(lines) => {
                            for line in lines {
                                if let Ok(node) = crate::syntax::parse_command_line(&line) {
                                    execute_node(&node, &mut jobs, &make_ctx!(permissive));
                                }
                            }
                        }
                        Err(e) => eprintln!("dpshell: source: {}", e),
                    }
                    continue;
                }
                "exec" => {
                    if let Err(e) = builtin_exec(sc.args()) {
                        eprintln!("dpshell: exec: {}", e);
                    }
                    // exec replaces the process; if we get here it failed.
                    continue;
                }
                "eval" => {
                    let cmd = builtin_eval(sc.args());
                    if let Ok(node) = crate::syntax::parse_command_line(&cmd) {
                        execute_node(&node, &mut jobs, &make_ctx!(permissive));
                    }
                    continue;
                }
                "trap"     => { builtin_trap(sc.args(), &mut state); continue; }
                "wait"     => { builtin_wait(sc.args()); continue; }
                "break"    => { builtin_break(sc.args()); continue; }
                "continue" => { builtin_continue(sc.args()); continue; }
                "shift"    => { builtin_shift(sc.args()); continue; }
                "local"    => { builtin_local(sc.args(), &mut state); continue; }
                "test" | "[" => { builtin_test(sc.args()); continue; }
                "dirs"     => { builtin_dirs(sc.args(), &state); continue; }
                "pushd"    => { builtin_pushd(sc.args(), &mut state); continue; }
                "popd"     => { builtin_popd(sc.args(), &mut state); continue; }
                "umask"    => { builtin_umask(sc.args()); continue; }
                "alias"    => { builtin_alias(sc.args(), &mut state); continue; }
                "unalias"  => { builtin_unalias(sc.args(), &mut state); continue; }
                "help"     => { builtin_help(sc.args()); continue; }
                "type"     => { builtin_type(sc.args(), &state); continue; }
                "builtin"  => {
                    // Re-dispatch without alias lookup — just strip "builtin" and re-parse.
                    if let Some(rest) = sc.args().first() {
                        let rebuilt = std::iter::once(rest.clone())
                            .chain(sc.args().iter().skip(1).cloned())
                            .collect::<Vec<_>>().join(" ");
                        if let Ok(node) = crate::syntax::parse_command_line(&rebuilt) {
                            execute_node(&node, &mut jobs, &make_ctx!(permissive));
                        }
                    }
                    continue;
                }
                "command"  => {
                    let args = sc.args();
                    if args.first().map(String::as_str) == Some("-v") {
                        for name in &args[1..] {
                            match builtin_command_v(name) {
                                Some(p) => println!("{}", p.display()),
                                None    => eprintln!("dpshell: command: {}: not found", name),
                            }
                        }
                    } else if let Some(_name) = args.first() {
                        let rebuilt = args.join(" ");
                        if let Ok(node) = crate::syntax::parse_command_line(&rebuilt) {
                            execute_node(&node, &mut jobs, &make_ctx!(permissive));
                        }
                    }
                    continue;
                }
                "return"   => {
                    // In interactive mode, `return` behaves like `exit 0`.
                    let code: i32 = sc.args().first()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    std::process::exit(code);
                }
                _ => {}
            }
        }

        // ── Function definition: store body, do not execute ──────────────
        if let CommandNode::FunctionDef { ref name, ref body } = node {
            state.functions.borrow_mut().insert(name.clone(), *body.clone());
            continue;
        }

        // ── Mode dispatch ──────────────────────────────────────────────────
        match mode.as_str() {

            "disable" => {
                execute_node(&node, &mut jobs, &make_ctx!(permissive));
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
                        println!(); continue;
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
                                None => { println!(); continue; }
                            }
                        } else {
                            node
                        };
                        let msg = if final_str != raw_input {
                            format!("replaced to: {}", final_str)
                        } else { "no replacement".to_string() };
                        execute_node(&final_node, &mut jobs, &make_ctx!(permissive));
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
                        println!(); continue;
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
                                None => { println!(); continue; }
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
                                let ctx = make_ctx!(enforcing);
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
                                    let ctx = make_ctx!(enforcing);
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
                    execute_node(&node, &mut jobs, &make_ctx!(permissive));
                }
            }
        }

        println!();
    }

    // ── Cleanup ────────────────────────────────────────────────────────────
    let new_level = dpshell_level.saturating_sub(1);
    unsafe { env::set_var("DPSHELL_LEVEL", new_level.to_string()); }
    let _ = logger.flush();
    Ok(())
}