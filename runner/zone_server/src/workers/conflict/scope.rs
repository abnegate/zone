//! A tool that may only touch the conflicted files.
//!
//! The repair's working directory already confines the file tools to the
//! throwaway checkout, but the checkout is a whole repository: without a second
//! bound an agent could rewrite every file in it and call the conflict resolved.
//! This wraps a tool and refuses any call whose `path` is not one of the files
//! git reported unmerged, so the permission set matches the prompt exactly.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use zone_core::llm::ToolDefinition;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolResult};
use zone_vcs::conflict::{Conflict, ConflictedPath};

/// The parameter every file tool names its target with.
const PATH: &str = "path";

/// The files one repair may read and write.
#[derive(Debug, Clone)]
pub struct RepairScope {
    root: std::path::PathBuf,
    files: Vec<ConflictedPath>,
}

impl RepairScope {
    pub fn new(conflict: &Conflict) -> Self {
        Self::confined(conflict.path(), conflict.files().to_vec())
    }

    pub fn confined(root: &std::path::Path, files: Vec<ConflictedPath>) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            files,
        }
    }

    pub fn files(&self) -> &[ConflictedPath] {
        &self.files
    }

    /// Whether a path a tool was handed names one of the conflicted files.
    pub fn admits(&self, candidate: &str) -> bool {
        self.relative(candidate)
            .is_some_and(|path| self.files.contains(&path))
    }

    fn relative(&self, candidate: &str) -> Option<ConflictedPath> {
        let trimmed = candidate.trim();
        let as_path = std::path::Path::new(trimmed);
        let within = if as_path.is_absolute() {
            as_path.strip_prefix(&self.root).ok()?.to_path_buf()
        } else {
            as_path.to_path_buf()
        };
        ConflictedPath::parse(
            &within
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/"),
        )
        .ok()
    }
}

/// One tool, held to the conflicted file set.
pub struct Scoped {
    inner: Arc<dyn Tool>,
    scope: Arc<RepairScope>,
}

impl Scoped {
    pub fn new(inner: Arc<dyn Tool>, scope: Arc<RepairScope>) -> Self {
        Self { inner, scope }
    }

    fn refusal(&self, candidate: &str) -> ToolResult {
        ToolResult::error(format!(
            "{candidate} is not one of the conflicted files. Only these may be read or written: {}",
            self.scope
                .files()
                .iter()
                .map(ConflictedPath::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

#[async_trait]
impl Tool for Scoped {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters_schema(&self) -> Value {
        self.inner.parameters_schema()
    }

    fn to_definition(&self) -> ToolDefinition {
        self.inner.to_definition()
    }

    fn mutating(&self) -> bool {
        self.inner.mutating()
    }

    fn timeout(&self, context: &ToolContext) -> std::time::Duration {
        self.inner.timeout(context)
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let Some(candidate) = params.get(PATH).and_then(Value::as_str) else {
            return Err(ToolError::InvalidParams(format!(
                "{} requires a {PATH} naming one of the conflicted files",
                self.inner.name()
            )));
        };

        if !self.scope.admits(candidate) {
            return Ok(self.refusal(candidate));
        }

        self.inner.execute(params, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Recorder;

    #[async_trait]
    impl Tool for Recorder {
        fn name(&self) -> &str {
            "write_file"
        }

        fn description(&self) -> &str {
            "records the call"
        }

        fn parameters_schema(&self) -> Value {
            json!({ "type": "object", "properties": { "path": { "type": "string" } } })
        }

        async fn execute(
            &self,
            params: Value,
            _context: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success(format!("wrote {}", params["path"])))
        }

        fn mutating(&self) -> bool {
            true
        }
    }

    fn scope(root: &std::path::Path, files: &[&str]) -> Arc<RepairScope> {
        Arc::new(RepairScope::confined(
            root,
            files
                .iter()
                .map(|value| ConflictedPath::parse(value).unwrap())
                .collect(),
        ))
    }

    #[tokio::test]
    async fn a_conflicted_file_is_written_through() {
        let root = tempfile::TempDir::new().unwrap();
        let scoped = Scoped::new(Arc::new(Recorder), scope(root.path(), &["src/value.rs"]));

        let result = scoped
            .execute(
                json!({ "path": "src/value.rs", "content": "resolved" }),
                &ToolContext::default(),
            )
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.unwrap().contains("src/value.rs"));
    }

    #[tokio::test]
    async fn the_absolute_form_of_a_conflicted_file_is_the_same_file() {
        let root = tempfile::TempDir::new().unwrap();
        let scope = scope(root.path(), &["src/value.rs"]);
        let absolute = scope.root.join("src/value.rs");

        assert!(scope.admits(&absolute.to_string_lossy()));
    }

    #[tokio::test]
    async fn a_path_outside_the_conflicted_set_is_refused() {
        let root = tempfile::TempDir::new().unwrap();
        let scoped = Scoped::new(Arc::new(Recorder), scope(root.path(), &["src/value.rs"]));

        for refused in [
            "README.md",
            "src/other.rs",
            ".git/config",
            "../outside.rs",
            "src/../README.md",
            "/etc/passwd",
            "",
        ] {
            let result = scoped
                .execute(json!({ "path": refused }), &ToolContext::default())
                .await
                .unwrap();
            assert!(
                !result.success,
                "{refused:?} is not conflicted and must be refused"
            );
            assert!(
                result
                    .error
                    .unwrap()
                    .contains("not one of the conflicted files"),
                "the refusal must say why"
            );
        }
    }

    #[tokio::test]
    async fn an_absolute_path_outside_the_checkout_is_refused() {
        let root = tempfile::TempDir::new().unwrap();
        let elsewhere = tempfile::TempDir::new().unwrap();
        let scoped = Scoped::new(Arc::new(Recorder), scope(root.path(), &["src/value.rs"]));

        let escape = elsewhere.path().join("src/value.rs");
        let result = scoped
            .execute(
                json!({ "path": escape.to_string_lossy() }),
                &ToolContext::default(),
            )
            .await
            .unwrap();

        assert!(
            !result.success,
            "a same-named file in another directory is still another directory"
        );
    }

    #[tokio::test]
    async fn a_call_with_no_path_at_all_is_an_invalid_call() {
        let root = tempfile::TempDir::new().unwrap();
        let scoped = Scoped::new(Arc::new(Recorder), scope(root.path(), &["src/value.rs"]));

        let failure = scoped
            .execute(json!({ "content": "resolved" }), &ToolContext::default())
            .await
            .expect_err("a file tool with no path cannot be scoped and must not run");

        assert!(matches!(failure, ToolError::InvalidParams(_)));
    }

    #[test]
    fn the_scope_carries_the_conflicted_names_forward() {
        let root = tempfile::TempDir::new().unwrap();
        let scope = scope(root.path(), &["src/value.rs", "docs/notes.md"]);
        assert_eq!(
            scope
                .files()
                .iter()
                .map(ConflictedPath::as_str)
                .collect::<Vec<_>>(),
            vec!["src/value.rs", "docs/notes.md"]
        );
    }
}
