use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Could not determine config directory")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    // Threads credentials
    pub access_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// Optional auth server URL for hosted OAuth flow
    pub auth_server: Option<String>,

    // Bluesky credentials
    pub bluesky: Option<BlueskyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueskyConfig {
    pub identifier: String,
    pub password: String,
    /// Optional: serialized session data for persistence
    pub session: Option<String>,
}

impl Config {
    /// Get the config directory path (~/.config/ndl)
    pub fn dir() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|p| p.join("ndl"))
            .ok_or(ConfigError::NoConfigDir)
    }

    /// Get the config file path (~/.config/ndl/config.toml)
    pub fn path() -> Result<PathBuf, ConfigError> {
        Ok(Self::dir()?.join("config.toml"))
    }

    /// Load config from disk, or return default if it doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::path()?;
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&contents)?)
        } else {
            Ok(Self::default())
        }
    }

    /// Save config to disk, creating the directory if needed
    pub fn save(&self) -> Result<(), ConfigError> {
        let dir = Self::dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = Self::path()?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    /// Check if we have a valid access token
    pub fn is_authenticated(&self) -> bool {
        self.access_token.is_some()
    }

    /// Check if client credentials are configured
    #[allow(dead_code)]
    pub fn has_credentials(&self) -> bool {
        self.client_id.is_some() && self.client_secret.is_some()
    }

    /// Check if Bluesky credentials are configured
    pub fn has_bluesky(&self) -> bool {
        self.bluesky.is_some()
    }

    /// Check if Threads is authenticated
    pub fn has_threads(&self) -> bool {
        self.access_token.is_some()
    }
}
