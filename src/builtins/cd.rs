// cd.rs - ARCHITECTURE.md §3.6 Interactive cd Navigation
// Retained verbatim from original design; see Refactored_Plan.md §6.
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

// Hardcoded English strings (i18n removed per architectural simplification)
const MSG_NO_SUBDIRECTORY: &str = "No subdirectories in current directory, press any key to exit";
const MSG_SELECT_DIRECTORY: &str = "Select directory (enter q to quit)";
const MSG_BACK_DIRECTORY: &str = "Back to parent directory";
const MSG_EXIT_RECURSIVECD: &str = "Exit recursive mode";
const MSG_CURRENT_DIRECTORY: &str = "Current directory";
const MSG_AT_ROOTDIRECTORY: &str = "Already at root directory";
const ERR_INVALID_SELECTION: &str = "Invalid selection";

/// Entry point for the built-in `cd` command.
///
/// Supported forms:
/// - `cd`          → change to $HOME
/// - `cd <path>`   → change to path, print new directory
/// - `cd ?`        → interactive subdirectory list (single level)
/// - `cd ??`       → recursive directory browser
pub fn execute_cd(args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    match args.len() {
        0 => {
            let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
            env::set_current_dir(&home)?;
            Ok(0)
        }
        1 => match args[0].as_str() {
            "?" => handle_cd_interactive(),
            "??" => handle_cd_recursive(),
            path => {
                env::set_current_dir(path)?;
                println!("{}", env::current_dir()?.display());
                Ok(0)
            }
        },
        _ => {
            eprintln!("cd: too many arguments");
            Ok(1)
        }
    }
}

/// `cd ?` — list non-hidden subdirectories, let user enter a number or q.
pub fn handle_cd_interactive() -> Result<i32, Box<dyn std::error::Error>> {
    let current_dir = env::current_dir()?;
    let mut subdirs: Vec<String> = Vec::new();

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
    subdirs.sort();

    if subdirs.is_empty() {
        println!("{}", MSG_NO_SUBDIRECTORY);
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        return Ok(0);
    }

    for (i, dir) in subdirs.iter().enumerate() {
        println!("{}) {}", i + 1, dir);
    }

    loop {
        print!("{}: ", MSG_SELECT_DIRECTORY);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();

        match trimmed {
            "q" | "Q" => break,
            _ => {
                if let Ok(choice) = trimmed.parse::<usize>() {
                    if choice >= 1 && choice <= subdirs.len() {
                        let target_dir = current_dir.join(&subdirs[choice - 1]);
                        env::set_current_dir(&target_dir)?;
                        println!("{}", env::current_dir()?.display());
                        break;
                    }
                }
                println!("{}", ERR_INVALID_SELECTION);
            }
        }
    }

    Ok(0)
}

/// `cd ??` — start recursive directory browser at current directory.
pub fn handle_cd_recursive() -> Result<i32, Box<dyn std::error::Error>> {
    let start = env::current_dir()?;
    recursive_cd_selector(&start)
}

/// Recursive helper: show subdirectories of `current_dir`, handle user input.
pub fn recursive_cd_selector(current_dir: &Path) -> Result<i32, Box<dyn std::error::Error>> {
    let mut options: Vec<PathBuf> = Vec::new();

    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && !path.is_symlink() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                println!("{}) {}", options.len() + 1, name);
                options.push(path);
            }
        }
    }

    if current_dir != Path::new("/") {
        println!("l] {}", MSG_BACK_DIRECTORY);
    }
    println!("q] {}", MSG_EXIT_RECURSIVECD);

    loop {
        print!("{}: {} > ", MSG_CURRENT_DIRECTORY, current_dir.display());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();

        match trimmed {
            "q" | "Q" => return Ok(0),
            "l" | "L" => {
                if current_dir != Path::new("/") {
                    if let Some(parent) = current_dir.parent() {
                        env::set_current_dir(parent)?;
                        return recursive_cd_selector(&env::current_dir()?);
                    }
                } else {
                    println!("{}", MSG_AT_ROOTDIRECTORY);
                }
            }
            _ => {
                if let Ok(choice) = trimmed.parse::<usize>() {
                    if choice >= 1 && choice <= options.len() {
                        let selected = &options[choice - 1];
                        env::set_current_dir(selected)?;
                        return recursive_cd_selector(&env::current_dir()?);
                    }
                }
                println!("{}", ERR_INVALID_SELECTION);
            }
        }
    }
}
