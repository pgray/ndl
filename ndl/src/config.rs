use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Could not determine config directory")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),
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

    /// Get the config file path (~/.config/ndl/config.json)
    pub fn path() -> Result<PathBuf, ConfigError> {
        Ok(Self::dir()?.join("config.json"))
    }

    /// Get the legacy TOML config path for migration
    fn legacy_path() -> Result<PathBuf, ConfigError> {
        Ok(Self::dir()?.join("config.toml"))
    }

    /// Load config from disk, or return default if it doesn't exist
    /// Automatically migrates from TOML if JSON doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        let json_path = Self::path()?;
        let toml_path = Self::legacy_path()?;

        if json_path.exists() {
            let contents = std::fs::read_to_string(&json_path)?;
            Ok(serde_json::from_str(&contents)?)
        } else if toml_path.exists() {
            // Migrate from TOML
            let contents = std::fs::read_to_string(&toml_path)?;
            let config: Self = toml::from_str(&contents).unwrap_or_default();
            // Save as JSON
            config.save()?;
            // Remove old TOML file
            let _ = std::fs::remove_file(&toml_path);
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Save config to disk, creating the directory if needed
    pub fn save(&self) -> Result<(), ConfigError> {
        let dir = Self::dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = Self::path()?;
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_preserves_bluesky_on_threads_update() {
        // Create a config with both Threads and Bluesky
        let mut config = Config {
            access_token: Some("old_threads_token".to_string()),
            client_id: None,
            client_secret: None,
            auth_server: None,
            bluesky: Some(BlueskyConfig {
                identifier: "user.bsky.social".to_string(),
                password: "secret".to_string(),
                session: Some("session_data".to_string()),
            }),
        };

        // Simulate updating Threads token (what login does)
        config.access_token = Some("new_threads_token".to_string());

        // Verify Bluesky config is still present
        assert!(config.has_bluesky());
        assert!(config.has_threads());
        assert_eq!(
            config.bluesky.as_ref().unwrap().identifier,
            "user.bsky.social"
        );
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = Config {
            access_token: Some("threads_token".to_string()),
            client_id: None,
            client_secret: None,
            auth_server: None,
            bluesky: Some(BlueskyConfig {
                identifier: "user.bsky.social".to_string(),
                password: "secret".to_string(),
                session: Some("session_data".to_string()),
            }),
        };

        // Serialize to JSON
        let json_str = serde_json::to_string_pretty(&config).unwrap();

        // Deserialize back
        let loaded: Config = serde_json::from_str(&json_str).unwrap();

        // Verify both sections are present
        assert!(loaded.has_threads());
        assert!(loaded.has_bluesky());
        assert_eq!(loaded.access_token, Some("threads_token".to_string()));
        assert_eq!(
            loaded.bluesky.as_ref().unwrap().identifier,
            "user.bsky.social"
        );
    }
}
