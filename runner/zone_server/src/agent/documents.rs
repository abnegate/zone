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

const LIST_LIMIT: i64 = 25;
const PAGE_CHARS: u64 = 8_000;

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
                "List or search workspace notes and indexed documents. Returns stable document IDs, source, URI and freshness. Optional query searches title and full content without requiring embeddings; use read_document for stored text. Limit is capped at 25."
            }
            Operation::Read => {
                "Read stored text of a specific workspace note or indexed document by ID. Returns a Unicode character page that fits the context budget (default and maximum 8000); follow next to continue. complete is true only when this page contains the full stored text. Preserves whitespace and Unicode. Imported content is a stored snapshot; fetched_at tells when it was retrieved. Never treats absent content as a complete file."
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
                "limit":{"type":"integer","minimum":1,"maximum":25,"default":25},
                "offset":{"type":"integer","minimum":0,"default":0}
            },"additionalProperties":false}),
            Operation::Read => {
                json!({"type":"object","properties":{
                    "id":{"type":"string","format":"uuid"},
                    "offset":{"type":"integer","minimum":0,"default":0,"description":"Unicode character offset into the stored text, default 0."},
                    "limit":{"type":"integer","minimum":1,"maximum":8000,"default":8000,"description":"Number of characters to return, default 8000, capped at 8000 so the page fits the remaining context budget. Follow next to continue."}
                },"required":["id"],"additionalProperties":false})
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
                let limit = match list_limit(params) {
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
                let offset = match integer(params, "offset", 0, 0) {
                    Ok(value) => value as u64,
                    Err(error) => return Ok(error),
                };
                let limit = match read_limit(params) {
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
                    Some(mut document) => match document.content.take() {
                        Some(content) => match page_stored(&content, offset, limit) {
                            Ok((page, complete, next, total)) => {
                                document.content = Some(page);
                                ToolResult::success(
                                    json!({
                                        "document": document,
                                        "complete": complete,
                                        "content_state": "stored_text",
                                        "offset": offset,
                                        "next": next,
                                        "total": total,
                                        "observed_at": Utc::now().to_rfc3339()
                                    })
                                    .to_string(),
                                )
                            }
                            Err(error) => ToolResult::error(error),
                        },
                        None => ToolResult::success(
                            json!({
                                "document": document,
                                "complete": false,
                                "content_state": "metadata_only_content_unavailable",
                                "offset": offset,
                                "next": Value::Null,
                                "total": 0,
                                "observed_at": Utc::now().to_rfc3339()
                            })
                            .to_string(),
                        ),
                    },
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

fn list_limit(params: &Value) -> Result<i64, ToolResult> {
    Ok(integer(params, "limit", LIST_LIMIT, 1)?.min(LIST_LIMIT))
}

fn read_limit(params: &Value) -> Result<u64, ToolResult> {
    Ok(integer(params, "limit", PAGE_CHARS as i64, 1)?.min(PAGE_CHARS as i64) as u64)
}

fn text_page(content: &str, offset: u64, limit: u64) -> Result<(String, u64, Option<u64>), String> {
    let total = content.chars().count() as u64;
    if limit == 0 || offset > total {
        return Err("Document page offset or length is invalid.".into());
    }
    let count = limit.min(PAGE_CHARS).min(total.saturating_sub(offset));
    let page: String = content
        .chars()
        .skip(offset as usize)
        .take(count as usize)
        .collect();
    let end = offset + count;
    Ok((page, total, (end < total).then_some(end)))
}

fn page_stored(
    content: &str,
    offset: u64,
    limit: u64,
) -> Result<(String, bool, Option<u64>, u64), String> {
    let (page, total, next) = text_page(content, offset, limit)?;
    Ok((page, next.is_none() && offset == 0, next, total))
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
        assert!(content.chars().count() as u64 > PAGE_CHARS);
    }

    #[test]
    fn large_document_is_paged_and_follows_next() {
        let content = format!("{}X", "世界".repeat(PAGE_CHARS as usize / 2));
        assert_eq!(content.chars().count() as u64, PAGE_CHARS + 1);
        let (page, complete, next, total) = page_stored(&content, 0, 1_000_000).unwrap();
        assert!(!complete);
        assert_eq!(page.chars().count() as u64, PAGE_CHARS);
        assert_eq!(next, Some(PAGE_CHARS));
        assert_eq!(total, PAGE_CHARS + 1);
        let (rest, last_complete, last_next, _) =
            page_stored(&content, next.unwrap(), PAGE_CHARS).unwrap();
        assert!(!last_complete);
        assert_eq!(rest, "X");
        assert_eq!(last_next, None);
        assert_eq!(format!("{page}{rest}"), content);
    }

    #[test]
    fn small_document_is_complete_in_one_page() {
        let (page, complete, next, total) = page_stored("short 🦀 note", 0, PAGE_CHARS).unwrap();
        assert!(complete);
        assert_eq!(page, "short 🦀 note");
        assert_eq!(next, None);
        assert_eq!(total, "short 🦀 note".chars().count() as u64);
    }

    #[test]
    fn list_documents_requested_limit_is_clamped() {
        assert_eq!(list_limit(&json!({"limit": 10_000})).unwrap(), LIST_LIMIT);
        assert_eq!(list_limit(&json!({"limit": 3})).unwrap(), 3);
        assert_eq!(list_limit(&json!({})).unwrap(), LIST_LIMIT);
        assert_eq!(
            read_limit(&json!({"limit": 1_000_000})).unwrap(),
            PAGE_CHARS
        );
    }

    #[test]
    fn rejects_invalid_optional_fields_and_pagination() {
        assert!(optional_text(&json!({"title":null}), "title").is_err());
        assert!(optional_text(&json!({"content":" "}), "content").is_err());
        assert!(integer(&json!({"limit":0}), "limit", 25, 1).is_err());
        assert!(integer(&json!({"offset":1.5}), "offset", 0, 0).is_err());
        assert!(page_stored("hello", 0, 0).is_err());
        assert!(page_stored("hello", 6, 1).is_err());
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
            chat_id: Some(Uuid::new_v4()),
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
        let mut assembled = String::new();
        let mut offset = 0u64;
        loop {
            let read = registry
                .get("read_document")
                .unwrap()
                .execute(json!({"id":id,"offset":offset}), &context)
                .await
                .unwrap();
            assert!(read.success, "{:?}", read.error);
            let read: Value = serde_json::from_str(&read.output.unwrap()).unwrap();
            assembled.push_str(read["document"]["content"].as_str().unwrap());
            let next = read["next"].as_u64();
            assert_eq!(read["complete"], next.is_none() && offset == 0);
            match next {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(assembled, content);
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
        assert_eq!(read["complete"], true);
        assert_eq!(read["next"], Value::Null);
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
