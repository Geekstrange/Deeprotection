use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use chrono::Local;
use regex::Regex;
use rustyline::Editor;
use rustyline::error::ReadlineError;
use terminal_size::{Width, terminal_size};
use rand::Rng;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const CONFIG_FILE: &str = "/etc/deeprotection/deeprotection.conf";
const LANG_DIR: &str = "/usr/share/locale/deeprotection";
const LOG_FILE: &str = "/var/log/deeprotection.log";

#[derive(Debug, Clone)]
struct Config {
    language: String,
    mode: String,
    protected_paths: Vec<PathBuf>,
    command_intercept_rules: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct LanguageStrings {
    err_create_directory: String,
    err_create_config: String,
    err_set_mode: String,
    err_use_enhanced: String,
    err_unknown_status: String,
    err_invalid_selection: String,
    war_forbid_path: String,
    war_rmstar_block: String,
    war_rule_match: String,
    war_cmd_original: String,
    war_cmd_replace: String,
    war_skip_unsupported: String,
    war_will_execute: String,
    log_rm_suspect: String,
    log_int_blocked: String,
    msg_back_directory: String,
    msg_exit_recursivecd: String,
    msg_current_directory: String,
    msg_at_rootdirectory: String,
    msg_select_directory: String,
    msg_no_subdirectory: String,
    msg_enter_dpshell: String,
    msg_exit_dpshell: String,
}

impl Default for LanguageStrings {
    fn default() -> Self {
        Self {
            err_create_directory: "Fatal error: Cannot create directory".to_string(),
            err_create_config: "Fatal error: Cannot create config file".to_string(),
            err_set_mode: "Error: Please manually set mode".to_string(),
            err_use_enhanced: "This mode is strictly case-sensitive, please use Enhanced".to_string(),
            err_unknown_status: "Unknown status".to_string(),
            err_invalid_selection: "Invalid selection".to_string(),
            war_forbid_path: "Warning: Operation on protected path forbidden".to_string(),
            war_rmstar_block: "Warning: Dangerous rm operation blocked".to_string(),
            war_rule_match: "Intercepted".to_string(),
            war_cmd_original: "Original command".to_string(),
            war_cmd_replace: "Replace with".to_string(),
            war_skip_unsupported: "Warning: Skip unsupported parameter".to_string(),
            war_will_execute: "About to execute".to_string(),
            log_rm_suspect: "Suspect rm operation".to_string(),
            log_int_blocked: "Rule match blocked".to_string(),
            msg_back_directory: "Back to parent directory".to_string(),
            msg_exit_recursivecd: "Exit recursive mode".to_string(),
            msg_current_directory: "Current directory".to_string(),
            msg_at_rootdirectory: "Already at root directory".to_string(),
            msg_select_directory: "Select directory (enter q to quit)".to_string(),
            msg_no_subdirectory: "No subdirectories in current directory, press any key to exit".to_string(),
            msg_enter_dpshell: "(Enter exit or Ctrl+D to quit)".to_string(),
            msg_exit_dpshell: "Exited".to_string(),
        }
    }
}

struct DeeprotectionShell {
    config: Config,
    lang: LanguageStrings,
    dpshell_level: u32,
    history_path: Option<PathBuf>, // 添加历史文件路径字段
}

impl DeeprotectionShell {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = Config {
            language: String::new(),
            mode: String::new(),
            protected_paths: Vec::new(),
            command_intercept_rules: HashMap::new(),
        };

        // Load configuration
        Self::load_config(&mut config)?;

        // Initialize log directory
        Self::init_log_directory()?;

        // Load language strings
        let lang = Self::load_language(&config.language)?;

        let dpshell_level = env::var("DPSHELL_LEVEL")
            .unwrap_or_else(|_| "0".to_string())
            .parse::<u32>()
            .unwrap_or(0) + 1;

        // Safe to set environment variable in this context
        unsafe {
            env::set_var("DPSHELL_LEVEL", dpshell_level.to_string());
        }

        Ok(Self {
            config,
            lang,
            dpshell_level,
            history_path: None, // 初始化为None
        })
    }

    fn load_config(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(CONFIG_FILE)?;
        let lines: Vec<&str> = content.lines().collect();

        // Parse language and mode
        for line in &lines {
            if line.trim().is_empty() || line.trim().starts_with('#') {
                continue;
            }

            if let Some(pos) = line.find('=') {
                let key = line[..pos].trim();
                let value = line[pos + 1..].trim();

                // Remove comments from value
                let value = if let Some(comment_pos) = value.find('#') {
                    value[..comment_pos].trim()
                } else {
                    value
                };

                // Remove quotes if present
                let value = if value.starts_with('"') && value.ends_with('"') {
                    &value[1..value.len() - 1]
                } else {
                    value
                };

                match key {
                    "language" => config.language = value.to_string(),
                    "mode" => config.mode = value.to_string(),
                    _ => {}
                }
            }
        }

        // Load protected paths
        let mut in_protected_section = false;
        for line in &lines {
            if line.trim() == "#protected_paths_list" {
                in_protected_section = true;
                continue;
            } else if line.trim().starts_with('#') && in_protected_section {
                break;
            }

            if in_protected_section && !line.trim().is_empty() && !line.trim().starts_with('#') {
                let path = PathBuf::from(line.trim());
                if let Ok(canonical) = path.canonicalize() {
                    config.protected_paths.push(canonical);
                } else {
                    // If canonicalize fails, try to resolve manually
                    config.protected_paths.push(Self::resolve_path(&path));
                }
            }
        }

        // Load command intercept rules
        let mut in_command_section = false;
        for line in &lines {
            if line.trim() == "#command_intercept_rules" {
                in_command_section = true;
                continue;
            } else if line.trim().starts_with('#') && in_command_section {
                break;
            }

            if in_command_section && !line.trim().is_empty() && !line.trim().starts_with('#') {
                if let Some(pos) = line.find('>') {
                    let original = line[..pos].trim().to_string();
                    let replaced = line[pos + 1..].trim().to_string();
                    config.command_intercept_rules.insert(original, replaced);
                }
            }
        }

        // Set default language if not specified
        if config.language.is_empty() {
            config.language = env::var("LANG")
                .unwrap_or_else(|_| "en_US".to_string())
                .split('.')
                .next()
                .unwrap_or("en_US")
                .to_string();
        }

        Ok(())
    }

    fn resolve_path(path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::Normal(name) => components.push(name.to_os_string()),
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::RootDir => {
                    components.clear();
                }
                _ => {}
            }
        }

        let mut result = PathBuf::from("/");
        for component in components {
            result.push(component);
        }
        result
    }

    fn init_log_directory() -> Result<(), Box<dyn std::error::Error>> {
        let log_dir = Path::new(LOG_FILE).parent().unwrap();
        if !log_dir.exists() {
            fs::create_dir_all(log_dir)?;
        }

        if !Path::new(LOG_FILE).exists() {
            File::create(LOG_FILE)?;
        }

        Ok(())
    }

    fn load_language(lang_code: &str) -> Result<LanguageStrings, Box<dyn std::error::Error>> {
        let lang_file = format!("{}/{}.ftl", LANG_DIR, lang_code);
        let default_file = format!("{}/en_US.ftl", LANG_DIR);

        let file_to_load = if Path::new(&lang_file).exists() {
            lang_file
        } else if Path::new(&default_file).exists() {
            println!("\x1b[32mUsing default language.\x1b[0m");
            default_file
        } else {
            eprintln!("\x1b[31mError: No language files found\x1b[0m");
            return Ok(LanguageStrings::default());
        };

        let mut lang = LanguageStrings::default();
        let content = fs::read_to_string(&file_to_load)?;

        for line in content.lines() {
            if line.trim().is_empty() || line.trim().starts_with('#') {
                continue;
            }

            if let Some(pos) = line.find('=') {
                let key = line[..pos].trim();
                let mut value = line[pos + 1..].trim();

                // Remove quotes if present
                if value.starts_with('"') && value.ends_with('"') {
                    value = &value[1..value.len() - 1];
                }

                // Handle escape sequences
                let value = value
                    .replace("\\\"", "\"")
                    .replace("\\\\", "\\")
                    .replace("\\n", "\n");

                match key {
                    "err_create_directory" => lang.err_create_directory = value,
                    "err_create_config" => lang.err_create_config = value,
                    "err_set_mode" => lang.err_set_mode = value,
                    "err_use_enhanced" => lang.err_use_enhanced = value,
                    "err_unknown_status" => lang.err_unknown_status = value,
                    "err_invalid_selection" => lang.err_invalid_selection = value,
                    "war_forbid_path" => lang.war_forbid_path = value,
                    "war_rmstar_block" => lang.war_rmstar_block = value,
                    "war_rule_match" => lang.war_rule_match = value,
                    "war_cmd_original" => lang.war_cmd_original = value,
                    "war_cmd_replace" => lang.war_cmd_replace = value,
                    "war_skip_unsupported" => lang.war_skip_unsupported = value,
                    "war_will_execute" => lang.war_will_execute = value,
                    "log_rm_suspect" => lang.log_rm_suspect = value,
                    "log_int_blocked" => lang.log_int_blocked = value,
                    "msg_back_directory" => lang.msg_back_directory = value,
                    "msg_exit_recursivecd" => lang.msg_exit_recursivecd = value,
                    "msg_current_directory" => lang.msg_current_directory = value,
                    "msg_at_rootdirectory" => lang.msg_at_rootdirectory = value,
                    "msg_select_directory" => lang.msg_select_directory = value,
                    "msg_no_subdirectory" => lang.msg_no_subdirectory = value,
                    "msg_enter_dpshell" => lang.msg_enter_dpshell = value,
                    "msg_exit_dpshell" => lang.msg_exit_dpshell = value,
                    _ => {}
                }
            }
        }

        Ok(lang)
    }

    fn output_log(&self, command: &str, additional_info: Option<&str>) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());

        // Resolve current directory separately to avoid temporary value drop
        let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let path = current_dir.display();

        let pid = std::process::id();

        let log_entry = format!(
            "{} | user: {} | command: {} | path: {} | current_pid: {} | exit_code: 0{}\n",
            timestamp, user, command, path, pid,
            additional_info.map(|info| format!(" | {}", info)).unwrap_or_default()
        );

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
            let _ = file.write_all(log_entry.as_bytes());
        }
    }

    fn check_protected_paths(&self, args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
        for arg in args {
            if arg.starts_with('-') {
                continue;
            }

            let target_path = Self::resolve_path(&PathBuf::from(arg));

            for protected in &self.config.protected_paths {
                if target_path.starts_with(protected) {
                    self.output_log(&args.join(" "), None);
                    println!("\x07\x1b[5;31m[!]\x1b[0m {} {}", self.lang.war_forbid_path, protected.display());
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn check_dangerous_rm_patterns(&self, args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
        if args.is_empty() || args[0] != "rm" {
            return Ok(true);
        }

        let danger_patterns = vec![
            "/", "/bin", "/etc", "/home", "/usr", "/root", "/lib", "/sbin", "/var", "/sys", "/proc", "/dev"
        ];

        for arg in &args[1..] {
            if arg.starts_with('-') {
                continue;
            }

            let resolved_arg = Self::resolve_path(&PathBuf::from(arg));
            let resolved_str = resolved_arg.to_string_lossy();
            let resolved_str = resolved_str.trim_end_matches('/');

            for pattern in &danger_patterns {
                if resolved_str == *pattern || resolved_str == &format!("{}/", pattern) {
                    println!("\x07\x1b[5;31m[!]\x1b[0m {}", self.lang.war_rmstar_block);
                    self.output_log(&args.join(" "), Some(&self.lang.log_rm_suspect));
                    return Ok(false);
                }
            }

            // Check for root directory wildcards
            let root_wildcard_re = Regex::new(r"^/(\*|\.\*)$").unwrap();
            if root_wildcard_re.is_match(resolved_str) {
                println!("\x07\x1b[5;31m[!]\x1b[0m {}", self.lang.war_rmstar_block);
                self.output_log(&args.join(" "), Some(&self.lang.log_rm_suspect));
                return Ok(false);
            }
        }

        // Check for wildcard expansion in current directory
        let current_dir = env::current_dir()?;
        let current_files: Vec<_> = fs::read_dir(&current_dir)?
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    e.file_name().to_str().map(|s| s.to_string())
                })
            })
            .collect();

        let mut files = Vec::new();
        for arg in &args[1..] {
            if !arg.starts_with('-') {
                files.push(arg.clone());
            }
        }

        if !files.is_empty() {
            let mut sorted_args = files.clone();
            let mut sorted_files = current_files.clone();
            sorted_args.sort();
            sorted_files.sort();

            if sorted_args == sorted_files {
                println!("\x07\x1b[5;31m[!]\x1b[0m {}", self.lang.war_rmstar_block);
                self.output_log(&args.join(" "), Some(&self.lang.log_rm_suspect));
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn check_command_intercept_rules(&self, args: &[String]) -> Result<Option<Vec<String>>, Box<dyn std::error::Error>> {
        let full_cmd = args.join(" ");
        let mut best_match = String::new();
        let mut best_match_length = 0;

        // Iterate over keys only to avoid unused variable warning
        for pattern in self.config.command_intercept_rules.keys() {
            let escaped_pattern = regex::escape(pattern).replace(r"\s+", r"\s*");
            let regex_pattern = format!(r"^\s*{}\s*$", escaped_pattern);

            if let Ok(re) = Regex::new(&regex_pattern) {
                if re.is_match(&full_cmd) && pattern.len() > best_match_length {
                    best_match = pattern.clone();
                    best_match_length = pattern.len();
                }
            }
        }

        if !best_match.is_empty() {
            if let Some(replacement) = self.config.command_intercept_rules.get(&best_match) {
                if replacement.is_empty() {
                    println!("\x07\x1b[5;31m[!]\x1b[0m {} {}", self.lang.war_rule_match, full_cmd);
                    self.output_log(&format!("[{}] {}", self.lang.log_int_blocked, full_cmd), None);
                    return Ok(None);
                } else {
                    println!("\x07\x1b[5;31m[!]\x1b[0m {}: {} -> {}: {}",
                        self.lang.war_cmd_original, full_cmd, self.lang.war_cmd_replace, replacement);
                    let new_args: Vec<String> = replacement.split_whitespace().map(|s| s.to_string()).collect();
                    self.output_log(&new_args.join(" "), None);
                    return Ok(Some(new_args));
                }
            }
        }

        Ok(Some(args.to_vec()))
    }

    fn execute_command(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        if args.is_empty() {
            return Ok(0);
        }

        let cmd = &args[0];
        let cmd_args = &args[1..];

        match cmd.as_str() {
            "rm" => self.execute_rm(cmd_args),
            "ls" => self.execute_ls(cmd_args, false, false),
            "la" => self.execute_ls(cmd_args, true, false),
            "ll" => self.execute_ls(cmd_args, false, true),
            "cd" => self.execute_cd(cmd_args),
            "history" => self.execute_history(), // 添加history命令处理
            _ => {
                // Check if command exists
                if let Ok(output) = Command::new("which").arg(cmd).output() {
                    if output.status.success() {
                        let status = Command::new(cmd)
                            .args(cmd_args)
                            .status()?;
                        Ok(status.code().unwrap_or(0))
                    } else {
                        println!("dpshell: {}: command not found", args.join(" "));
                        Ok(127)
                    }
                } else {
                    println!("dpshell: {}: command not found", args.join(" "));
                    Ok(127)
                }
            }
        }
    }

    // 添加history命令执行函数
    fn execute_history(&self) -> Result<i32, Box<dyn std::error::Error>> {
        if let Some(hist_path) = &self.history_path {
            if hist_path.exists() {
                let mut file = File::open(hist_path)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;

                // 为历史记录添加行号
                for (i, line) in contents.lines().enumerate() {
                    println!("{:4}  {}", i + 1, line);
                }
                Ok(0)
            } else {
                eprintln!("dpshell: history: history file not found");
                Ok(1)
            }
        } else {
            eprintln!("dpshell: history: not available in non-interactive mode");
            Ok(1)
        }
    }

    fn execute_rm(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        let mut recursive_flag = false;
        let mut files = Vec::new();

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-r" | "-R" | "--recursive" => recursive_flag = true,
                arg if arg.starts_with("-rf") => {
                    recursive_flag = true;
                }
                "--" => {
                    files.extend_from_slice(&args[i + 1..]);
                    break;
                }
                arg if arg.starts_with('-') => {
                    println!("{} '{}'", self.lang.war_skip_unsupported, arg);
                }
                _ => files.push(args[i].clone()),
            }
            i += 1;
        }

        let mut rm_args = vec!["-i".to_string(), "-v".to_string()];
        if recursive_flag {
            rm_args.push("-r".to_string());
        }
        rm_args.extend(files);

        println!("\x1b[5;33m<!>\x1b[0m {}: /bin/rm {}", self.lang.war_will_execute, rm_args.join(" "));

        let status = Command::new("/bin/rm")
            .args(&rm_args)
            .status()?;

        Ok(status.code().unwrap_or(0))
    }

    fn execute_ls(&self, args: &[String], all: bool, long: bool) -> Result<i32, Box<dyn std::error::Error>> {
        let mut ls_args = vec!["--color=auto".to_string()];
        if all {
            ls_args.push("-a".to_string());
        }
        if long {
            ls_args.push("-l".to_string());
        }
        ls_args.extend_from_slice(args);

        let status = Command::new("/bin/ls")
            .args(&ls_args)
            .status()?;

        Ok(status.code().unwrap_or(0))
    }

    fn execute_cd(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        match args.len() {
            0 => {
                let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
                env::set_current_dir(&home)?;
                Ok(0)
            }
            1 => {
                match args[0].as_str() {
                    "?" => self.handle_cd_interactive(),
                    "??" => self.handle_cd_recursive(),
                    path => {
                        env::set_current_dir(path)?;
                        println!("{}", env::current_dir()?.display());
                        Ok(0)
                    }
                }
            }
            _ => {
                eprintln!("cd: too many arguments");
                Ok(1)
            }
        }
    }

    fn handle_cd_interactive(&self) -> Result<i32, Box<dyn std::error::Error>> {
        let current_dir = env::current_dir()?;
        let mut subdirs = Vec::new();

        for entry in fs::read_dir(&current_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !name.starts_with('.') {
                        subdirs.push(name.to_string());
                    }
                }
            }
        }

        if subdirs.is_empty() {
            println!("{}", self.lang.msg_no_subdirectory);
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            return Ok(0);
        }

        for (i, dir) in subdirs.iter().enumerate() {
            println!("{}) {}", i + 1, dir);
        }

        loop {
            print!("{}: ", self.lang.msg_select_directory);
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            match input {
                "q" | "Q" => break,
                _ => {
                    if let Ok(choice) = input.parse::<usize>() {
                        if choice >= 1 && choice <= subdirs.len() {
                            let target_dir = current_dir.join(&subdirs[choice - 1]);
                            env::set_current_dir(&target_dir)?;
                            println!("{}", env::current_dir()?.display());
                            break;
                        }
                    }
                    println!("{}", self.lang.err_invalid_selection);
                }
            }
        }

        Ok(0)
    }

    fn handle_cd_recursive(&self) -> Result<i32, Box<dyn std::error::Error>> {
        self.recursive_cd_selector(&env::current_dir()?)
    }

    fn recursive_cd_selector(&self, current_dir: &Path) -> Result<i32, Box<dyn std::error::Error>> {
        let mut options = Vec::new();

        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    options.push(path.clone());
                    println!("{}) {}", options.len(), name);
                }
            }
        }

        if current_dir != Path::new("/") {
            println!("l] {}", self.lang.msg_back_directory);
        }
        println!("q] {}", self.lang.msg_exit_recursivecd);

        loop {
            print!("{}: {} > ", self.lang.msg_current_directory, current_dir.display());
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            match input {
                "q" | "Q" => return Ok(0),
                "l" | "L" => {
                    if current_dir != Path::new("/") {
                        if let Some(parent) = current_dir.parent() {
                            env::set_current_dir(parent)?;
                            return self.recursive_cd_selector(&env::current_dir()?);
                        }
                    } else {
                        println!("{}", self.lang.msg_at_rootdirectory);
                    }
                }
                _ => {
                    if let Ok(choice) = input.parse::<usize>() {
                        if choice >= 1 && choice <= options.len() {
                            let selected_dir = &options[choice - 1];
                            env::set_current_dir(selected_dir)?;
                            return self.recursive_cd_selector(&env::current_dir()?);
                        }
                    }
                    println!("{}", self.lang.err_invalid_selection);
                }
            }
        }
    }

    fn command_pipeline(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        // 1. Check protected paths
        if !self.check_protected_paths(args)? {
            return Ok(1);
        }

        // 2. Check command intercept rules
        let processed_args = match self.check_command_intercept_rules(args)? {
            Some(args) => args,
            None => return Ok(1), // Command was blocked
        };

        // 3. Check dangerous rm patterns
        if !self.check_dangerous_rm_patterns(&processed_args)? {
            return Ok(1);
        }

        // 4. Execute command
        if self.config.mode.to_lowercase() == "permissive" {
            // In permissive mode, skip rm replacement module
            self.execute_command(&processed_args)
        } else {
            // In enhanced mode, use rm replacement
            self.execute_command(&processed_args)
        }
    }

    fn start_animation(&self) {
        let str_text = "dpshell>";
        let cols = if let Some((Width(w), _)) = terminal_size() {
            w as usize
        } else {
            80
        };
        let len = str_text.len();

        print!("\x1b[?25l"); // Hide cursor
        io::stdout().flush().unwrap();

        for i in 0..=(cols.saturating_sub(len)) {
            print!("\r{:width$}{}", "", str_text, width = i);
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(1));
        }

        print!("\r\x1b[32m{:width$}{}\x1b[0m\n", "", str_text, width = cols.saturating_sub(len));
        print!("\x1b[?25h"); // Show cursor
        io::stdout().flush().unwrap();
    }

    fn get_prompt(&self) -> String {
        let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
        let prompt_char = if user == "root" { "#" } else { "$" };
        format!("dpshell({}){} ", self.dpshell_level, prompt_char)
    }

    fn run_interactive_shell(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.start_animation();
        println!("{}", self.lang.msg_enter_dpshell);

        // 创建随机名称的临时历史文件
        let mut rng = rand::thread_rng();
        let rand_num: u32 = rng.gen_range(0..0xFFFFFF); // 使用 gen_range 替代 gen
        let hist_path = format!("/tmp/dpshell_history.{:06X}", rand_num);
        let hist_path = PathBuf::from(&hist_path);

        // 创建并设置历史文件
        File::create(&hist_path)?;
        self.setup_history(&hist_path)?;

        // 保存历史文件路径到结构体
        self.history_path = Some(hist_path.clone());

        // 创建信号处理标志
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_clone = Arc::clone(&interrupted);

        // 设置信号处理
        ctrlc::set_handler(move || {
            interrupted_clone.store(true, Ordering::SeqCst);
        })?;

        // 创建 rustyline 编辑器
        let mut rl: rustyline::Editor<(), rustyline::history::FileHistory> = Editor::new()?;
        rl.load_history(&hist_path).ok(); // 加载历史文件

        let prompt = self.get_prompt();

        loop {
            if interrupted.load(Ordering::SeqCst) {
                // 重置中断标志
                interrupted.store(false, Ordering::SeqCst);
                println!(); // 新行
                continue; // 跳过当前迭代
            }

            let readline = rl.readline(&prompt);
            match readline {
                Ok(line) => {
                    let line = line.trim();

                    // 处理特殊控制字符
                    if line == "\x0C" { // Ctrl+L - clear screen
                        print!("\x1b[2J\x1b[H");
                        io::stdout().flush()?;
                        continue;
                    }

                    if line.is_empty() {
                        continue;
                    }

                    if line == "exit" {
                        break;
                    }

                    // 解析命令
                    let args: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();

                    // 处理 SIGINT 陷阱
                    unsafe {
                        libc::signal(libc::SIGINT, libc::SIG_DFL);
                    }

                    // 执行命令
                    let _ = self.check_mode_module(&args);

                    // 恢复 SIGINT 陷阱
                    unsafe {
                        libc::signal(libc::SIGINT, libc::SIG_IGN);
                    }

                    // 确保历史记录被保存
                    let _ = rl.add_history_entry(line);
                    rl.save_history(&hist_path).ok();
                }
                Err(ReadlineError::Interrupted) => {
                    // Ctrl+C - 我们已经设置了全局处理程序
                    println!(); // 新行
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl+D
                    break;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    break;
                }
            }
        }

        println!("\x1b[32m{}\x1b[0m {}", self.lang.msg_exit_dpshell,
                 Local::now().format("%Y-%m-%d %H:%M:%S"));

        // 清理：删除临时历史文件
        if let Err(e) = std::fs::remove_file(&hist_path) {
            eprintln!("Error removing history file: {}", e);
        }

        // 重置历史文件路径
        self.history_path = None;

        // 递减 DPSHELL_LEVEL
        let new_level = self.dpshell_level.saturating_sub(1);
        unsafe {
            env::set_var("DPSHELL_LEVEL", new_level.to_string());
        }

        Ok(())
    }

    fn setup_history(&self, hist_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // 设置环境变量
        unsafe {
            env::set_var("HISTFILE", hist_path.to_string_lossy().as_ref());
            env::set_var("HISTSIZE", "1000");
            env::set_var("HISTFILESIZE", "2000");
            env::set_var("HISTCONTROL", "ignoredups");
        }
        Ok(())
    }

    fn check_mode_module(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        if self.config.mode.is_empty() {
            eprintln!("{}", self.lang.err_set_mode);
            return Ok(1);
        }

        match self.config.mode.to_lowercase().as_str() {
            "enhanced" => {
                if self.config.mode == "Enhanced" {
                    self.command_pipeline(args)
                } else {
                    println!("{}", self.lang.err_use_enhanced);
                    Ok(1)
                }
            }
            "permissive" => {
                self.command_intercept_module(args)
            }
            _ => {
                println!("{}: {}", self.lang.err_unknown_status, self.config.mode);
                Ok(2)
            }
        }
    }

    fn command_intercept_module(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        // Check command intercept rules
        let processed_args = match self.check_command_intercept_rules(args)? {
            Some(args) => args,
            None => return Ok(1), // Command was blocked
        };

        // Check dangerous rm patterns and execute
        if !self.check_dangerous_rm_patterns(&processed_args)? {
            return Ok(1);
        }

        self.execute_command(&processed_args)
    }

    fn run_command(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        self.check_mode_module(args)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let mut shell = DeeprotectionShell::new()?;

    if args.len() < 2 {
        // Run interactive shell
        shell.run_interactive_shell()?;
    } else {
        // Run single command
        let command_args = &args[1..];
        let exit_code = shell.run_command(command_args)?;
        std::process::exit(exit_code);
    }

    Ok(())
}
