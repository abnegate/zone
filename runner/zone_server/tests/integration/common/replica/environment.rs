use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;
use uuid::Uuid;

const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379";
const JWT_SECRET: &str = "replica-test-jwt-secret-of-at-least-32-bytes";
const LITELLM_KEY: &str = "replica-test-key";
const AGENTS: &str = "agents";
const CLONES: &str = "clones";

/// Everything the replicas of one test share: their database, Valkey and
/// LiteLLM, the agent state directory a shared volume would hold, and an
/// encryption key of the test's own. The key derives the bus namespace, so
/// tests running at once on one Valkey never hear each other.
#[derive(Debug, Clone)]
pub struct Environment {
    pub database_url: String,
    pub redis_url: String,
    pub litellm_url: String,
    pub encryption_key: String,
    shared: Arc<TempDir>,
    variables: Vec<(String, String)>,
}

impl Environment {
    /// Valkey comes from `REDIS_URL`, as it does for the rest of the suite.
    pub fn new(database_url: impl Into<String>, litellm_url: impl Into<String>) -> Self {
        let shared = TempDir::new().expect("a shared state directory");
        for directory in [AGENTS, CLONES] {
            std::fs::create_dir_all(shared.path().join(directory)).expect("a shared directory");
        }
        Self {
            database_url: database_url.into(),
            redis_url: std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.into()),
            litellm_url: litellm_url.into(),
            encryption_key: format!("replica-test-{}", Uuid::new_v4().simple()),
            shared: Arc::new(shared),
            variables: Vec::new(),
        }
    }

    /// The same environment reaching Valkey at `url`, such as through a proxy.
    pub fn with_redis(mut self, url: impl Into<String>) -> Self {
        self.redis_url = url.into();
        self
    }

    /// Adds or overrides one variable on top of the defaults.
    pub fn with_variable(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.push((name.into(), value.into()));
        self
    }

    pub fn agent_state(&self) -> PathBuf {
        self.shared.path().join(AGENTS)
    }

    pub fn clones(&self) -> PathBuf {
        self.shared.path().join(CLONES)
    }

    /// The full environment of a replica listening on `port` with `home` as HOME.
    pub fn variables(&self, port: u16, home: &std::path::Path) -> Vec<(String, String)> {
        let refused = super::super::REFUSED_URL.to_string();
        let mut variables: Vec<(String, String)> = [
            ("PATH", std::env::var("PATH").unwrap_or_default()),
            ("HOME", home.display().to_string()),
            ("HOST", "127.0.0.1".to_string()),
            ("PORT", port.to_string()),
            ("DATABASE_URL", self.database_url.clone()),
            ("REDIS_URL", self.redis_url.clone()),
            ("JWT_SECRET", JWT_SECRET.to_string()),
            ("ENCRYPTION_KEY", self.encryption_key.clone()),
            ("LITELLM_HOST", self.litellm_url.clone()),
            ("LITELLM_KEY", LITELLM_KEY.to_string()),
            (
                "ZONE_AGENT_STATE_DIR",
                self.agent_state().display().to_string(),
            ),
            (
                "ZONE_PROJECT_CLONE_DIR",
                self.clones().display().to_string(),
            ),
            ("ZONE_CLAUDE_API_URL", refused.clone()),
            ("ZONE_CODEX_API_URL", refused.clone()),
            ("OLLAMA_HOST", refused.clone()),
            ("COMFYUI_BASE_URL", refused),
            ("COMFYUI_ENABLED", "false".to_string()),
            ("ZONE_AUTO_ENABLED", "false".to_string()),
            ("SOURCE_RESYNC_ENABLED", "false".to_string()),
            ("MONITORING_ENABLED", "false".to_string()),
            (
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            ),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
        for (name, value) in &self.variables {
            variables.retain(|(existing, _)| existing != name);
            variables.push((name.clone(), value.clone()));
        }
        variables
    }
}
