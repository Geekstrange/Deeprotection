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
mod config;
mod executor;
mod interactive;
mod jobs;
mod logging;
mod parser;
mod security;
mod shell;
mod utils;

use crate::interactive::{build_editor, FeatureFlags};
use anyhow::Result;
use builtins::registry::default_builtins;
use builtins::{builtin_command_v, builtin_eval, builtin_exec, read_source_file, ShellState};
use builtins::cd::execute_cd;
use executor::{execute_node, expand_command_substitutions, ExecContext};
use jobs::JobManager;
use logging::{JsonLinesWriter, LogEntry};
use nix::unistd::Pid;
use parser::syntax::{parse_command_line, CommandNode};
use parser::ParseError;
use reedline::{Color, Prompt, PromptEditMode, PromptHistorySearch, Signal};
use security::plugins::{load_plugins, plugin_dirs_for_path, run_plugins, PluginMeta};
use security::protection::{check_node, ProtectionResult};
use security::rules::{apply_rules_to_node, check_raw_input, compile_rule, CompiledRule};
use sha2::{Digest, Sha256};
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
        Self {
            text: utils::get_prompt(dpshell_level),
        }
    }
}

impl Prompt for DpPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed(&self.text)
    }
    fn render_prompt_right(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("")
    }
    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("> ")
    }
    fn render_prompt_history_search_indicator(
        &self,
        _: PromptHistorySearch,
    ) -> std::borrow::Cow<str> {
        std::borrow::Cow::Borrowed("(search) ")
    }
    fn get_prompt_color(&self) -> Color {
        Color::Reset
    }
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
            println!(
                "Authentication failed. {} attempt(s) remaining.",
                still_left
            );
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

fn is_var_assignment(s: &str) -> bool {
    if let Some(eq) = s.find('=') {
        let name = &s[..eq];
        !name.is_empty()
            && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

fn try_execute_assignments(sc: &parser::syntax::SimpleCommand, state: &mut ShellState) -> Option<i32> {
    if !is_var_assignment(&sc.program) {
        return None;
    }
    for word in &sc.argv {
        if !is_var_assignment(word) {
            return None;
        }
    }
    for word in &sc.argv {
        if let Some((k, v)) = word.split_once('=') {
            if state.readonly_vars.contains(k) {
                eprintln!("dpshell: {}: readonly variable", k);
                continue;
            }
            state.shell_vars.insert(k.to_string(), v.to_string());
            unsafe { env::set_var(k, v); }
        }
    }
    Some(0)
}

// ──────────────────────────────────────────────────────────────────────────────
// Main
// ──────────────────────────────────────────────────────────────────────────────

/// Returns true if the shell is allowed to exit.
/// In enforcing mode, requires successful password authentication.
fn try_exit(
    logger: &logging::JsonLinesWriter,
    user: &str,
    mode: &str,
    password_hash: &str,
    cwd_str: &str,
    pid: u32,
) -> bool {
    if mode == "enforcing" {
        println!("Authentication required to exit enforcing mode.");
        if !authenticate(password_hash) {
            println!("Authentication failed. Staying in shell.");
            let entry = LogEntry::new(
                "WARN",
                user,
                mode,
                "exit",
                cwd_str,
                pid,
                "exit blocked: authentication failed",
            );
            let _ = logger.write_entry(&entry);
            let _ = logger.flush();
            return false;
        }
        let entry = LogEntry::new("INFO", user, mode, "exit", cwd_str, pid, "exit authorized");
        let _ = logger.write_entry(&entry);
        let _ = logger.flush();
    }
    true
}

fn main() -> Result<()> {
    // ── 1. Configuration ───────────────────────────────────────────────────
    let initial_config = config::load_config().unwrap_or_else(|e| {
        eprintln!(
            "dpshell: warning: could not load config: {}. Using defaults.",
            e
        );
        config::Config {
            core: config::CoreConfig::default(),
            auth: config::AuthConfig::default(),
            paths: config::PathsConfig::default(),
            rules: vec![],
            features: config::FeaturesConfig::default(),
        }
    });

    let bash_compat = initial_config.core.bash_compat;
    let dynamic_config = initial_config.core.dynamic_config;

    // Snapshot the fields that drive the REPL; refreshed on reload.
    let mut mode = initial_config.core.mode.clone();
    let mut protect_paths = initial_config.paths.protect.clone();
    let mut allowlist = initial_config.paths.allowlist.clone();
    let mut password_hash = initial_config.auth.password_hash.clone();
    let mut compiled_rules: Vec<CompiledRule> =
        initial_config.rules.iter().filter_map(compile_rule).collect();

    // ── 2. Logger ──────────────────────────────────────────────────────────
    std::fs::create_dir_all("/var/log")
        .map_err(|e| anyhow::anyhow!("Failed to create /var/log: {}", e))?;
    let logger = JsonLinesWriter::new("/var/log/audit.log")
        .map_err(|e| anyhow::anyhow!("Failed to open /var/log/audit.log: {}", e))?;

    // ── 3. Plugins ─────────────────────────────────────────────────────────
    let loaded_plugins: Vec<PluginMeta> = load_plugins();
    if !loaded_plugins.is_empty() {
        eprintln!(
            "dpshell: {} plugin(s) loaded: {}",
            loaded_plugins.len(),
            loaded_plugins
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let plugin_dirs = plugin_dirs_for_path(&loaded_plugins);
        let existing = env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = plugin_dirs
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect();
        parts.push(existing);
        unsafe {
            env::set_var("PATH", parts.join(":"));
        }
    }

    // ── 4. Nesting level ───────────────────────────────────────────────────
    let dpshell_level = env::var("DPSHELL_LEVEL")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    unsafe {
        env::set_var("DPSHELL_LEVEL", dpshell_level.to_string());
    }

    // ── 5. History ─────────────────────────────────────────────────────────
    // bash_compat=true  → ~/.bash_history (compatible with bash history format)
    // bash_compat=false → /tmp/dpshell_history.<pid-xor> (dpshell default)
    let hist_path = if bash_compat {
        let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        PathBuf::from(home).join(".bash_history")
    } else {
        let rand_suffix: u32 = std::process::id() ^ 0xDEAD_BEEF;
        PathBuf::from(format!("/tmp/dpshell_history.{:06X}", rand_suffix))
    };

    // ── 5b. Dynamic config: track file content for change detection ──────
    // Instead of a background thread, we check the file content directly in
    // the REPL loop (before each command).  This is simple, race-free, and
    // the I/O cost of reading a small config file once per command is negligible.
    let mut last_config_content = if dynamic_config {
        std::fs::read_to_string(config::CONFIG_PATH).unwrap_or_default()
    } else {
        String::new()
    };

    // ── 7. Signal handling ─────────────────────────────────────────────────
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_clone = Arc::clone(&interrupted);
    ctrlc::set_handler(move || {
        interrupted_clone.store(true, Ordering::SeqCst);
    })?;
    unsafe {
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
    }

    // ── 8. Job manager ─────────────────────────────────────────────────────
    let shell_pgid = Pid::from_raw(unsafe { libc::getpgrp() });
    let mut jobs = JobManager::new(shell_pgid);
    let mut state = ShellState::new();
    if bash_compat {
        if let Ok(contents) = std::fs::read_to_string(&hist_path) {
            state.history = contents.lines().map(String::from).collect();
        }
    }
    let builtin_registry = default_builtins();
    let mut last_exit: i32 = 0;

    /// Build an ExecContext from the current state for permissive/disable modes.
    macro_rules! make_ctx {
        (permissive) => {
            ExecContext::permissive_with_fns(&state.functions, &state.shell_vars, last_exit)
        };
        (enforcing) => {
            ExecContext {
                protected_paths: &protect_paths,
                allowlist: &allowlist,
                password_hash: &password_hash,
                enforce: true,
                functions: &state.functions,
                call_depth: 0,
                shell_vars: &state.shell_vars,
                last_exit,
                positional_params: &[],
            }
        };
    }

    // ── 8b. Login shell detection ──────────────────────────────────────────
    let cli_args: Vec<String> = env::args().collect();
    let _is_login = cli_args.iter().any(|a| a == "-l" || a == "--login")
        || cli_args
            .first()
            .and_then(|a| std::path::Path::new(a).file_name())
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('-'))
            .unwrap_or(false);

    // ── 9. Non-interactive execution: script file or -c command ─────────────
    if cli_args.len() > 1 {
        let script_lines: Vec<String>;

        if cli_args[1] == "-c" {
            // dpshell -c 'command string' — preprocess heredocs, then execute as
            // a single logical command rather than splitting blindly on newlines.
            if let Some(cmd) = cli_args.get(2) {
                let processed = preprocess_heredocs(cmd);
                script_lines = processed.lines().map(str::to_string).collect();
            } else {
                eprintln!("dpshell: -c: option requires an argument");
                std::process::exit(2);
            }
        } else {
            // dpshell script.sh [args...]
            let path = &cli_args[1];
            match std::fs::read_to_string(path) {
                Ok(contents) => {
                    let processed = preprocess_heredocs(&contents);
                    script_lines = processed
                        .lines()
                        .map(str::to_string)
                        .filter(|l| !l.starts_with("#!"))
                        .collect();
                }
                Err(e) => {
                    eprintln!("dpshell: {}: {}", path, e);
                    std::process::exit(127);
                }
            }
        }

        // Join lines into complete logical units (multi-line for/if/while/case/functions)
        let logical_lines: Vec<String> = {
            let mut out: Vec<String> = Vec::new();
            let mut accum = String::new();
            for line in &script_lines {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    if accum.is_empty() {
                        continue;
                    }
                    accum.push('\n');
                    continue;
                }
                if accum.is_empty() {
                    accum = trimmed.to_string();
                } else {
                    accum.push('\n');
                    accum.push_str(trimmed);
                }
                if !has_unclosed_block(&accum) {
                    out.push(std::mem::take(&mut accum));
                }
            }
            if !accum.is_empty() {
                out.push(accum);
            }
            out
        };

        // Execute all logical lines and exit
        for line in &logical_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // ── Security: raw-input rule check (fork bomb detection etc.) ──
            if mode != "disable" {
                if check_raw_input(trimmed, &compiled_rules).is_none() {
                    eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m Blocked by rule (raw input match)");
                    last_exit = 1;
                    continue;
                }
            }

            let after_alias = parser::expand_vars::expand_alias(trimmed, &state.aliases);
            let heredocs_done = preprocess_heredocs(&after_alias);

            match parse_command_line(&heredocs_done) {
                Ok(node) => {
                    // ── Security: AST-level rule check ────────────────────
                    if mode != "disable" {
                        if let Some(checked_node) = apply_rules_to_node(node, &compiled_rules) {
                            if let CommandNode::FunctionDef { ref name, ref body } = checked_node {
                                state
                                    .functions
                                    .borrow_mut()
                                    .insert(name.clone(), *body.clone());
                                continue;
                            }
                            last_exit =
                                execute_node(&checked_node, &mut jobs, &make_ctx!(permissive));
                        } else {
                            eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m Blocked by rule");
                            last_exit = 1;
                        }
                    } else {
                        // disable mode — no rule checks
                        if let CommandNode::FunctionDef { ref name, ref body } = node {
                            state
                                .functions
                                .borrow_mut()
                                .insert(name.clone(), *body.clone());
                            continue;
                        }
                        last_exit = execute_node(&node, &mut jobs, &make_ctx!(permissive));
                    }
                }
                Err(ParseError::Empty) => {}
                Err(ParseError::NotFound(name)) => {
                    eprintln!("dpshell: {}: command not found", name);
                    last_exit = 127;
                }
                Err(ParseError::Lex(msg)) => {
                    eprintln!("dpshell: {}", msg);
                    last_exit = 2;
                }
            }
        }

        let new_level = dpshell_level.saturating_sub(1);
        unsafe {
            env::set_var("DPSHELL_LEVEL", new_level.to_string());
        }
        std::process::exit(last_exit);
    }

    // ── 10. Startup animation (interactive only, skipped in bash_compat mode) ──
    if !bash_compat {
        utils::start_animation(&mode);
    }

    // ── 10b. bash_compat: source ~/.bashrc into initial shell state ────────
    // Best-effort: execute simple lines (exports, aliases, simple commands).
    // Multi-line constructs (if/for/while, functions) and bash-specific features
    // (shopt, [[, $(...)) are silently skipped — dpshell's line-by-line parser
    // cannot handle them.
    if bash_compat {
        let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let bashrc = PathBuf::from(&home).join(".bashrc");
        if bashrc.exists() {
            if let Ok(contents) = std::fs::read_to_string(&bashrc) {
                // Track brace/block depth to skip multi-line constructs.
                let mut block_depth: i32 = 0;

                for raw_line in contents.lines() {
                    let line = raw_line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }

                    // Track multi-line block depth (functions, if/for/while/case).
                    // Skip all lines inside blocks we can't parse.
                    let first_word = line.split_whitespace().next().unwrap_or("");
                    match first_word {
                        "if" | "for" | "while" | "until" | "case" => {
                            if !line.contains("fi") && !line.contains("done") && !line.contains("esac") {
                                block_depth += 1;
                                continue;
                            }
                        }
                        _ => {}
                    }
                    if line.ends_with('{') || line.contains("() {") || line.contains("(){") {
                        block_depth += 1;
                        continue;
                    }
                    if line == "}" || line.starts_with("}") {
                        block_depth -= 1;
                        continue;
                    }
                    if line == "fi" || line == "done" || line == "esac" {
                        block_depth -= 1;
                        continue;
                    }
                    if block_depth > 0 {
                        continue;
                    }

                    // Skip bash-only builtins and unsupported syntax.
                    match first_word {
                        "shopt" | "complete" | "compopt" | "declare" | "typeset"
                        | "local" | "let" | "select" | "then" | "else" | "elif"
                        | "do" | "function" => continue,
                        _ => {}
                    }

                    // Skip lines with unsupported bash syntax: $(...), [[ ]], (( ))
                    if line.contains("$(" ) || line.contains("[[") || line.contains("((") {
                        continue;
                    }

                    // Security: raw-input check
                    if mode != "disable" {
                        if check_raw_input(line, &compiled_rules).is_none() {
                            continue;
                        }
                    }

                    // Variable expansion
                    let after_alias = parser::expand_vars::expand_alias(line, &state.aliases);
                    let heredocs_done = preprocess_heredocs(&after_alias);
                    let expanded = parser::expand_vars::expand_line(&heredocs_done, &state.shell_vars, last_exit);
                    let cmd_sub =
                        expand_command_substitutions(&expanded, &mut jobs, &make_ctx!(permissive));

                    let node = match parse_command_line(&cmd_sub) {
                        Ok(n) => n,
                        // Silently skip unparseable lines in .bashrc
                        Err(_) => continue,
                    };

                    // Dispatch
                    if let CommandNode::Simple(ref sc) = node {
                        match sc.program.as_str() {
                            "cd" => { let _ = execute_cd(&sc.args().to_vec()); continue; }
                            "exit" | "return" => continue,
                            "eval" | "source" | "." => continue, // can't handle subshells/nested source
                            _ => {}
                        }
                        if let Some(reg) = builtin_registry.get(sc.program.as_str()) {
                            last_exit = (reg.execute)(sc.args(), &mut state);
                            continue;
                        }
                    }
                    if let CommandNode::FunctionDef { ref name, ref body } = node {
                        state.functions.borrow_mut().insert(name.clone(), *body.clone());
                        continue;
                    }

                    // Execute
                    if mode != "disable" {
                        if let Some(checked) = apply_rules_to_node(node, &compiled_rules) {
                            execute_node(&checked, &mut jobs, &make_ctx!(permissive));
                        }
                    } else {
                        execute_node(&node, &mut jobs, &make_ctx!(permissive));
                    }
                }
            }
        }
    }

    // ── 11. REPL ───────────────────────────────────────────────────────────
    let feature_flags = FeatureFlags {
        syntax_highlighting: initial_config.features.syntax_highlighting,
        auto_suggest: initial_config.features.auto_suggest,
        enhance_completion: initial_config.features.enhance_completion,
    };
    let mut rl = build_editor(&hist_path, &feature_flags)?;
    let prompt = DpPrompt::new(dpshell_level);

    /// Preprocess heredocs in an input string that may span multiple lines.
    /// Finds `<<DELIM` / `<<-DELIM` patterns, extracts the body (lines between
    /// the operator and the delimiter on its own line), writes the body to a
    /// temp file, and replaces the heredoc syntax with `<TEMPFILE`.
    ///
    /// Handles multiple heredocs in one input (e.g. `cat <<A <<B`).
    pub fn preprocess_heredocs(input: &str) -> String {
        // Find all heredoc operator positions with their delimiters.
        struct HdPos { start: usize, end: usize, delim: String, strip_tabs: bool }
        let mut positions: Vec<HdPos> = Vec::new();

        {
            let chars: Vec<char> = input.chars().collect();
            let len = chars.len();
            let mut i = 0;
            while i < len {
                match chars[i] {
                    '\'' => { i += 1; while i < len && chars[i] != '\'' { i += 1; } if i < len { i += 1; } }
                    '"' => { i += 1; while i < len && chars[i] != '"' { if chars[i] == '\\' { i += 1; } i += 1; } if i < len { i += 1; } }
                    '\\' => { i += 2; }
                    '<' if i + 1 < len && chars[i + 1] == '<' => {
                        let op_start = i;
                        let mut strip = false;
                        i += 2;
                        if i < len && chars[i] == '-' { strip = true; i += 1; }
                        while i < len && chars[i].is_ascii_whitespace() && chars[i] != '\n' { i += 1; }
                        let dstart = i;
                        while i < len && !chars[i].is_ascii_whitespace() && !matches!(chars[i], '|' | ';' | '&' | '<' | '>') { i += 1; }
                        let delim: String = chars[dstart..i].iter().collect();
                        if !delim.is_empty() && delim != "/tmp/dpshell_hd_" {
                            let byte_start = input.chars().take(op_start).map(|c| c.len_utf8()).sum::<usize>();
                            let byte_end = input.chars().take(i).map(|c| c.len_utf8()).sum::<usize>();
                            positions.push(HdPos { start: byte_start, end: byte_end, delim, strip_tabs: strip });
                        }
                    }
                    _ => { i += 1; }
                }
            }
        }

        if positions.is_empty() {
            return input.to_string();
        }

        let mut result = input.to_string();
        // Process in reverse order so byte offsets stay valid.
        positions.reverse();
        for hd in &positions {
            // Find the body: scan forward from hd.end for lines until a line
            // consisting solely of the delimiter (with optional trailing \r).
            let after_op = &result[hd.end..];
            let mut body_end = None;
            let mut scan = 0usize;
            let delim_str = &hd.delim;
            loop {
                let line_start = scan;
                match after_op[scan..].find('\n') {
                    Some(offset) => {
                        let line = &after_op[line_start..scan + offset];
                        let line_trim = line.trim_end_matches('\r');
                        if line_trim == delim_str {
                            body_end = Some(line_start); // body is everything before this line
                            scan = scan + offset + 1; // position after the delimiter line
                            break;
                        }
                        scan = scan + offset + 1;
                    }
                    None => break,
                }
            }

            // If the delimiter wasn't found with trailing \n, check if the
            // remainder of after_op (after the last scanned line) is the
            // delimiter at end-of-input (no trailing newline).
            if body_end.is_none() && scan < after_op.len() {
                let last = after_op[scan..].trim_end();
                if last == delim_str {
                    body_end = Some(scan);
                }
            }

            if let Some(body_start_pos) = body_end {
                // Skip leading newline after the heredoc operator.
                let body_text = after_op[..body_start_pos].trim_start_matches('\n');
                let body = if hd.strip_tabs {
                    body_text.lines().map(|l| l.strip_prefix('\t').unwrap_or(l)).collect::<Vec<_>>().join("\n")
                } else {
                    body_text.to_string()
                };

                let tmp = format!("/tmp/dpshell_hd_{}_{}", std::process::id(), hd.delim);
                if std::fs::write(&tmp, &body).is_err() {
                    continue;
                }

                // Remove the heredoc operator, body, and delimiter (but keep the
                // newline after the delimiter as a command separator).
                let scan_end = body_start_pos + delim_str.len(); // delimiter line without trailing \n

                // Replace: `<<DELIM\nBODY\nDELIM` → `<TEMPFILE`
                let range_start = hd.start;
                let range_end = hd.end + scan_end;
                let replacement = format!("<{}", tmp);

                let mut new_result = String::with_capacity(result.len());
                new_result.push_str(&result[..range_start]);
                new_result.push_str(&replacement);
                new_result.push_str(&result[range_end..]);
                result = new_result;
            } else {
                // No end delimiter found on its own line: keep the operator but replace
                // with a temp file redirect so parse_simple still works. We use an empty
                // file (body is everything after the operator until EOF).
                let body = after_op.trim();
                let tmp = format!("/tmp/dpshell_hd_{}_{}", std::process::id(), hd.delim);
                let body_text = if hd.strip_tabs {
                    body.lines().map(|l| l.strip_prefix('\t').unwrap_or(l)).collect::<Vec<_>>().join("\n")
                } else {
                    body.to_string()
                };
                let _ = std::fs::write(&tmp, &body_text);

                let replacement = format!("<{}", tmp);
                let mut new_result = String::with_capacity(result.len());
                new_result.push_str(&result[..hd.start]);
                new_result.push_str(&replacement);
                new_result.push_str(&result[hd.end..]);
                result = new_result;
            }
        }

        result
    }

    /// Read a possibly-multi-line command from the REPL.
    /// Collects heredoc bodies and handles line continuations on `|`, `&&`, `||`.
    /// Returns None on EOF/error, or Some(processed_input).
    fn read_multi_line(
        rl: &mut reedline::Reedline,
        prompt: &DpPrompt,
    ) -> Option<String> {
        let first = match rl.read_line(prompt) {
            Ok(Signal::Success(l)) => l,
            Ok(Signal::CtrlC) => return Some(String::new()),
            Ok(Signal::CtrlD) => return None,
            Err(e) => {
                eprintln!("dpshell: readline error: {}", e);
                return None;
            }
        };

        let trimmed = first.trim();
        if trimmed.is_empty() {
            return Some(String::new());
        }

        // No heredocs or continuations — return immediately.
        if !trimmed.contains("<<") && !needs_continuation(trimmed) {
            return Some(first);
        }

        // Accumulate lines.
        let mut accumulated = first;
        let multi_prompt = DpPrompt { text: "> ".to_string() };

        'ml: loop {
            // ── Collect pending heredoc bodies ──────────────────────────
            let n = count_pending_heredocs(&accumulated, &String::new());
            let mut heredoc_bodies: Vec<(String, String)> = Vec::new(); // (delim, body)
            for _ in 0..n {
                let mut body = String::new();
                let delim = get_last_heredoc_delim(&accumulated);
                if delim.is_empty() { break; }

                loop {
                    let line = match rl.read_line(&multi_prompt) {
                        Ok(Signal::Success(l)) => l,
                        Ok(Signal::CtrlC) => break 'ml,
                        Ok(Signal::CtrlD) => break,
                        Err(_) => break,
                    };
                    let line_trimmed = line.trim_end_matches('\n');
                    // Strip leading tab if <<-
                    let delim_stripped = delim.trim_start_matches('-');
                    if line_trimmed == delim || line_trimmed == delim_stripped {
                        break;
                    }
                    body.push_str(&line);
                }
                if !delim.is_empty() {
                    heredoc_bodies.push((delim, body));
                }
            }

            // ── Replace heredocs with temp-file redirects ───────────────
            for (delim, body) in &heredoc_bodies {
                let tmp = format!("/tmp/dpshell_hd_{}", std::process::id());
                if let Err(e) = std::fs::write(&tmp, body) {
                    eprintln!("dpshell: heredoc: temp file error: {}", e);
                } else {
                    // Replace `<<DELIM` or `<<-DELIM` with `<TMPFILE`
                    let pattern = if delim.starts_with('-') {
                        format!("<<-{}", &delim[1..])
                    } else {
                        format!("<<{}", delim)
                    };
                    accumulated = accumulated.replacen(&pattern, &format!("<{}", tmp), 1);
                }
            }

            // ── Check if we still need more input ─────────────────────
            if !needs_continuation(accumulated.trim()) && !accumulated.contains("<<") {
                break;
            }

            // Read another continuation line
            let extra = match rl.read_line(&multi_prompt) {
                Ok(Signal::Success(l)) => l,
                Ok(Signal::CtrlC) => break,
                Ok(Signal::CtrlD) => break,
                Err(_) => break,
            };
            if extra.trim().is_empty() {
                break;
            }
            accumulated.push('\n');
            accumulated.push_str(&extra);
        }

        Some(accumulated)
    }

    /// Returns true if the input has an unclosed control structure
    /// (if without fi, for/while/until without done, case without esac,
    /// unmatched `{` without `}`).
    fn has_unclosed_block(input: &str) -> bool {
        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();
        let mut i = 0;

        let mut if_depth: i32 = 0;
        let _for_depth: i32 = 0;
        let mut while_depth: i32 = 0;
        let mut case_depth: i32 = 0;
        let mut brace_depth: i32 = 0;

        while i < len {
            match chars[i] {
                '\'' => { i += 1; while i < len && chars[i] != '\'' { i += 1; } if i < len { i += 1; } }
                '"' => { i += 1; while i < len && chars[i] != '"' { if chars[i] == '\\' { i += 1; } i += 1; } if i < len { i += 1; } }
                '\\' => { i += 2; }
                '#' => {
                    while i < len && chars[i] != '\n' { i += 1; }
                }
                '{' => {
                    let glued = i > 0 && !chars[i-1].is_ascii_whitespace() && chars[i-1] != ';' && chars[i-1] != '\n';
                    if !glued {
                        brace_depth += 1;
                    }
                    i += 1;
                }
                '}' => {
                    let glued = i > 0 && !chars[i-1].is_ascii_whitespace() && chars[i-1] != ';' && chars[i-1] != '\n';
                    if !glued {
                        brace_depth -= 1;
                    }
                    i += 1;
                }
                _ => {
                    if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
                        let start = i;
                        while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') { i += 1; }
                        let word: String = chars[start..i].iter().collect();
                        match word.as_str() {
                            "if" => if_depth += 1,
                            "fi" => if_depth = (if_depth - 1).max(0),
                            "for" | "while" | "until" | "select" => while_depth += 1,
                            "done" => while_depth = (while_depth - 1).max(0),
                            "case" => case_depth += 1,
                            "esac" => case_depth = (case_depth - 1).max(0),
                            _ => {}
                        }
                        continue;
                    }
                    i += 1;
                }
            }
        }

        if_depth > 0 || while_depth > 0 || case_depth > 0 || brace_depth > 0
    }

    /// Returns true if the line looks like it needs more input
    /// (ends with unclosed pipe, &&, ||).
    fn needs_continuation(line: &str) -> bool {
        let line = line.trim();
        has_unclosed_block(line)
            || line.ends_with('|') && !line.ends_with("||") && !line.ends_with(">|")
            || line.ends_with("&&")
            || line.ends_with("||")
    }

    /// Count pending heredocs in `input` whose body hasn't been collected yet.
    /// A heredoc is "pending" if `<<` or `<<-` appears outside quotes and has
    /// a delimiter token following it, but no body yet.
    fn count_pending_heredocs(input: &str, _collected: &str) -> usize {
        let mut count = 0usize;
        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();
        let mut i = 0;
        while i < len {
            match chars[i] {
                '\'' => { i += 1; while i < len && chars[i] != '\'' { i += 1; } if i < len { i += 1; } }
                '"' => { i += 1; while i < len && chars[i] != '"' { if chars[i] == '\\' { i += 1; } i += 1; } if i < len { i += 1; } }
                '\\' => { i += 2; }
                '<' if i + 1 < len && chars[i + 1] == '<' => {
                    // Check if we have a delimiter after the operator.
                    let mut j = i + 2;
                    if j < len && chars[j] == '-' { j += 1; }
                    while j < len && chars[j].is_ascii_whitespace() { j += 1; }
                    if j < len && !matches!(chars[j], '|' | ';' | '&' | '<' | '>' | '\n') {
                        count += 1;
                    }
                    i = j;
                }
                _ => { i += 1; }
            }
        }
        // Subtract heredocs already replaced with temp files.
        count.saturating_sub(input.matches("/tmp/dpshell_hd_").count())
    }

    /// Get the delimiter of the last heredoc in `input` that hasn't been
    /// replaced with a temp file redirect yet.
    fn get_last_heredoc_delim(input: &str) -> String {
        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();
        let mut i = 0;
        let mut last_delim = String::new();
        while i < len {
            match chars[i] {
                '\'' => { i += 1; while i < len && chars[i] != '\'' { i += 1; } if i < len { i += 1; } }
                '"' => { i += 1; while i < len && chars[i] != '"' { if chars[i] == '\\' { i += 1; } i += 1; } if i < len { i += 1; } }
                '\\' => { i += 2; }
                '<' if i + 1 < len && chars[i + 1] == '<' => {
                    let mut strip = false;
                    let mut j = i + 2;
                    if j < len && chars[j] == '-' { strip = true; j += 1; }
                    while j < len && chars[j].is_ascii_whitespace() { j += 1; }
                    let start = j;
                    while j < len && !chars[j].is_ascii_whitespace() && !matches!(chars[j], '|' | ';' | '&') { j += 1; }
                    let delim: String = chars[start..j].iter().collect();
                    // Skip if already replaced
                    if !input[..start].contains(&format!("<{}", delim)) {
                        if strip {
                            last_delim = format!("-{}", delim);
                        } else {
                            last_delim = delim;
                        }
                    }
                    i = j;
                }
                _ => { i += 1; }
            }
        }
        last_delim
    }

    'repl: loop {
        // Poll background jobs and print completion notices before the prompt.
        jobs.poll_background();

        if interrupted.load(Ordering::SeqCst) {
            interrupted.store(false, Ordering::SeqCst);
            println!();
            continue;
        }

        let line = match read_multi_line(&mut rl, &prompt) {
            Some(l) => l,
            None => {
                let user = utils::get_current_user();
                let cwd_str = utils::get_current_working_dir()
                    .to_string_lossy()
                    .to_string();
                let pid = std::process::id();
                if try_exit(&logger, &user, &mode, &password_hash, &cwd_str, pid) {
                    break;
                }
                continue;
            }
        };

        let raw_input = line.trim();
        if raw_input.is_empty() {
            // Ctrl+C in multi-line mode sends an empty string.
            continue;
        }

        // Apply any pending config reload before processing this command.
        // Read the file directly — no background thread, no race conditions.
        if dynamic_config {
            if let Ok(content) = std::fs::read_to_string(config::CONFIG_PATH) {
                if !content.is_empty() && content != last_config_content {
                    if let Ok(new_cfg) = toml::from_str::<config::Config>(&content) {
                        last_config_content = content;
                        mode = new_cfg.core.mode.clone();
                        protect_paths = new_cfg.paths.protect.clone();
                        allowlist = new_cfg.paths.allowlist.clone();
                        password_hash = new_cfg.auth.password_hash.clone();
                        compiled_rules = new_cfg.rules.iter().filter_map(compile_rule).collect();
                        // Rebuild editor if feature flags changed.
                        let new_flags = FeatureFlags {
                            syntax_highlighting: new_cfg.features.syntax_highlighting,
                            auto_suggest: new_cfg.features.auto_suggest,
                            enhance_completion: new_cfg.features.enhance_completion,
                        };
                        if let Ok(new_rl) = build_editor(&hist_path, &new_flags) {
                            rl = new_rl;
                        }
                        eprintln!("dpshell: config reloaded");
                    }
                }
            }
        }

        if raw_input == "\x0C" {
            print!("\x1b[2J\x1b[H");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            continue;
        }
        if raw_input.is_empty() {
            continue;
        }

        state.push_history(raw_input);
        if bash_compat {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(&hist_path) {
                let _ = writeln!(f, "{}", raw_input);
            }
        }

        // ── Alias expansion ───────────────────────────────────────────────
        let after_alias = parser::expand_vars::expand_alias(raw_input, &state.aliases);

        // ── Heredoc preprocessing ─────────────────────────────────────────
        let heredocs_done = preprocess_heredocs(&after_alias);

        // ── Variable expansion ────────────────────────────────────────────
        let expanded_input = parser::expand_vars::expand_line(&heredocs_done, &state.shell_vars, last_exit);

        // ── Command substitution ───────────────────────────────────────────
        let cmd_substituted =
            expand_command_substitutions(&expanded_input, &mut jobs, &make_ctx!(permissive));

        // ── Parse ──────────────────────────────────────────────────────────
        let node = match parse_command_line(&cmd_substituted) {
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

        let user = utils::get_current_user();
        let cwd = utils::get_current_working_dir();
        let pid = std::process::id();
        let cwd_str = cwd.to_string_lossy().to_string();

        // ── Pre-AST raw-input rule check (catches structural patterns like fork bombs)
        if mode != "disable" {
            if check_raw_input(raw_input, &compiled_rules).is_none() {
                let entry = LogEntry::new(
                    "WARN",
                    &user,
                    &mode,
                    raw_input,
                    &cwd_str,
                    pid,
                    "blocked by rule (raw input match)",
                );
                let _ = logger.write_entry(&entry);
                let _ = logger.flush();
                println!();
                continue;
            }
        }

        // ── Built-in dispatch ─────────────────────────────────────────────
        // Only Simple nodes can be built-ins; compound expressions never are.
        if let CommandNode::Simple(ref sc) = node {
            let program = sc.program.as_str();

            // Special-case builtins that need non-uniform handling
            // (re-enter execution, control flow, job manager, etc.)
            match program {
                "exit" | "logout" => {
                    if try_exit(&logger, &user, &mode, &password_hash, &cwd_str, pid) {
                        break 'repl;
                    }
                    continue;
                }
                "cd" => {
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
                "jobs" => {
                    jobs.builtin_jobs();
                    continue;
                }
                "source" | "." => {
                    match read_source_file(sc.args().first().map(String::as_str).unwrap_or("")) {
                        Ok(lines) => {
                            for line in lines {
                                let line = line.trim().to_string();
                                if line.is_empty() || line.starts_with('#') {
                                    continue;
                                }
                                // Security: raw-input check
                                if mode != "disable" {
                                    if check_raw_input(&line, &compiled_rules).is_none() {
                                        eprintln!(
                                            "dpshell: \x1b[31;5m[!]\x1b[0m source: blocked by rule (raw input match)"
                                        );
                                        continue;
                                    }
                                }
                                // Apply variable expansion and command substitution
                                let line_hd = preprocess_heredocs(&line);
                                let line_exp =
                                    parser::expand_vars::expand_line(&line_hd, &state.shell_vars, last_exit);
                                let line_sub =
                                    expand_command_substitutions(&line_exp, &mut jobs, &make_ctx!(permissive));
                                if let Ok(node) = crate::parser::syntax::parse_command_line(&line_sub) {
                                    // Security: AST-level check
                                    if mode != "disable" {
                                        if let Some(checked) =
                                            apply_rules_to_node(node, &compiled_rules)
                                        {
                                            execute_node(
                                                &checked,
                                                &mut jobs,
                                                &make_ctx!(permissive),
                                            );
                                        } else {
                                            eprintln!(
                                                "dpshell: \x1b[31;5m[!]\x1b[0m source: blocked by rule"
                                            );
                                        }
                                    } else {
                                        execute_node(&node, &mut jobs, &make_ctx!(permissive));
                                    }
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
                    continue;
                }
                "eval" => {
                    let cmd = builtin_eval(sc.args());
                    // Security: raw-input check
                    if mode != "disable" {
                        if check_raw_input(&cmd, &compiled_rules).is_none() {
                            eprintln!(
                                "dpshell: \x1b[31;5m[!]\x1b[0m eval: blocked by rule (raw input match)"
                            );
                            continue;
                        }
                    }
                    if let Ok(node) = crate::parser::syntax::parse_command_line(&cmd) {
                        if mode != "disable" {
                            if let Some(checked) = apply_rules_to_node(node, &compiled_rules) {
                                execute_node(&checked, &mut jobs, &make_ctx!(permissive));
                            } else {
                                eprintln!("dpshell: \x1b[31;5m[!]\x1b[0m eval: blocked by rule");
                            }
                        } else {
                            execute_node(&node, &mut jobs, &make_ctx!(permissive));
                        }
                    }
                    continue;
                }
                "builtin" => {
                    if let Some(rest) = sc.args().first() {
                        let rebuilt = std::iter::once(rest.clone())
                            .chain(sc.args().iter().skip(1).cloned())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if let Ok(node) = crate::parser::syntax::parse_command_line(&rebuilt) {
                            execute_node(&node, &mut jobs, &make_ctx!(permissive));
                        }
                    }
                    continue;
                }
                "command" => {
                    let args = sc.args();
                    if args.first().map(String::as_str) == Some("-v") {
                        for name in &args[1..] {
                            match builtin_command_v(name) {
                                Some(p) => println!("{}", p.display()),
                                None => eprintln!("dpshell: command: {}: not found", name),
                            }
                        }
                    } else if let Some(_name) = args.first() {
                        let rebuilt = args.join(" ");
                        if let Ok(node) = crate::parser::syntax::parse_command_line(&rebuilt) {
                            execute_node(&node, &mut jobs, &make_ctx!(permissive));
                        }
                    }
                    continue;
                }
                "return" => {
                    let code: i32 = sc.args().first().and_then(|s| s.parse().ok()).unwrap_or(0);
                    std::process::exit(code);
                }
                _ => {}
            }

            // Assignment detection (VAR=value)
            if let Some(code) = try_execute_assignments(sc, &mut state) {
                last_exit = code;
                continue;
            }

            // Registry-based dispatch for all standard builtins
            if let Some(reg) = builtin_registry.get(program) {
                last_exit = (reg.execute)(sc.args(), &mut state);
                continue;
            }
        }

        // ── Function definition: store body, do not execute ──────────────
        if let CommandNode::FunctionDef { ref name, ref body } = node {
            state
                .functions
                .borrow_mut()
                .insert(name.clone(), *body.clone());
            continue;
        }

        // ── Mode dispatch ──────────────────────────────────────────────────
        match mode.as_str() {
            "disable" => {
                execute_node(&node, &mut jobs, &make_ctx!(permissive));
                let entry = LogEntry::new(
                    "INFO",
                    &user,
                    &mode,
                    raw_input,
                    &cwd_str,
                    pid,
                    "command executed (disable mode)",
                );
                let _ = logger.write_entry(&entry);
                let _ = logger.flush();
            }

            "permissive" => {
                // Rules — operate on the AST.
                let node = match apply_rules_to_node(node, &compiled_rules) {
                    None => {
                        let entry = LogEntry::new(
                            "WARN",
                            &user,
                            &mode,
                            raw_input,
                            &cwd_str,
                            pid,
                            "blocked by rule",
                        );
                        let _ = logger.write_entry(&entry);
                        let _ = logger.flush();
                        println!();
                        continue;
                    }
                    Some(n) => n,
                };

                // Plugins — receive raw string representation of the (possibly
                // rule-modified) command; re-parse their output.
                let raw_for_plugins = raw_input.to_string(); // original for plugins
                match run_plugins(&loaded_plugins, &raw_for_plugins) {
                    None => {
                        let entry = LogEntry::new(
                            "WARN",
                            &user,
                            &mode,
                            raw_input,
                            &cwd_str,
                            pid,
                            "blocked by plugin",
                        );
                        let _ = logger.write_entry(&entry);
                        let _ = logger.flush();
                    }
                    Some(final_str) => {
                        let final_node = if final_str != raw_for_plugins {
                            match reparse(&final_str) {
                                Some(n) => n,
                                None => {
                                    println!();
                                    continue;
                                }
                            }
                        } else {
                            node
                        };
                        let msg = if final_str != raw_input {
                            format!("replaced to: {}", final_str)
                        } else {
                            "no replacement".to_string()
                        };
                        execute_node(&final_node, &mut jobs, &make_ctx!(permissive));
                        let entry =
                            LogEntry::new("INFO", &user, &mode, raw_input, &cwd_str, pid, &msg);
                        let _ = logger.write_entry(&entry);
                        let _ = logger.flush();
                    }
                }
            }

            "enforcing" => {
                let node = match apply_rules_to_node(node, &compiled_rules) {
                    None => {
                        let entry = LogEntry::new(
                            "WARN",
                            &user,
                            &mode,
                            raw_input,
                            &cwd_str,
                            pid,
                            "blocked by rule",
                        );
                        let _ = logger.write_entry(&entry);
                        let _ = logger.flush();
                        println!();
                        continue;
                    }
                    Some(n) => n,
                };

                match run_plugins(&loaded_plugins, raw_input) {
                    None => {
                        let entry = LogEntry::new(
                            "WARN",
                            &user,
                            &mode,
                            raw_input,
                            &cwd_str,
                            pid,
                            "blocked by plugin",
                        );
                        let _ = logger.write_entry(&entry);
                        let _ = logger.flush();
                    }
                    Some(final_str) => {
                        let final_node = if final_str != raw_input {
                            match reparse(&final_str) {
                                Some(n) => n,
                                None => {
                                    println!();
                                    continue;
                                }
                            }
                        } else {
                            node
                        };

                        // Phase 3: audit the full tree — every leaf is checked.
                        match check_node(&final_node, &protect_paths, &allowlist) {
                            ProtectionResult::Allowed => {
                                let msg = if final_str != raw_input {
                                    format!("replaced to: {}", final_str)
                                } else {
                                    "no replacement".to_string()
                                };
                                let ctx = make_ctx!(enforcing);
                                execute_node(&final_node, &mut jobs, &ctx);
                                let entry = LogEntry::new(
                                    "INFO", &user, &mode, raw_input, &cwd_str, pid, &msg,
                                );
                                let _ = logger.write_entry(&entry);
                                let _ = logger.flush();
                            }
                            ProtectionResult::Blocked(offender) => {
                                println!(
                                    "\x1b[31;5m[!]\x1b[0m Blocked: '{}' targets a \
                                    protected path and is not allowlisted.",
                                    offender
                                );
                                let entry = LogEntry::new(
                                    "WARN",
                                    &user,
                                    &mode,
                                    raw_input,
                                    &cwd_str,
                                    pid,
                                    &format!("blocked: {}", offender),
                                );
                                let _ = logger.write_entry(&entry);
                                let _ = logger.flush();
                            }
                            ProtectionResult::RequiresAuth(offender) => {
                                println!(
                                    "\x1b[31;5m[!]\x1b[0m '{}' requires authorization.",
                                    offender
                                );
                                if authenticate(&password_hash) {
                                    println!("Authorization granted. Executing...");
                                    let ctx = make_ctx!(enforcing);
                                    execute_node(&final_node, &mut jobs, &ctx);
                                    let entry = LogEntry::new(
                                        "INFO",
                                        &user,
                                        &mode,
                                        raw_input,
                                        &cwd_str,
                                        pid,
                                        &format!("auth granted for: {}", offender),
                                    );
                                    let _ = logger.write_entry(&entry);
                                    let _ = logger.flush();
                                } else {
                                    println!("Authorization denied.");
                                    let entry = LogEntry::new(
                                        "WARN",
                                        &user,
                                        &mode,
                                        raw_input,
                                        &cwd_str,
                                        pid,
                                        &format!("auth failed for: {}", offender),
                                    );
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
    unsafe {
        env::set_var("DPSHELL_LEVEL", new_level.to_string());
    }
    let _ = logger.flush();
    Ok(())
}
