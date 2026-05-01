use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::{CliError, CliResult};

// ---------------------------------------------------------------------------
// Config paths
// ---------------------------------------------------------------------------

/// Returns `~/.intr/config.toml`.
pub fn config_path() -> CliResult<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| CliError::Generic("cannot locate home directory".into()))?;
    Ok(home.join(".intr").join("config.toml"))
}

/// Returns `~/.intr/` directory, creating it if needed.
pub fn intr_dir() -> CliResult<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| CliError::Generic("cannot locate home directory".into()))?;
    let dir = home.join(".intr");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Config struct
// ---------------------------------------------------------------------------

/// Contents of `~/.intr/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub auth: AuthConfig,
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AuthConfig {
    pub default_account: Option<String>,
    pub api_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DefaultsConfig {
    /// Default model to use when frontmatter has no preference.
    pub model: Option<String>,
}

impl Config {
    /// Load config from `~/.intr/config.toml`. Returns default if file missing.
    pub fn load() -> CliResult<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        // Parse as TOML via serde_yaml would fail; use basic toml parsing.
        // We include toml via serde support. For now, fall back to serde_yaml
        // since toml is not in workspace deps yet - replace with toml crate later.
        let config: Config = serde_yaml::from_str(&raw)
            .map_err(|e| CliError::Generic(format!("config parse error: {e}")))?;
        Ok(config)
    }

    /// Save config to `~/.intr/config.toml`.
    pub fn save(&self) -> CliResult<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_yaml::to_string(self)
            .map_err(|e| CliError::Generic(format!("config serialise error: {e}")))?;
        fs::write(&path, raw)?;
        Ok(())
    }

    /// Get a single key by dotted path (for `intr config get <key>`).
    pub fn get_key(&self, key: &str) -> Option<String> {
        match key {
            "auth.default_account" => self.auth.default_account.clone(),
            "auth.api_base_url"    => self.auth.api_base_url.clone(),
            "defaults.model"       => self.defaults.model.clone(),
            _ => None,
        }
    }

    /// Set a single key by dotted path (for `intr config set <key> <value>`).
    pub fn set_key(&mut self, key: &str, value: &str) -> CliResult<()> {
        match key {
            "auth.default_account" => self.auth.default_account = Some(value.to_string()),
            "auth.api_base_url"    => self.auth.api_base_url = Some(value.to_string()),
            "defaults.model"       => self.defaults.model = Some(value.to_string()),
            _ => return Err(CliError::Usage(format!("unknown config key: {key}"))),
        }
        Ok(())
    }

    /// API base URL (default: https://api.intentry.dev).
    pub fn api_base_url(&self) -> &str {
        self.auth
            .api_base_url
            .as_deref()
            .unwrap_or("https://api.intentry.dev")
    }
}

// ---------------------------------------------------------------------------
// Local space detection
// ---------------------------------------------------------------------------

/// Find the `.intr/` directory by walking up from `start`.
pub fn find_space_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".intr").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}
