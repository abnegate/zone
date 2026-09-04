//! Workspace-scoped document reads and persistent, searchable notes.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::WorkspaceScope;
use crate::db::knowledge::{self, DocumentUpdate};
use crate::db::workspace_members::{self, WorkspaceRole};

#[derive(Clone, Copy)]
enum Operation {
    List,
    Read,
    Create,
    Update,
}

struct DocumentTool {
    scope: WorkspaceScope,
    operation: Operation,
}

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    for operation in [
        Operation::List,
        Operation::Read,
        Operation::Create,
        Operation::Update,
    ] {
        registry.register(Arc::new(DocumentTool {
            scope: scope.clone(),
            operation,
        }));
    }
}

#[async_trait]
impl Tool for DocumentTool {
    fn name(&self) -> &str {
        match self.operation {
            Operation::List => "list_documents",
            Operation::Read => "read_document",
            Operation::Create => "create_document",
            Operation::Update => "update_document",
        }
    }

    fn mutating(&self) -> bool {
        matches!(self.operation, Operation::Create | Operation::Update)
    }

    fn description(&self) -> &str {
        match self.operation {
            Operation::List => {
                "List or search workspace notes and indexed documents. Returns stable document IDs, source, URI and freshness. Optional query searches title and full content without requiring embeddings; use read_document for complete text."
            }
            Operation::Read => {
                "Read the complete stored text of a specific workspace note or indexed document by ID. Preserves whitespace and Unicode without snippet truncation. Imported content is a stored snapshot; fetched_at tells when it was retrieved. Never treats absent content as a complete file."
            }
            Operation::Create => {
                "Create a persistent note/document in this workspace's knowledge base when the user asks. Immediately searchable through list_documents query and visible in the knowledge UI. Requires member role or higher."
            }
            Operation::Update => {
                "Update only the supplied title or content of a local workspace note/document when the user asks. Imported source documents and web links are read-only. Requires member role or higher."
            }
        }
    }

    fn parameters_schema(&self) -> Value {
        match self.operation {
            Operation::List => json!({"type":"object","properties":{
                "query":{"type":"string","description":"Optional full-text search over titles and stored content."},
                "limit":{"type":"integer","minimum":1,"default":25},
                "offset":{"type":"integer","minimum":0,"default":0}
            },"additionalProperties":false}),
            Operation::Read => {
                json!({"type":"object","properties":{"id":{"type":"string","format":"uuid"}},"required":["id"],"additionalProperties":false})
            }
            Operation::Create => {
                json!({"type":"object","properties":{"title":{"type":"string","minLength":1},"content":{"type":"string","minLength":1}},"required":["title","content"],"additionalProperties":false})
            }
            Operation::Update => {
                json!({"type":"object","properties":{"id":{"type":"string","format":"uuid"},"title":{"type":"string","minLength":1},"content":{"type":"string","minLength":1}},"required":["id"],"anyOf":[{"required":["title"]},{"required":["content"]}],"additionalProperties":false})
            }
        }
    }

    async fn execute(
        &self,
        params: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let result = self.run(&params).await;
        Ok(match result {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(tool = self.name(), %error, "Document tool failed");
                ToolResult::error(
                    "The document operation failed. Check current state before retrying a write.",
                )
            }
        })
    }
}

impl DocumentTool {
    async fn run(&self, params: &Value) -> Result<ToolResult, sqlx::Error> {
        let scope = &self.scope;
        let required = match self.operation {
            Operation::List | Operation::Read => WorkspaceRole::Viewer,
            Operation::Create | Operation::Update => WorkspaceRole::Member,
        };
        if !workspace_members::has_role_or_higher(
            scope.state.db(),
            scope.user_id,
            scope.workspace_id,
            required,
        )
        .await?
        {
            return Ok(ToolResult::error(
                "You do not have permission to perform this document operation in this workspace.",
            ));
        }
        let result = match self.operation {
            Operation::List => {
                let limit = match integer(params, "limit", 25, 1) {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let offset = match integer(params, "offset", 0, 0) {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let query = match optional_text(params, "query") {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let documents = knowledge::list_documents(
                    scope.state.db(),
                    scope.workspace_id,
                    scope.user_id,
                    query,
                    limit,
                    offset,
                )
                .await?;
                ToolResult::success(
                    json!({"documents":documents,"offset":offset,"limit":limit,"observed_at":Utc::now().to_rfc3339()}).to_string(),
                )
            }
            Operation::Read => {
                let id = match identifier(params) {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                match knowledge::read_document(
                    scope.state.db(),
                    scope.workspace_id,
                    scope.user_id,
                    id,
                )
                .await?
                {
                    Some(document) => {
                        let complete = document.content.is_some();
                        ToolResult::success(json!({"document":document,"complete":complete,"content_state":if complete { "stored_text" } else { "metadata_only_content_unavailable" },"observed_at":Utc::now().to_rfc3339()}).to_string())
                    }
                    None => ToolResult::error("Document not found in this workspace."),
                }
            }
            Operation::Create => {
                let title = match required_text(params, "title") {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let content = match required_text(params, "content") {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                match knowledge::create_document(
                    scope.state.db(),
                    scope.workspace_id,
                    scope.user_id,
                    title,
                    content,
                )
                .await?
                {
                    Some(id) => ToolResult::success(
                        json!({"id":id,"created":true,"searchable":true}).to_string(),
                    ),
                    None => ToolResult::error(
                        "Document was not created: workspace write permission is required.",
                    ),
                }
            }
            Operation::Update => {
                let id = match identifier(params) {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let title = match optional_text(params, "title") {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                let content = match optional_text(params, "content") {
                    Ok(value) => value,
                    Err(error) => return Ok(error),
                };
                if title.is_none() && content.is_none() {
                    return Ok(ToolResult::error("Supply a title or content to update."));
                }
                if knowledge::update_document(
                    scope.state.db(),
                    scope.workspace_id,
                    scope.user_id,
                    id,
                    DocumentUpdate { title, content },
                )
                .await?
                {
                    ToolResult::success(
                        json!({"id":id,"updated":true,"searchable":true}).to_string(),
                    )
                } else {
                    ToolResult::error(
                        "Document is unavailable, read-only, or you no longer have write permission.",
                    )
                }
            }
        };
        Ok(result)
    }
}

fn integer(params: &Value, key: &str, default: i64, minimum: i64) -> Result<i64, ToolResult> {
    match params.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_i64()
            .filter(|value| *value >= minimum)
            .ok_or_else(|| {
                ToolResult::error(format!("{key} must be an integer of at least {minimum}."))
            }),
    }
}

fn identifier(params: &Value) -> Result<Uuid, ToolResult> {
    required_text(params, "id")?
        .parse()
        .map_err(|_| ToolResult::error("id must be a valid document UUID."))
}

fn optional_text<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, ToolResult> {
    match params.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value)),
        _ => Err(ToolResult::error(format!(
            "{key} must be a non-empty string."
        ))),
    }
}

fn required_text<'a>(params: &'a Value, key: &str) -> Result<&'a str, ToolResult> {
    optional_text(params, key)?.ok_or_else(|| ToolResult::error(format!("{key} is required.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_complete_unicode_text_and_whitespace() {
        let content = format!("  {}\n", "世界 🦀 document\n".repeat(2000));
        let params = json!({"content": content});
        assert_eq!(required_text(&params, "content").unwrap(), content);
        assert!(content.chars().count() > 8000);
    }

    #[test]
    fn rejects_invalid_optional_fields_and_pagination() {
        assert!(optional_text(&json!({"title":null}), "title").is_err());
        assert!(optional_text(&json!({"content":" "}), "content").is_err());
        assert!(integer(&json!({"limit":0}), "limit", 25, 1).is_err());
        assert!(integer(&json!({"offset":1.5}), "offset", 0, 0).is_err());
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
    async fn document_tools_round_trip_complete_content() {
        use crate::db::{organizations, users, workspaces};
        use crate::state::{AppState, test_config};
        let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL"))
            .await
            .unwrap();
        let user = users::create_user(
            &pool,
            &format!("{}@example.com", Uuid::new_v4()),
            "hash",
            Some("Reader"),
            false,
        )
        .await
        .unwrap();
        let organization = organizations::create_organization(
            &pool,
            "Document tool tests",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Documents",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        workspace_members::add_member(&pool, workspace.id, user.id, WorkspaceRole::Member, None)
            .await
            .unwrap();
        let scope = WorkspaceScope {
            state: AppState::new(test_config(), pool.clone(), None),
            workspace_id: workspace.id,
            chat_id: Uuid::new_v4(),
            user_id: user.id,
        };
        let mut registry = ToolRegistry::new();
        register(&mut registry, &scope);
        let context = ToolContext::default();
        let content = format!(
            "  orbitalneedle\n{}\n ",
            "🌌 complete content\n".repeat(1500)
        );
        let created = registry
            .get("create_document")
            .unwrap()
            .execute(json!({"title":"Guide","content":content}), &context)
            .await
            .unwrap();
        assert!(created.success, "{:?}", created.error);
        let created: Value = serde_json::from_str(&created.output.unwrap()).unwrap();
        let id = &created["id"];
        let read = registry
            .get("read_document")
            .unwrap()
            .execute(json!({"id":id}), &context)
            .await
            .unwrap();
        assert!(read.success, "{:?}", read.error);
        let read: Value = serde_json::from_str(&read.output.unwrap()).unwrap();
        assert_eq!(read["document"]["content"], content);
        assert_eq!(read["complete"], true);
        let listed = registry
            .get("list_documents")
            .unwrap()
            .execute(json!({"query":"orbitalneedle"}), &context)
            .await
            .unwrap();
        assert!(listed.success, "{:?}", listed.error);
        let listed: Value = serde_json::from_str(&listed.output.unwrap()).unwrap();
        assert_eq!(listed["documents"][0]["id"], *id);
        let updated = registry
            .get("update_document")
            .unwrap()
            .execute(json!({"id":id,"content":" revisedneedle 🌌\n"}), &context)
            .await
            .unwrap();
        assert!(updated.success, "{:?}", updated.error);
        let read = registry
            .get("read_document")
            .unwrap()
            .execute(json!({"id":id}), &context)
            .await
            .unwrap();
        let read: Value = serde_json::from_str(&read.output.unwrap()).unwrap();
        assert_eq!(read["document"]["content"], " revisedneedle 🌌\n");
        workspace_members::remove_member(&pool, workspace.id, user.id)
            .await
            .unwrap();
        let denied = registry
            .get("read_document")
            .unwrap()
            .execute(json!({"id":id}), &context)
            .await
            .unwrap();
        assert!(!denied.success);
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization.id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
