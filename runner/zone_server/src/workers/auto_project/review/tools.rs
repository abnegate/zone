//! What a reviewer may read: the change, the files it touched, and any file
//! at the head it is reviewing. Nothing on disk, nothing it can change.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use zone_core::tools::{Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};
use zone_vcs::pull_request::{ChangedFile, PrService, PullRequestReference};

/// Bytes of one file the reviewer may read at a time.
const FILE_BYTES: usize = 40_000;

pub struct Shared {
    pub pr: PrService,
    pub reference: PullRequestReference,
    pub head: String,
    pub token: String,
    pub diff: String,
    pub files: Vec<ChangedFile>,
}

/// The read-only tools a reviewer session gets, backed by the pull request service.
pub fn registry(shared: Arc<Shared>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadPrFile(Arc::clone(&shared))));
    registry.register(Arc::new(ListPrFiles(Arc::clone(&shared))));
    registry.register(Arc::new(ReadDiff(shared)));
    registry
}

struct ReadPrFile(Arc<Shared>);

#[async_trait]
impl Tool for ReadPrFile {
    /// The tool's name, as the model calls it.
    fn name(&self) -> &str {
        "read_pr_file"
    }

    /// What the model is told the tool does.
    fn description(&self) -> &str {
        "Read one file as it is at the head of the pull request under review. Use it to see the \
         context around a change; the diff alone shows only the changed lines."
    }

    /// The JSON schema of the tool's arguments.
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Repository-relative path, e.g. src/cart.ts"}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    /// A read: nothing to approve.
    fn tier(&self) -> Tier {
        Tier::Read
    }

    /// Answer the model from the pull request through the shared service.
    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let Some(path) = params["path"]
            .as_str()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        else {
            return Ok(ToolResult::error("`read_pr_file` needs a `path`."));
        };
        let shared = &self.0;
        Ok(
            match shared
                .pr
                .fetch_file(
                    &shared.reference.owner,
                    &shared.reference.repository,
                    path,
                    &shared.head,
                    &shared.token,
                    FILE_BYTES,
                )
                .await
            {
                Ok(content) => ToolResult::success(content),
                Err(error) => ToolResult::error(format!("Could not read {path}: {error}")),
            },
        )
    }
}

struct ListPrFiles(Arc<Shared>);

#[async_trait]
impl Tool for ListPrFiles {
    /// The tool's name, as the model calls it.
    fn name(&self) -> &str {
        "list_pr_files"
    }

    /// What the model is told the tool does.
    fn description(&self) -> &str {
        "List every file the pull request changes, with its status and line counts."
    }

    /// The JSON schema of the tool's arguments.
    fn parameters_schema(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    /// A read: nothing to approve.
    fn tier(&self) -> Tier {
        Tier::Read
    }

    /// Answer the model from the pull request through the shared service.
    async fn execute(
        &self,
        _params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let listed: Vec<String> = self
            .0
            .files
            .iter()
            .map(|file| {
                format!(
                    "{} {} (+{} / -{})",
                    file.status, file.filename, file.additions, file.deletions
                )
            })
            .collect();
        Ok(ToolResult::success(if listed.is_empty() {
            "No files were listed for this pull request.".to_string()
        } else {
            listed.join("\n")
        }))
    }
}

struct ReadDiff(Arc<Shared>);

#[async_trait]
impl Tool for ReadDiff {
    /// The tool's name, as the model calls it.
    fn name(&self) -> &str {
        "read_diff"
    }

    /// What the model is told the tool does.
    fn description(&self) -> &str {
        "Read the pull request's unified diff again, in full, as it was given to you."
    }

    /// The JSON schema of the tool's arguments.
    fn parameters_schema(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    /// A read: nothing to approve.
    fn tier(&self) -> Tier {
        Tier::Read
    }

    /// Answer the model from the pull request through the shared service.
    async fn execute(
        &self,
        _params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::success(self.0.diff.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reviewer_is_offered_only_reading_tools() {
        let shared = Arc::new(Shared {
            pr: PrService::new(),
            reference: PullRequestReference {
                owner: "acme".into(),
                repository: "shop".into(),
                number: 1,
            },
            head: "abc".into(),
            token: "token".into(),
            diff: "diff".into(),
            files: vec![],
        });
        let registry = registry(shared);
        let mut names = registry.names();
        names.sort_unstable();
        assert_eq!(names, vec!["list_pr_files", "read_diff", "read_pr_file"]);
        for name in names {
            assert_eq!(registry.tier(name), Some(Tier::Read), "{name}");
        }
    }
}
