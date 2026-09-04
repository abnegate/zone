//! How Zone discovers MCP servers.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// A stdio MCP server Zone should launch and keep for the agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerSpec {
    /// Registry key used as the tool-name prefix (`magents_spawn_session`).
    #[serde(default)]
    pub name: String,
    /// Executable to spawn.
    pub command: String,
    /// Arguments after the executable (`["mcp"]` for magents).
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables overlaid on the inherited runner environment.
    ///
    /// Stdio MCP children are trusted local processes: they see `PATH`, `HOME`,
    /// and any credentials already in the Zone process.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Working directory for the child. Inherits the process cwd when omitted.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Skip this server when true.
    #[serde(default)]
    pub disabled: bool,
}

impl McpServerSpec {
    /// Default magents stdio server (`magents mcp`).
    pub fn magents() -> Self {
        Self {
            name: "magents".to_string(),
            command: "magents".to_string(),
            args: vec!["mcp".to_string()],
            env: HashMap::new(),
            cwd: None,
            disabled: false,
        }
    }
}

/// The set of MCP servers to attach for one agent run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpConfig {
    pub servers: Vec<McpServerSpec>,
}

#[derive(Debug, Error)]
pub enum McpConfigError {
    #[error("invalid MCP config: {0}")]
    Invalid(String),
    #[error("failed to read MCP config {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse MCP config: {0}")]
    Json(#[from] serde_json::Error),
}

impl McpConfig {
    /// Load from environment. Never fails: bad files are logged and skipped.
    pub fn from_env() -> Self {
        if !env_flag("ZONE_MCP_ENABLED", true) {
            return Self::default();
        }

        let auto_magents = env_flag("ZONE_MCP_AUTO_MAGENTS", true);

        if let Ok(raw) = std::env::var("ZONE_MCP_SERVERS")
            && !raw.trim().is_empty()
        {
            return match Self::from_json_str(&raw) {
                Ok(mut config) => {
                    config.apply_auto_magents(auto_magents);
                    config
                }
                Err(error) => {
                    tracing::warn!(error = %error, "ZONE_MCP_SERVERS is invalid; ignoring");
                    Self::with_auto_magents(auto_magents)
                }
            };
        }

        if let Ok(path) = std::env::var("ZONE_MCP_CONFIG")
            && !path.trim().is_empty()
        {
            return match Self::from_file(Path::new(&path)) {
                Ok(mut config) => {
                    config.apply_auto_magents(auto_magents);
                    config
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path,
                        error = %error,
                        "ZONE_MCP_CONFIG could not be loaded; ignoring"
                    );
                    Self::with_auto_magents(auto_magents)
                }
            };
        }

        if let Some(home) = dirs::home_dir() {
            let default_path = home.join(".zone").join("mcp.json");
            if default_path.is_file() {
                return match Self::from_file(&default_path) {
                    Ok(mut config) => {
                        config.apply_auto_magents(auto_magents);
                        config
                    }
                    Err(error) => {
                        tracing::warn!(
                            path = %default_path.display(),
                            error = %error,
                            "default MCP config could not be loaded; ignoring"
                        );
                        Self::with_auto_magents(auto_magents)
                    }
                };
            }
        }

        Self::with_auto_magents(auto_magents)
    }

    /// Parse a Cursor-style `mcpServers` document or a bare name → spec map.
    pub fn from_json_str(raw: &str) -> Result<Self, McpConfigError> {
        let value: Value = serde_json::from_str(raw)?;
        Self::from_value(&value)
    }

    /// Read and parse a JSON config file.
    pub fn from_file(path: &Path) -> Result<Self, McpConfigError> {
        let raw = std::fs::read_to_string(path).map_err(|source| McpConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_json_str(&raw)
    }

    /// Parse `{ "mcpServers": { ... } }`, `{ "servers": { ... } }`, or a bare map.
    pub fn from_value(value: &Value) -> Result<Self, McpConfigError> {
        let map = if let Some(servers) = value.get("mcpServers").or_else(|| value.get("servers")) {
            servers
                .as_object()
                .ok_or_else(|| McpConfigError::Invalid("mcpServers must be an object".into()))?
        } else if value.is_object()
            && value
                .as_object()
                .is_some_and(|object| object.values().all(|item| item.is_object()))
        {
            value.as_object().expect("object")
        } else {
            return Err(McpConfigError::Invalid(
                "expected mcpServers object or a map of server specs".into(),
            ));
        };

        let mut servers = Vec::with_capacity(map.len());
        for (name, spec) in map {
            if spec.get("url").is_some() && spec.get("command").is_none() {
                tracing::warn!(
                    server = %name,
                    "skipping HTTP MCP server; Zone currently supports stdio only"
                );
                continue;
            }

            let mut parsed: McpServerSpec = serde_json::from_value(spec.clone())
                .map_err(|error| McpConfigError::Invalid(format!("server '{name}': {error}")))?;
            if parsed.command.trim().is_empty() {
                return Err(McpConfigError::Invalid(format!(
                    "server '{name}' is missing command"
                )));
            }
            parsed.name = name.clone();
            if !parsed.disabled {
                servers.push(parsed);
            }
        }

        Ok(Self { servers })
    }

    fn with_auto_magents(auto: bool) -> Self {
        let mut config = Self::default();
        config.apply_auto_magents(auto);
        config
    }

    fn apply_auto_magents(&mut self, auto: bool) {
        if !auto || !self.servers.is_empty() {
            return;
        }
        if command_on_path("magents") {
            self.servers.push(McpServerSpec::magents());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => parse_flag(&value, default),
        Err(_) => default,
    }
}

fn parse_flag(value: &str, default: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// Whether `name` resolves on `PATH`.
pub(crate) fn command_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        if dir.join(name).is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            if dir.join(format!("{name}.exe")).is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cursor_mcp_servers_document() {
        let config = McpConfig::from_json_str(
            r#"{
                "mcpServers": {
                    "magents": {
                        "command": "magents",
                        "args": ["mcp"]
                    },
                    "docs": {
                        "command": "uvx",
                        "args": ["mcp-server-fetch"],
                        "env": {"FOO": "bar"}
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(config.servers.len(), 2);
        let magents = config
            .servers
            .iter()
            .find(|server| server.name == "magents")
            .unwrap();
        assert_eq!(magents.args, ["mcp"]);
        let docs = config
            .servers
            .iter()
            .find(|server| server.name == "docs")
            .unwrap();
        assert_eq!(docs.env.get("FOO").map(String::as_str), Some("bar"));
    }

    #[test]
    fn parses_bare_server_map() {
        let config = McpConfig::from_json_str(
            r#"{ "magents": { "command": "/opt/homebrew/bin/magents", "args": ["mcp"] } }"#,
        )
        .unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].command, "/opt/homebrew/bin/magents");
    }

    #[test]
    fn skips_disabled_and_http_only_servers() {
        let config = McpConfig::from_json_str(
            r#"{
                "mcpServers": {
                    "off": { "command": "x", "disabled": true },
                    "remote": { "url": "http://localhost:3000/mcp" },
                    "ok": { "command": "magents", "args": ["mcp"] }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "ok");
    }

    #[test]
    fn from_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        std::fs::write(
            &path,
            r#"{ "mcpServers": { "magents": { "command": "magents", "args": ["mcp"] } } }"#,
        )
        .unwrap();
        let config = McpConfig::from_file(&path).unwrap();
        assert_eq!(config.servers[0].name, "magents");
    }

    #[test]
    fn rejects_non_object() {
        let error = McpConfig::from_json_str("[1, 2]").unwrap_err();
        assert!(error.to_string().contains("expected mcpServers"));
    }

    #[test]
    fn auto_magents_skips_when_servers_already_configured() {
        let mut config =
            McpConfig::from_json_str(r#"{ "mcpServers": { "docs": { "command": "echo" } } }"#)
                .unwrap();
        config.apply_auto_magents(true);
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "docs");
    }

    #[test]
    fn magents_default_spec() {
        let spec = McpServerSpec::magents();
        assert_eq!(spec.name, "magents");
        assert_eq!(spec.command, "magents");
        assert_eq!(spec.args, ["mcp"]);
    }

    #[test]
    fn parse_flag_keeps_default_for_empty_and_unknown() {
        assert!(parse_flag("", true));
        assert!(!parse_flag("", false));
        assert!(parse_flag("maybe", true));
        assert!(!parse_flag("maybe", false));
        assert!(parse_flag(" true ", true));
        assert!(!parse_flag("0", true));
        assert!(!parse_flag("off", true));
        assert!(!parse_flag("false", true));
        assert!(parse_flag("1", false));
        assert!(parse_flag("yes", false));
        assert!(parse_flag("on", false));
    }
}
