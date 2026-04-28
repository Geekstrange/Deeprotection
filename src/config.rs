// config.rs - ARCHITECTURE.md §4 Configuration File Format
use serde::Deserialize;

/// Top-level configuration structure, parsed from TOML.
/// Config file path: /etc/deeprotection/config.toml
#[derive(Debug, Deserialize)]
pub struct Config {
    pub core: CoreConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// [core] section
#[derive(Debug, Deserialize)]
pub struct CoreConfig {
    /// Operating mode: "disable" | "permissive" | "enforcing"
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_mode() -> String {
    "permissive".to_string()
}

/// [auth] section
#[derive(Debug, Deserialize, Default)]
pub struct AuthConfig {
    /// SHA-256 hex digest of the admin password (generated via `echo -n "pass" | sha256sum`)
    #[serde(default)]
    pub password_hash: String,
}

/// [paths] section
#[derive(Debug, Deserialize, Default)]
pub struct PathsConfig {
    /// List of protected path prefixes
    #[serde(default)]
    pub protect: Vec<String>,
    /// Commands that are allowed (with auth) on protected paths; all others are outright blocked
    #[serde(default)]
    pub allowlist: Vec<String>,
}

/// Single [[rules]] entry
#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub name: String,
    /// Plain string or "re:<regex>" for explicit regex
    pub pattern: String,
    pub action: Action,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Rule action: block or replace
#[derive(Debug, Deserialize, Clone)]
pub struct Action {
    /// If Some(true), block execution
    pub block: Option<bool>,
    /// If Some(cmd), replace with this command
    pub replace: Option<String>,
}

impl Action {
    pub fn is_block(&self) -> bool {
        self.block.unwrap_or(false)
    }

    pub fn replacement(&self) -> Option<&str> {
        self.replace.as_deref()
    }
}

/// Load config from the fixed path /etc/deeprotection/config.toml
pub fn load_config() -> anyhow::Result<Config> {
    let content = std::fs::read_to_string("/etc/deeprotection/config.toml")?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}