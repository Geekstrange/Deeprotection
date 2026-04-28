// utils.rs - ARCHITECTURE.md §3.7 Auxiliary Features
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use terminal_size::{terminal_size, Width};

/// Get the current OS username via the `users` crate.
pub fn get_current_user() -> String {
    users::get_current_username()
        .map(|os| os.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Get the current working directory. Falls back to "." on error.
pub fn get_current_working_dir() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Startup animation: slides "dpshell>" from left to right across the terminal.
/// The final resting color depends on the active mode:
///   - "permissive" → green  (\x1b[32m)
///   - "enforcing"  → red    (\x1b[31m)
///   - "disable" / other → terminal default (no color code)
/// Retained from original implementation (Refactored_Plan.md §5); only the
/// color selection is new.
pub fn start_animation(mode: &str) {
    let color = match mode {
        "permissive" => "\x1b[32m",
        "enforcing"  => "\x1b[31m",
        _            => "",          // "disable" or any unknown mode: default color
    };

    let str_text = "dpshell>";
    let cols = if let Some((Width(w), _)) = terminal_size() {
        w as usize
    } else {
        80
    };
    let len = str_text.len();

    // Hide cursor
    print!("\x1b[?25l");
    io::stdout().flush().unwrap();

    for i in 0..=(cols.saturating_sub(len)) {
        print!("\r{:width$}{}", "", str_text, width = i);
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(1));
    }

    // Final position: right-aligned, mode-driven color, then reset
    print!(
        "\r{}{:width$}{}\x1b[0m\n",
        color,
        "",
        str_text,
        width = cols.saturating_sub(len)
    );
    // Restore cursor
    print!("\x1b[?25h");
    io::stdout().flush().unwrap();
}

/// Build the shell prompt string.
/// Format: `dpshell(<level>)$ ` (or `#` for root).
/// ARCHITECTURE.md §3.7: DPSHELL_LEVEL env var tracks nesting depth.
pub fn get_prompt(dpshell_level: u32) -> String {
    let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let prompt_char = if user == "root" { "#" } else { "$" };
    format!("dpshell({}){} ", dpshell_level, prompt_char)
}