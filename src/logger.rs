// logger.rs - ARCHITECTURE.md §3.4 Logging Module
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{LineWriter, Write};
use std::sync::Mutex;

/// Log entry matching ARCHITECTURE.md §3.4 field definitions exactly.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogEntry {
    /// ISO 8601 UTC (second precision)
    pub timestamp: String,
    /// INFO / WARN
    pub level: String,
    /// Username who executed the command
    pub user: String,
    /// disable / permissive / Enforcing
    pub mode: String,
    /// Original user input command
    pub command: String,
    /// Current working directory at execution time
    pub working_dir: String,
    /// Process ID
    pub pid: u32,
    /// Exit code (reserved, currently 0)
    pub exit_code: i32,
    /// Additional info
    pub message: String,
}

impl LogEntry {
    /// Construct a new log entry, auto-populating timestamp and pid.
    pub fn new(
        level: impl Into<String>,
        user: impl Into<String>,
        mode: impl Into<String>,
        command: impl Into<String>,
        working_dir: impl Into<String>,
        pid: u32,
        message: impl Into<String>,
    ) -> Self {
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        Self {
            timestamp,
            level: level.into(),
            user: user.into(),
            mode: mode.into(),
            command: command.into(),
            working_dir: working_dir.into(),
            pid,
            exit_code: 0, // reserved
            message: message.into(),
        }
    }

    /// Serialize to JSON with trailing newline (JSON Lines format).
    pub fn to_json_line(&self) -> Result<String, serde_json::Error> {
        let json = serde_json::to_string(self)?;
        Ok(json + "\n")
    }
}

/// Thread-safe JSON Lines log writer. Uses Mutex<LineWriter<File>>.
/// ARCHITECTURE.md §3.4: append mode, auto-creates log file, no auto-flush per entry.
pub struct JsonLinesWriter {
    writer: Mutex<LineWriter<File>>,
}

impl JsonLinesWriter {
    /// Open (or create) the log file in append mode.
    /// Log file name: audit.log in the program's working directory.
    pub fn new(path: &str) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let writer = LineWriter::new(file);
        Ok(Self {
            writer: Mutex::new(writer),
        })
    }

    /// Write a single log entry. Does not panic on lock failure; returns Err instead.
    pub fn write_entry(&self, entry: &LogEntry) -> anyhow::Result<()> {
        let line = entry
            .to_json_line()
            .map_err(|e| anyhow::anyhow!("JSON serialization error: {}", e))?;
        let mut guard = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Logger mutex poisoned"))?;
        guard
            .write_all(line.as_bytes())
            .map_err(|e| anyhow::anyhow!("Log write error: {}", e))?;
        Ok(())
    }

    /// Flush the underlying buffer. Call before program exit.
    pub fn flush(&self) -> anyhow::Result<()> {
        let mut guard = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Logger mutex poisoned"))?;
        guard
            .flush()
            .map_err(|e| anyhow::anyhow!("Log flush error: {}", e))?;
        Ok(())
    }
}
