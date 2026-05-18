// config.rs - ARCHITECTURE.md §4 Configuration File Format
use serde::Deserialize;

/// Hardcoded config file path — used by load_config() and the inotify watcher.
pub const CONFIG_PATH: &str = "/etc/deeprotection/config.toml";

/// Top-level configuration structure, parsed from TOML.
/// Config file path: /etc/deeprotection/config.toml
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub core: CoreConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// [features] section — interactive feature toggles.
    #[serde(default)]
    pub features: FeaturesConfig,
}

/// [features] section
///
/// Example config.toml:
/// ```toml
/// [features]
/// syntax_highlighting = true
/// auto_suggest        = false
/// enhance_completion  = true
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct FeaturesConfig {
    /// Real-time syntax colouring as the user types.
    #[serde(default = "default_true")]
    pub syntax_highlighting: bool,
    /// Grey ghost-text autosuggestions from history.
    #[serde(default = "default_true")]
    pub auto_suggest: bool,
    /// Tab-triggered smart completion with fuzzy matching and columnar menu.
    /// When false, completion still works but uses the same smart engine.
    #[serde(default = "default_true")]
    pub enhance_completion: bool,
}

fn default_true() -> bool {
    true
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            syntax_highlighting: true,
            auto_suggest: true,
            enhance_completion: true,
        }
    }
}

/// [core] section
#[derive(Debug, Deserialize, Clone)]
pub struct CoreConfig {
    /// Operating mode: "disable" | "permissive" | "enforcing"
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Load ~/.bashrc on startup, use ~/.bash_history, disable startup animation.
    /// Default: false (dpshell native behaviour).
    #[serde(default = "default_false")]
    pub bash_compat: bool,
    /// Watch /etc/deeprotection/config.toml with inotify and reload on change.
    /// Default: true. When false, config is read once at startup only.
    #[serde(default = "default_true")]
    pub dynamic_config: bool,
}

fn default_false() -> bool {
    false
}

fn default_mode() -> String {
    "permissive".to_string()
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            bash_compat: false,
            dynamic_config: true,
        }
    }
}

/// [auth] section
#[derive(Debug, Deserialize, Default, Clone)]
pub struct AuthConfig {
    /// SHA-256 hex digest of the admin password (generated via `echo -n "pass" | sha256sum`)
    #[serde(default)]
    pub password_hash: String,
}

/// [paths] section
#[derive(Debug, Deserialize, Default, Clone)]
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
    let content = std::fs::read_to_string(CONFIG_PATH)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}
