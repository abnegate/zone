//! CLI configuration
//!
//! Settings live in `~/.zone/config.toml`. `ZONE_CONFIG_DIRECTORY` moves the
//! directory, and the sessions kept in it, and `ZONE_CONFIG_PATH` names the
//! file outright.

use abnegate_config::{Application, Loader};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Names the `~/.zone` directory and the `ZONE_CONFIG_*` variables.
const APPLICATION: &str = "zone";

const SESSIONS: &str = "sessions";

/// Configuration error
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error(transparent)]
    Config(#[from] abnegate_config::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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

    /// OpenAI-compatible base URL `zone run` sends completions to, such as
    /// LiteLLM at http://localhost:4000 or Ollama at http://127.0.0.1:11434/v1
    pub llm_base_url: Option<String>,

    /// Bearer token for `llm_base_url`; omitted for a server that needs none
    pub llm_api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
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
            llm_base_url: None,
            llm_api_key: None,
            device_id: None,
        }
    }
}

fn application() -> Result<Application, ConfigError> {
    Ok(Application::new(APPLICATION).map_err(abnegate_config::Error::from)?)
}

impl Config {
    /// The directory the CLI keeps its files in
    pub fn directory() -> Result<PathBuf, ConfigError> {
        Ok(abnegate_config::directory(&application()?)?)
    }

    /// The configuration file
    pub fn path() -> Result<PathBuf, ConfigError> {
        Ok(abnegate_config::path(&application()?)?)
    }

    /// The directory saved sessions are kept in
    pub fn sessions_directory() -> Result<PathBuf, ConfigError> {
        Ok(Self::directory()?.join(SESSIONS))
    }

    /// Load configuration from file, creating default if it doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&Loader::new(&application()?)?)
    }

    fn load_from(loader: &Loader<'_>) -> Result<Self, ConfigError> {
        let mut config = if loader.exists() {
            loader.load()?.into_value()
        } else {
            let created = abnegate_config::Config::new(loader.path(), Self::default());
            created.save()?;
            created.into_value()
        };
        if config
            .device_id
            .as_deref()
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .is_none()
        {
            config.device_id = Some(uuid::Uuid::new_v4().to_string());
            abnegate_config::Config::new(loader.path(), config.clone()).save()?;
        }
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), ConfigError> {
        Ok(abnegate_config::Config::new(Self::path()?, self).save()?)
    }

    /// Ensure sessions directory exists
    pub fn ensure_sessions_directory() -> Result<PathBuf, ConfigError> {
        let directory = Self::sessions_directory()?;
        std::fs::create_dir_all(&directory)?;
        Ok(directory)
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
            llm_base_url: None,
            llm_api_key: None,
            device_id: None,
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
            llm_base_url: None,
            llm_api_key: None,
            device_id: None,
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
        assert_eq!(application().unwrap().directory(), ".zone");
        if let Ok(dir) = Config::directory() {
            assert!(dir.to_string_lossy().ends_with(".zone"));
        }
    }

    #[test]
    fn test_config_path() {
        if let Ok(path) = Config::path() {
            assert!(path.to_string_lossy().ends_with("config.toml"));
            assert!(path.to_string_lossy().contains(".zone"));
        }
    }

    #[test]
    fn test_sessions_dir() {
        if let Ok(dir) = Config::sessions_directory() {
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

        let no_home = ConfigError::from(abnegate_config::Error::NoHomeDirectory);
        assert_eq!(no_home.to_string(), "Home directory not found");
    }

    #[test]
    fn test_config_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
        let config_err: ConfigError = io_err.into();
        assert!(matches!(config_err, ConfigError::Io(_)));
    }

    #[test]
    fn a_file_that_is_not_toml_fails_the_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "invalid { toml").unwrap();

        let error = Config::load_from(&Loader::at(&path)).unwrap_err();

        assert!(
            matches!(
                error,
                ConfigError::Config(abnegate_config::Error::Parse { .. })
            ),
            "{error:?}"
        );
    }

    /// `zone config` opens the file in an editor, so the first load has to
    /// leave one there.
    #[test]
    fn the_first_load_writes_the_defaults_for_the_editor_to_open() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("config.toml");

        let config = Config::load_from(&Loader::at(&path)).unwrap();

        assert_eq!(config.model, "gpt-4o");
        let written: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.max_iterations, 50);
        let device_id = config.device_id.as_deref().expect("device_id");
        assert!(uuid::Uuid::parse_str(device_id).is_ok(), "{device_id}");
        assert_eq!(written.device_id.as_deref(), Some(device_id));
    }

    #[test]
    fn a_saved_file_keeps_what_an_earlier_cli_wrote() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(
            &path,
            "model = \"claude-3\"\nhost = \"https://api.example.com\"\nmax_iterations = 25\neditor = \"code\"\nllm_api_key = \"ollama\"\n",
        )
        .unwrap();

        let loaded = Config::load_from(&Loader::at(&path)).unwrap();

        assert_eq!(loaded.model, "claude-3");
        assert_eq!(loaded.host.as_deref(), Some("https://api.example.com"));
        assert_eq!(loaded.max_iterations, 25);
        assert_eq!(loaded.llm_api_key.as_deref(), Some("ollama"));
        let device_id = loaded.device_id.as_deref().expect("device_id");
        assert!(uuid::Uuid::parse_str(device_id).is_ok(), "{device_id}");
        let rewritten = std::fs::read_to_string(&path).unwrap();
        assert!(rewritten.contains("claude-3"));
        assert!(rewritten.contains("device_id"));
    }

    #[cfg(unix)]
    #[test]
    fn a_written_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");

        Config::load_from(&Loader::at(&path)).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// The variables are read from the process environment, which every test
    /// shares, so this runs again in a child that sets one.
    #[test]
    fn the_config_directory_variable_moves_the_file_and_the_sessions() {
        const NAME: &str =
            "config::tests::the_config_directory_variable_moves_the_file_and_the_sessions";
        const CHILD: &str = "ZONE_CLI_CONFIG_TEST_CHILD";
        const MOVED: &str = "/nonexistent/zone-moved";

        if std::env::var(CHILD).as_deref() != Ok(NAME) {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture", "--test-threads", "1"])
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env(CHILD, NAME)
                .env("ZONE_CONFIG_DIRECTORY", MOVED)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        assert_eq!(Config::directory().unwrap(), PathBuf::from(MOVED));
        assert_eq!(
            Config::path().unwrap(),
            PathBuf::from(MOVED).join("config.toml")
        );
        assert_eq!(
            Config::sessions_directory().unwrap(),
            PathBuf::from(MOVED).join("sessions")
        );
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let original = Config {
            model: "gpt-4o-mini".to_string(),
            host: Some("https://zone.test.com".to_string()),
            max_iterations: 75,
            editor: "nvim".to_string(),
            llm_base_url: Some("http://127.0.0.1:11434/v1".to_string()),
            llm_api_key: Some("ollama".to_string()),
            device_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(original.model, deserialized.model);
        assert_eq!(original.host, deserialized.host);
        assert_eq!(original.max_iterations, deserialized.max_iterations);
        assert_eq!(original.editor, deserialized.editor);
        assert_eq!(original.llm_base_url, deserialized.llm_base_url);
        assert_eq!(original.llm_api_key, deserialized.llm_api_key);
    }

    #[test]
    fn test_config_with_none_host() {
        let config = Config {
            model: "gpt-4".to_string(),
            host: None,
            max_iterations: 50,
            editor: "vim".to_string(),
            llm_base_url: None,
            llm_api_key: None,
            device_id: None,
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
                llm_base_url: None,
                llm_api_key: None,
                device_id: None,
            };

            let toml_str = toml::to_string(&config).unwrap();
            let deserialized: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(deserialized.max_iterations, iterations);
        }
    }
}
