use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, exit};
use regex::Regex;

const CONFIG_FILE: &str = "/etc/deeprotection/deeprotection.conf";
const LANGUAGE_PATH: &str = "/usr/share/locale/deeprotection/";

#[derive(Debug, Clone)]
struct Config {
    language: String,
    disable: String,
    expire_hours: String,
    timestamp: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            language: String::new(),
            disable: String::new(),
            expire_hours: String::new(),
            timestamp: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct LocalizedStrings {
    name: String,
    greet: String,
    msg_select_language: String,
    err_no_lang_files: String,
    msg_lang_support: String,
    msg_select_option: String,
    err_invalid_selection: String,
    msg_confirm_lang: String,
    err_no_config: String,
    err_create_directory: String,
    err_create_config: String,
    msg_init_complete: String,
    err_expire_hours: String,
    msg_remain_time: String,
    ask_enable_now: String,
    msg_skip_this: String,
    msg_invalid_input: String,
    msg_temporary_disable: String,
}

impl Default for LocalizedStrings {
    fn default() -> Self {
        LocalizedStrings {
            name: String::new(),
            greet: String::new(),
            msg_select_language: "Available languages".to_string(),
            err_no_lang_files: "Error: No language files found".to_string(),
            msg_lang_support: "Appreciate if you could provide language support".to_string(),
            msg_select_option: "Select option".to_string(),
            err_invalid_selection: "Invalid selection".to_string(),
            msg_confirm_lang: "Is".to_string(),
            err_no_config: "Error: No valid configuration found".to_string(),
            err_create_directory: "Fatal error: Unable to create directory".to_string(),
            err_create_config: "Fatal error: Unable to create configuration file".to_string(),
            msg_init_complete: "Configuration initialization complete, please add rules".to_string(),
            err_expire_hours: "Error: Invalid expire_hours format".to_string(),
            msg_remain_time: "Remaining disable time".to_string(),
            ask_enable_now: "Do you want to enable the feature now".to_string(),
            msg_skip_this: "Skip this time".to_string(),
            msg_invalid_input: "Invalid input".to_string(),
            msg_temporary_disable: "Temporarily disabled, valid for".to_string(),
        }
    }
}

struct DpLauncher {
    config: Config,
    strings: LocalizedStrings,
}

impl DpLauncher {
    fn new() -> Self {
        DpLauncher {
            config: Config::default(),
            strings: LocalizedStrings::default(),
        }
    }

    fn parse_ftl(&mut self, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(file_path)?;
        let key_value_regex = Regex::new(r#"^([[:alnum:]_]+)\s*=\s*"(.*)"$"#)?;

        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(captures) = key_value_regex.captures(line) {
                let key = captures.get(1).unwrap().as_str();
                let mut value = captures.get(2).unwrap().as_str().to_string();

                // Replace escape characters
                value = value.replace("\\\"", "\"");
                value = value.replace("\\n", "\n");

                // Set the appropriate field based on key
                match key {
                    "name" => self.strings.name = value,
                    "greet" => self.strings.greet = value,
                    "msg_select_language" => self.strings.msg_select_language = value,
                    "err_no_lang_files" => self.strings.err_no_lang_files = value,
                    "msg_lang_support" => self.strings.msg_lang_support = value,
                    "msg_select_option" => self.strings.msg_select_option = value,
                    "err_invalid_selection" => self.strings.err_invalid_selection = value,
                    "msg_confirm_lang" => self.strings.msg_confirm_lang = value,
                    "err_no_config" => self.strings.err_no_config = value,
                    "err_create_directory" => self.strings.err_create_directory = value,
                    "err_create_config" => self.strings.err_create_config = value,
                    "msg_init_complete" => self.strings.msg_init_complete = value,
                    "err_expire_hours" => self.strings.err_expire_hours = value,
                    "msg_remain_time" => self.strings.msg_remain_time = value,
                    "ask_enable_now" => self.strings.ask_enable_now = value,
                    "msg_skip_this" => self.strings.msg_skip_this = value,
                    "msg_invalid_input" => self.strings.msg_invalid_input = value,
                    "msg_temporary_disable" => self.strings.msg_temporary_disable = value,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn call_dploader(&self) {
        if let Ok(output) = Command::new("which").arg("dploader").output() {
            if output.status.success() {
                let dploader_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let _ = Command::new(dploader_path).spawn();
            }
        }
    }

    fn get_config_value(&mut self, key: &str) -> String {
        if let Ok(content) = fs::read_to_string(CONFIG_FILE) {
            let regex_pattern = format!(r"^\s*{}\s*=([^#]*)", key);
            if let Ok(regex) = Regex::new(&regex_pattern) {
                for line in content.lines() {
                    if let Some(captures) = regex.captures(line) {
                        let value = captures.get(1).unwrap().as_str();
                        return value.trim().to_string();
                    }
                }
            }
        }
        String::new()
    }

    fn update_config_value(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(CONFIG_FILE)?;
        let regex_pattern = format!(r"^\s*{}\s*=.*", key);
        let regex = Regex::new(&regex_pattern)?;
        let replacement = format!("{}={}", key, value);

        let mut found = false;
        let new_content = content.lines()
            .map(|line| {
                if regex.is_match(line) {
                    found = true;
                    replacement.clone()
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !found {
            fs::write(CONFIG_FILE, format!("{}\n{}", new_content, replacement))?;
        } else {
            fs::write(CONFIG_FILE, new_content)?;
        }

        Ok(())
    }

    fn get_language_files(&self) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
        let mut lang_files = Vec::new();

        if let Ok(entries) = fs::read_dir(LANGUAGE_PATH) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if let Some(extension) = path.extension() {
                        if extension == "ftl" {
                            if let Some(stem) = path.file_stem() {
                                let lang_code = stem.to_string_lossy().to_string();

                                // Try to extract language name from file
                                let mut lang_name = lang_code.clone();
                                if let Ok(content) = fs::read_to_string(&path) {
                                    let name_regex = Regex::new(r#"^name\s*=\s*"(.*)""#).unwrap();
                                    for line in content.lines() {
                                        if let Some(captures) = name_regex.captures(line.trim()) {
                                            lang_name = captures.get(1).unwrap().as_str().to_string();
                                            break;
                                        }
                                    }
                                }

                                lang_files.push((lang_code, lang_name));
                            }
                        }
                    }
                }
            }
        }

        Ok(lang_files)
    }

    fn manual_language_setup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n{}:", self.strings.msg_select_language);

        let lang_files = self.get_language_files()?;
        if lang_files.is_empty() {
            eprintln!("{}", self.strings.err_no_lang_files);
            exit(1);
        }

        // Display language list
        for (i, (_, lang_name)) in lang_files.iter().enumerate() {
            println!("{:2}) {}", i + 1, lang_name);
        }

        println!("\n{}", self.strings.msg_lang_support);

        loop {
            print!("\n{} (1-{}): ", self.strings.msg_select_option, lang_files.len());
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if let Ok(choice) = input.trim().parse::<usize>() {
                if choice >= 1 && choice <= lang_files.len() {
                    let selected_code = &lang_files[choice - 1].0;
                    let selected_file = format!("{}/{}.ftl", LANGUAGE_PATH, selected_code);

                    if Path::new(&selected_file).exists() {
                        self.update_config_value("language", selected_code)?;
                        self.parse_ftl(&selected_file)?;
                        println!("\n{}{}", self.strings.greet, self.strings.name);
                        break;
                    }
                }
            }

            println!("{}", self.strings.err_invalid_selection);
        }

        Ok(())
    }

    fn get_current_language(&self) -> String {
        env::var("LC_ALL")
            .or_else(|_| env::var("LANG"))
            .unwrap_or_else(|_| "en_US".to_string())
            .split('.')
            .next()
            .unwrap_or("en_US")
            .split('_')
            .next()
            .unwrap_or("en")
            .to_string()
    }

    fn check_language(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.config.language = self.get_config_value("language");

        if self.config.language.is_empty() {
            let current_lang = self.get_current_language();
            let lang_files = self.get_language_files()?;

            // Find best match
            let mut best_match = None;
            for (lang_code, lang_name) in &lang_files {
                if lang_code == &current_lang {
                    best_match = Some((lang_code.clone(), lang_name.clone()));
                    break;
                }
                if lang_code.starts_with(&format!("{}_", current_lang)) && best_match.is_none() {
                    best_match = Some((lang_code.clone(), lang_name.clone()));
                }
            }

            if let Some((code, name)) = best_match {
                print!("{} {}? (\x1b[32my\x1b[0m)es/(\x1b[31mn\x1b[0m)o:\n",
                       self.strings.msg_confirm_lang, name);
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase().starts_with('y') {
                    self.update_config_value("language", &code)?;
                    let lang_file = format!("{}/{}.ftl", LANGUAGE_PATH, code);
                    self.parse_ftl(&lang_file)?;
                    println!("{}{}", self.strings.greet, self.strings.name);
                } else {
                    self.manual_language_setup()?;
                }
            } else {
                self.manual_language_setup()?;
            }
        } else {
            // Load configured language
            let lang_file = format!("{}/{}.ftl", LANGUAGE_PATH, self.config.language);
            if Path::new(&lang_file).exists() {
                self.parse_ftl(&lang_file)?;
            } else {
                // Try without .ftl extension
                let lang_file = format!("{}/{}", LANGUAGE_PATH, self.config.language);
                if Path::new(&lang_file).exists() {
                    self.parse_ftl(&lang_file)?;
                } else {
                    eprintln!("{}", self.strings.err_no_config);
                    self.manual_language_setup()?;
                }
            }
        }

        Ok(())
    }

    fn check_config(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_dir = Path::new(CONFIG_FILE).parent().unwrap();

        if !config_dir.exists() {
            if let Err(_) = fs::create_dir_all(config_dir) {
                eprintln!("{} {}", self.strings.err_create_directory, config_dir.display());
                exit(1);
            }

            if let Err(_) = fs::write(CONFIG_FILE, "") {
                eprintln!("{} {}", self.strings.err_create_config, CONFIG_FILE);
                exit(1);
            }

            println!("{}: {}", self.strings.msg_init_complete, CONFIG_FILE);
        }

        Ok(())
    }

    fn self_starting_manager(&self, action: &str) -> Result<(), Box<dyn std::error::Error>> {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let shell_name = Path::new(&shell).file_name().unwrap().to_string_lossy();
        let config_file = format!("{}/.{}rc", env::var("HOME")?, shell_name);

        let dplauncher_path = if let Ok(output) = Command::new("which").arg("dplauncher").output() {
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };

        match action {
            "add" => {
                if let Ok(content) = fs::read_to_string(&config_file) {
                    if !content.contains(&dplauncher_path) {
                        fs::write(&config_file, format!("{}\n{}", content, dplauncher_path))?;
                    }
                }
            }
            "remove" => {
                if let Ok(content) = fs::read_to_string(&config_file) {
                    let new_content = content.lines()
                        .filter(|line| !line.contains(&dplauncher_path))
                        .collect::<Vec<_>>()
                        .join("\n");
                    fs::write(&config_file, new_content)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn check_expire(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.config.disable = self.get_config_value("disable").to_lowercase();

        if self.config.disable == "true" {
            self.self_starting_manager("remove")?;
            exit(0);
        } else {
            self.self_starting_manager("add")?;
        }

        self.config.expire_hours = self.get_config_value("expire_hours");
        self.config.timestamp = self.get_config_value("timestamp");

        // Validate expire_hours format
        let expire_hours_regex = Regex::new(r"^[0-9]+(\.[0-9]+)?$")?;
        if !expire_hours_regex.is_match(&self.config.expire_hours) {
            eprintln!(" {}", self.strings.err_expire_hours);
            exit(1);
        }

        // Check if timestamp is valid
        if let Ok(timestamp) = self.config.timestamp.parse::<i64>() {
            let current_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64;

            let expire_hours: f64 = self.config.expire_hours.parse()?;
            let expire_seconds = (expire_hours * 3600.0) as i64;
            let expire_ts = timestamp + expire_seconds;

            if current_ts < expire_ts {
                let remain = expire_ts - current_ts;
                let hours = remain / 3600;
                let minutes = (remain % 3600) / 60;

                println!(" {}: {:02}h {:02}min", self.strings.msg_remain_time, hours, minutes);
                exit(0);
            }
        }

        Ok(())
    }

    fn user_interaction(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            print!("{}? (\x1b[32my\x1b[0m)es/(\x1b[31mn\x1b[0m)o/(\x1b[33ms\x1b[0m)kip:\n",
                   self.strings.ask_enable_now);
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let choice = input.trim().to_lowercase();

            match choice.as_str() {
                "y" => {
                    self.update_config_value("timestamp", "0")?;
                    self.call_dploader();
                    exit(0);
                }
                "n" => {
                    let current_ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs();
                    self.update_config_value("timestamp", &current_ts.to_string())?;
                    println!("{} {} h", self.strings.msg_temporary_disable, self.config.expire_hours);
                    exit(0);
                }
                "s" => {
                    println!("{}", self.strings.msg_skip_this);
                    exit(0);
                }
                _ => {
                    println!("{}", self.strings.msg_invalid_input);
                }
            }
        }
    }

    fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.check_language()?;
        self.check_config()?;
        self.check_expire()?;
        self.user_interaction()?;
        Ok(())
    }
}

fn main() {
    let mut launcher = DpLauncher::new();
    if let Err(e) = launcher.run() {
        eprintln!("Error: {}", e);
        exit(1);
    }
}
