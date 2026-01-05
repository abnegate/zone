//! CLI configuration
//!
//! Manages CLI configuration stored in ~/.zone/config.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Configuration error
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Home directory not found")]
    NoHomeDir,
}

/// CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default model to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Default host URL
    pub host: Option<String>,

    /// Maximum iterations for agent loop
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Editor command for editing files
    #[serde(default = "default_editor")]
    pub editor: String,
}

fn default_model() -> String {
    "gpt-4o".to_string()
}

fn default_max_iterations() -> u32 {
    50
}

fn default_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: default_model(),
            host: None,
            max_iterations: default_max_iterations(),
            editor: default_editor(),
        }
    }
}

impl Config {
    /// Get the config directory path (~/.zone)
    pub fn config_dir() -> Result<PathBuf, ConfigError> {
        let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
        Ok(home.join(".zone"))
    }

    /// Get the config file path (~/.zone/config.toml)
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    /// Get the sessions directory path (~/.zone/sessions)
    pub fn sessions_dir() -> Result<PathBuf, ConfigError> {
        Ok(Self::config_dir()?.join("sessions"))
    }

    /// Load configuration from file, creating default if it doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;

        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), ConfigError> {
        let dir = Self::config_dir()?;
        std::fs::create_dir_all(&dir)?;

        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;

        Ok(())
    }

    /// Ensure sessions directory exists
    pub fn ensure_sessions_dir() -> Result<PathBuf, ConfigError> {
        let dir = Self::sessions_dir()?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_model() {
        assert_eq!(default_model(), "gpt-4o");
    }

    #[test]
    fn test_default_max_iterations() {
        assert_eq!(default_max_iterations(), 50);
    }

    #[test]
    fn test_default_editor_from_env() {
        // Test that it reads from EDITOR or falls back to vim
        let editor = default_editor();
        // Either should be EDITOR env var or vim
        assert!(!editor.is_empty());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert_eq!(config.model, "gpt-4o");
        assert!(config.host.is_none());
        assert_eq!(config.max_iterations, 50);
        assert!(!config.editor.is_empty());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config {
            model: "gpt-4".to_string(),
            host: Some("https://zone.example.com".to_string()),
            max_iterations: 100,
            editor: "nano".to_string(),
        };

        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("gpt-4"));
        assert!(toml_str.contains("zone.example.com"));
        assert!(toml_str.contains("100"));
        assert!(toml_str.contains("nano"));
    }

    #[test]
    fn test_config_deserialization() {
        let toml_str = r#"
            model = "claude-3"
            host = "https://api.example.com"
            max_iterations = 25
            editor = "code"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.model, "claude-3");
        assert_eq!(config.host, Some("https://api.example.com".to_string()));
        assert_eq!(config.max_iterations, 25);
        assert_eq!(config.editor, "code");
    }

    #[test]
    fn test_config_deserialization_defaults() {
        // Empty TOML should use defaults for optional fields
        let toml_str = "";

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.model, "gpt-4o"); // default
        assert!(config.host.is_none());
        assert_eq!(config.max_iterations, 50); // default
    }

    #[test]
    fn test_config_deserialization_partial() {
        // Only some fields specified
        let toml_str = r#"
            model = "custom-model"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.model, "custom-model");
        assert!(config.host.is_none());
        assert_eq!(config.max_iterations, 50); // default
    }

    #[test]
    fn test_config_clone() {
        let config = Config {
            model: "gpt-4".to_string(),
            host: Some("https://zone.example.com".to_string()),
            max_iterations: 100,
            editor: "nano".to_string(),
        };

        let cloned = config.clone();

        assert_eq!(config.model, cloned.model);
        assert_eq!(config.host, cloned.host);
        assert_eq!(config.max_iterations, cloned.max_iterations);
        assert_eq!(config.editor, cloned.editor);
    }

    #[test]
    fn test_config_debug() {
        let config = Config::default();
        let debug_str = format!("{:?}", config);

        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("model"));
    }

    #[test]
    fn test_config_dir() {
        // Should return a path ending in .zone
        if let Ok(dir) = Config::config_dir() {
            assert!(dir.to_string_lossy().ends_with(".zone"));
        }
    }

    #[test]
    fn test_config_path() {
        // Should return a path ending in config.toml
        if let Ok(path) = Config::config_path() {
            assert!(path.to_string_lossy().ends_with("config.toml"));
            assert!(path.to_string_lossy().contains(".zone"));
        }
    }

    #[test]
    fn test_sessions_dir() {
        // Should return a path ending in sessions
        if let Ok(dir) = Config::sessions_dir() {
            assert!(dir.to_string_lossy().ends_with("sessions"));
            assert!(dir.to_string_lossy().contains(".zone"));
        }
    }

    #[test]
    fn test_config_error_display() {
        let io_err = ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found",
        ));
        assert!(io_err.to_string().contains("IO error"));

        let no_home = ConfigError::NoHomeDir;
        assert_eq!(no_home.to_string(), "Home directory not found");
    }

    #[test]
    fn test_config_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
        let config_err: ConfigError = io_err.into();
        assert!(matches!(config_err, ConfigError::Io(_)));
    }

    #[test]
    fn test_config_error_from_toml_parse() {
        let result: Result<Config, toml::de::Error> = toml::from_str("invalid { toml");
        let toml_err = result.unwrap_err();
        let config_err: ConfigError = toml_err.into();
        assert!(matches!(config_err, ConfigError::TomlParse(_)));
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let original = Config {
            model: "gpt-4o-mini".to_string(),
            host: Some("https://zone.test.com".to_string()),
            max_iterations: 75,
            editor: "nvim".to_string(),
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(original.model, deserialized.model);
        assert_eq!(original.host, deserialized.host);
        assert_eq!(original.max_iterations, deserialized.max_iterations);
        assert_eq!(original.editor, deserialized.editor);
    }

    #[test]
    fn test_config_with_none_host() {
        let config = Config {
            model: "gpt-4".to_string(),
            host: None,
            max_iterations: 50,
            editor: "vim".to_string(),
        };

        let toml_str = toml::to_string(&config).unwrap();
        // host should not appear in output when None
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert!(deserialized.host.is_none());
    }

    #[test]
    fn test_max_iterations_range() {
        // Test that various max_iterations values work
        for iterations in [1, 10, 50, 100, 1000] {
            let config = Config {
                model: "test".to_string(),
                host: None,
                max_iterations: iterations,
                editor: "vim".to_string(),
            };

            let toml_str = toml::to_string(&config).unwrap();
            let deserialized: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(deserialized.max_iterations, iterations);
        }
    }
}
