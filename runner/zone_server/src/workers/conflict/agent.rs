//! Running a model against one conflict, with nothing but the conflicted files.
//!
//! The repair gets two tools — read and write — each wrapped in the conflicted
//! file set, and a tool context whose working directory is the throwaway checkout
//! and whose environment came from [`super::environment`]. There is no shell, no
//! patch tool, no search, and no network: everything a repair could reach for to
//! go beyond resolving the files it was given has been left out rather than
//! forbidden in prose.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use zone_core::llm::{LlmClient, Message, ToolDefinition};
use zone_core::tools::{
    ReadFileTool, Session, Tool, ToolContext, ToolRegistry, ToolResult, WriteFileTool,
};
use zone_vcs::conflict::Conflict;

use super::scope::{RepairScope, Scoped};

/// Turns a repair may take before it has either finished or lost its way.
const MAX_TURNS: usize = 12;

/// Longest a single repair may run.
pub const REPAIR_TIMEOUT: Duration = Duration::from_secs(600);

/// Everything one repair turn needs, already scoped and isolated.
pub struct RepairTask<'a> {
    pub conflict: &'a Conflict,
    pub scope: Arc<RepairScope>,
    pub context: ToolContext,
    pub system: String,
    pub prompt: String,
}

impl RepairTask<'_> {
    /// The read and write tools, each held to the conflicted files.
    pub fn tools(&self) -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        for tool in [
            Arc::new(ReadFileTool) as Arc<dyn Tool>,
            Arc::new(WriteFileTool) as Arc<dyn Tool>,
        ] {
            registry.register(Arc::new(Scoped::new(tool, self.scope.clone())));
        }
        registry
    }
}

/// Something that can resolve the conflicted files in a prepared checkout.
#[async_trait]
pub trait RepairAgent: Send + Sync {
    async fn repair(&self, task: &RepairTask<'_>) -> Result<(), String>;
}

/// A repair driven by a language model over the scoped read and write tools.
pub struct ModelRepairAgent {
    client: LlmClient,
    model: String,
}

impl ModelRepairAgent {
    pub fn new(client: LlmClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }
}

#[async_trait]
impl RepairAgent for ModelRepairAgent {
    async fn repair(&self, task: &RepairTask<'_>) -> Result<(), String> {
        let registry = task.tools();
        let definitions: Vec<ToolDefinition> = registry.definitions();
        let mut messages = vec![
            Message::system(task.system.clone()),
            Message::user(task.prompt.clone()),
        ];

        for _ in 0..MAX_TURNS {
            let response = self
                .client
                .chat_with_model(&self.model, &messages, Some(&definitions))
                .await
                .map_err(|error| error.to_string())?;

            let Some(choice) = response.choices.into_iter().next() else {
                return Err("the model returned no choices".to_string());
            };

            let calls = choice.message.tool_calls.clone().unwrap_or_default();
            messages.push(choice.message);

            if calls.is_empty() {
                return Ok(());
            }

            for call in calls {
                let arguments = serde_json::from_str(&call.function.arguments)
                    .unwrap_or(serde_json::Value::Null);
                let outcome = registry
                    .execute(&call.function.name, arguments, &task.context)
                    .await
                    .unwrap_or_else(|error| ToolResult::error(error.to_string()));

                let rendered = outcome
                    .output
                    .or(outcome.error)
                    .unwrap_or_else(|| "no output".to_string());
                messages.push(Message::tool_result(call.id, rendered));
            }
        }

        Err(format!(
            "the repair did not finish within {MAX_TURNS} turns"
        ))
    }
}

/// The tool context a repair runs in: the throwaway checkout, and nothing else.
pub fn context(conflict: &Conflict, environment: HashMap<String, String>) -> ToolContext {
    ToolContext {
        cwd: conflict.path().to_path_buf(),
        env: environment,
        max_file_size: 10 * 1024 * 1024,
        command_timeout: 60,
        unrestricted: false,
        session: Session::Detached,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_vcs::conflict::ConflictedPath;

    fn registry(root: &std::path::Path, files: &[&str]) -> ToolRegistry {
        let scope = Arc::new(RepairScope::confined(
            root,
            files
                .iter()
                .map(|value| ConflictedPath::parse(value).unwrap())
                .collect(),
        ));
        let mut registry = ToolRegistry::new();
        for tool in [
            Arc::new(ReadFileTool) as Arc<dyn Tool>,
            Arc::new(WriteFileTool) as Arc<dyn Tool>,
        ] {
            registry.register(Arc::new(Scoped::new(tool, scope.clone())));
        }
        registry
    }

    #[test]
    fn a_repair_is_offered_only_the_read_and_write_tools() {
        let root = tempfile::TempDir::new().unwrap();
        let registry = registry(root.path(), &["src/value.rs"]);
        let mut names = registry.names();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["read_file", "write_file"],
            "no shell, no patch tool, no search, no network"
        );
    }

    #[tokio::test]
    async fn every_offered_tool_refuses_a_file_outside_the_conflict() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("README.md"), "docs").unwrap();
        let registry = registry(root.path(), &["src/value.rs"]);
        let context = ToolContext {
            cwd: root.path().to_path_buf(),
            env: HashMap::new(),
            max_file_size: 1024,
            command_timeout: 5,
            unrestricted: false,
            session: Session::Detached,
        };

        for name in ["read_file", "write_file"] {
            let outcome = registry
                .execute(
                    name,
                    serde_json::json!({ "path": "README.md", "content": "rewritten" }),
                    &context,
                )
                .await
                .unwrap();
            assert!(!outcome.success, "{name} must refuse an unconflicted file");
        }

        assert_eq!(
            std::fs::read_to_string(root.path().join("README.md")).unwrap(),
            "docs",
            "a refused write must not have happened"
        );
    }
}
