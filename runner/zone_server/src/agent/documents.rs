//! Workspace-scoped document reads and persistent, searchable notes.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
use zone_core::tools::{
    MAX_TOOL_OUTPUT_CHARS, Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult, excerpt,
};

use super::PREVIEW_TITLE_CHARS;
use super::identifier::Kind;
use super::tools::WorkspaceScope;
use crate::db::knowledge::{self, Document, DocumentUpdate};
use crate::db::workspace_members::{self, WorkspaceRole};
use crate::db::{DbResult, chat_sources};

const LIST_LIMIT: i64 = 25;
const PAGE_CHARS: u64 = 8_000;

/// Where a document's registry identifier is rendered: inline on the record,
/// under the name a citation carries it under.
///
/// Never a trailing array. These envelopes are trimmed by whole records, so an
/// identifier beside its document goes when the document goes, where a list at
/// the end would outlive the record it names and leave the model a marker for
/// a document that is no longer in front of it.
const IDENTIFIER: &str = "identifier";

const DOCUMENT: &str = "document";
const DOCUMENTS: &str = "documents";
const OMITTED: &str = "documents_omitted";
const NOTE: &str = "note";
const STORED_TEXT: &str = "stored_text";
const METADATA_ONLY: &str = "metadata_only_content_unavailable";
const NOT_FOUND: &str = "Document not found in this workspace.";

/// The model is taught that a source arrives with a bracketed identifier, and
/// a JSON record carries a bare one, so the envelope says how to write it.
const CITE_NOTE: &str = "Cite a document by its identifier in brackets, such as [doc:6a1f2c]. A \
                         document with no identifier cannot be cited.";

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

    /// A workspace document is read by people who were not in this chat, so
    /// writing one publishes on their behalf whether it is new or a revision.
    fn tier(&self) -> Tier {
        match self.operation {
            Operation::Create | Operation::Update => Tier::Outward,
            Operation::List | Operation::Read => Tier::Read,
        }
    }

    fn preview(&self, params: &Value) -> Option<String> {
        let title = params["title"].as_str();
        let characters = params["content"].as_str().map(|body| body.chars().count());
        match self.operation {
            Operation::Create => Some(format!(
                "Publish a workspace document titled \"{}\", {} characters long.",
                excerpt(title?, PREVIEW_TITLE_CHARS),
                characters?
            )),
            Operation::Update => {
                let target = params["id"].as_str().unwrap_or("an unnamed document");
                Some(match (title, characters) {
                    (Some(title), Some(characters)) => format!(
                        "Retitle workspace document {target} to \"{}\" and replace its text with {characters} characters.",
                        excerpt(title, PREVIEW_TITLE_CHARS)
                    ),
                    (Some(title), None) => format!(
                        "Retitle workspace document {target} to \"{}\".",
                        excerpt(title, PREVIEW_TITLE_CHARS)
                    ),
                    (None, Some(characters)) => format!(
                        "Replace the text of workspace document {target} with {characters} characters."
                    ),
                    (None, None) => return None,
                })
            }
            _ => None,
        }
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
                let records = self.identify_all(&documents).await;
                ToolResult::success(
                    listing(
                        json!({
                            "offset": offset,
                            "limit": limit,
                            "observed_at": Utc::now().to_rfc3339()
                        }),
                        &records,
                    )
                    .to_string(),
                )
            }
            Operation::Read => {
                let id = match document_id(params) {
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
                let Some(mut document) = knowledge::read_document(
                    scope.state.db(),
                    scope.workspace_id,
                    scope.user_id,
                    id,
                )
                .await?
                else {
                    return Ok(ToolResult::error(NOT_FOUND));
                };
                let page = match document.content.take() {
                    Some(content) => match page_stored(&content, offset, limit) {
                        Ok((page, complete, next, total)) => {
                            document.content = Some(page);
                            Page {
                                complete,
                                state: STORED_TEXT,
                                next,
                                total,
                            }
                        }
                        Err(error) => return Ok(ToolResult::error(error)),
                    },
                    None => Page {
                        complete: false,
                        state: METADATA_ONLY,
                        next: None,
                        total: 0,
                    },
                };
                let record = self.identify(&document).await;
                ToolResult::success(
                    reading(
                        json!({
                            "complete": page.complete,
                            "content_state": page.state,
                            "offset": offset,
                            "next": page.next,
                            "total": page.total,
                            "observed_at": Utc::now().to_rfc3339()
                        }),
                        record,
                    )
                    .to_string(),
                )
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
                let id = match document_id(params) {
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

    /// Register a document against the chat that read it, so the model can
    /// cite it by an identifier the server can prove it retrieved.
    ///
    /// The document's own URI is what is hashed, verbatim. It is stable across
    /// turns where a title or a position in a listing is not, which is what
    /// makes reading the same document twice produce one citation rather than
    /// two. A document with no URI is left bare: there is nothing to hash, and
    /// a citation with no url is dropped downstream anyway.
    ///
    /// A task run has no chat and mints nothing. A per-chat identifier written
    /// into another chat's registry would let one conversation cite a document
    /// it never read.
    async fn identify(&self, document: &Document) -> Value {
        let mut record = json!(document);
        let Some(chat) = self.scope.chat_id else {
            return record;
        };
        if document.uri.is_empty() {
            return record;
        }
        let observed = chat_sources::observe(
            self.scope.state.db(),
            chat,
            Kind::Doc,
            &document.uri,
            &document.title,
        )
        .await;
        stamp(&mut record, chat, &document.uri, observed);
        record
    }

    async fn identify_all(&self, documents: &[Document]) -> Vec<Value> {
        let mut records = Vec::with_capacity(documents.len());
        for document in documents {
            records.push(self.identify(document).await);
        }
        records
    }
}

/// What a read hands back about the page it returned.
struct Page {
    complete: bool,
    state: &'static str,
    next: Option<u64>,
    total: u64,
}

/// Only the write knows the identifier, because the registry extends a digest
/// whose prefix another URI already holds. A failed write leaves that one
/// document bare rather than rendering a marker that could never resolve.
fn stamp(record: &mut Value, chat: Uuid, uri: &str, observed: DbResult<chat_sources::Source>) {
    match observed {
        Ok(source) => record[IDENTIFIER] = json!(source.identifier),
        Err(error) => tracing::warn!(
            %error,
            %chat,
            %uri,
            "Could not register a workspace document; citing it without an identifier"
        ),
    }
}

/// List the documents, trimming them until the whole envelope fits the output
/// budget.
///
/// Cutting the serialised envelope to length instead lands the cut inside the
/// JSON: the model reads a string that no longer parses, and every citation
/// derived from this output downstream is lost with it. Whole records go
/// instead, so a document that no longer fits takes its identifier and its
/// citation with it. Rows arrive ranked, so the cut keeps the head.
fn listing(mut envelope: Value, records: &[Value]) -> Value {
    let mut kept = records.len();
    loop {
        let listed = &records[..kept];
        envelope[DOCUMENTS] = Value::Array(listed.to_vec());
        if kept < records.len() {
            envelope[OMITTED] = json!(records.len() - kept);
        }
        cite(&mut envelope, listed);
        if json_chars(&envelope) <= MAX_TOOL_OUTPUT_CHARS || kept <= 1 {
            return envelope;
        }
        kept -= 1;
    }
}

fn reading(mut envelope: Value, record: Value) -> Value {
    cite(&mut envelope, std::slice::from_ref(&record));
    envelope[DOCUMENT] = record;
    envelope
}

/// Tell the model how to write a marker, but only where it has one to write.
/// Trimming can take the last identified record, and an instruction to cite
/// what is left would then be an invitation to invent a marker.
fn cite(envelope: &mut Value, records: &[Value]) {
    if records.iter().any(identified) {
        envelope[NOTE] = json!(CITE_NOTE);
    } else if let Some(object) = envelope.as_object_mut() {
        object.remove(NOTE);
    }
}

fn identified(record: &Value) -> bool {
    record.get(IDENTIFIER).is_some()
}

fn json_chars(value: &Value) -> usize {
    value.to_string().chars().count()
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

fn document_id(params: &Value) -> Result<Uuid, ToolResult> {
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
    use crate::agent::citations::{self, CitationKind};
    use crate::agent::identifier;
    use crate::state::{AppState, test_config};
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    const URI: &str = "knowledge://0f9e8d7c-6b5a-4d3e-8f1a-2b3c4d5e6f70";
    const TITLE: &str = "Deployment guide";
    const OBSERVED: &str = "2026-09-05T00:00:00+00:00";
    const READ_TOOL: &str = "read_document";
    const LIST_TOOL: &str = "list_documents";

    /// Bounds the wait on a registry that never answers, so a failed write
    /// costs a test milliseconds rather than the default acquire timeout.
    const REGISTRY_TIMEOUT: Duration = Duration::from_millis(250);

    /// How long a connection the registry already holds may take to surface.
    const ACCEPT_TIMEOUT: Duration = Duration::from_millis(250);

    fn tool(chat: Option<Uuid>, registry: u16) -> DocumentTool {
        let database = PgPoolOptions::new()
            .acquire_timeout(REGISTRY_TIMEOUT)
            .connect_lazy(&format!("postgres://127.0.0.1:{registry}/zone"))
            .expect("a lazy pool needs no server");
        DocumentTool {
            scope: WorkspaceScope {
                state: AppState::new(test_config(), database, None),
                workspace_id: Uuid::new_v4(),
                chat_id: chat,
                user_id: Uuid::new_v4(),
            },
            operation: Operation::Read,
        }
    }

    fn document(uri: &str, title: &str) -> Document {
        Document {
            id: Uuid::new_v4(),
            title: title.to_string(),
            content: None,
            source: "knowledge".to_string(),
            source_id: None,
            uri: uri.to_string(),
            updated_at: None,
            fetched_at: None,
            editable: true,
            revision: Some("2f6a1b".to_string()),
        }
    }

    fn registered(document: &Document, identifier: &str) -> chat_sources::Source {
        let observed = Utc::now();
        chat_sources::Source {
            chat_id: Uuid::new_v4(),
            identifier: identifier.to_string(),
            kind: Kind::Doc,
            uri: document.uri.clone(),
            title: document.title.clone(),
            first_observed_at: observed,
            last_observed_at: observed,
        }
    }

    /// More documents than one envelope can hold, so a listing of them trims.
    fn oversized() -> Vec<Document> {
        (0..LIST_LIMIT)
            .map(|index| {
                document(
                    &format!("knowledge://{index}-{}", "e4b1c9a7".repeat(12)),
                    &format!("{index} {}", TITLE.repeat(8)),
                )
            })
            .collect()
    }

    fn identified_record(document: &Document, identifier: &str) -> Value {
        let mut record = json!(document);
        stamp(
            &mut record,
            Uuid::new_v4(),
            &document.uri,
            Ok(registered(document, identifier)),
        );
        record
    }

    /// The registry owns the identifier: a digest whose prefix another URI
    /// already holds is extended by the write, so anything minted here could be
    /// stale before it is rendered.
    #[test]
    fn a_document_carries_the_identifier_the_registry_returned() {
        let document = document(URI, TITLE);
        let minted = identifier::mint(Kind::Doc, &document.uri);
        let extended =
            identifier::extend(&minted, &document.uri).expect("a minted identifier extends");

        let record = identified_record(&document, &extended);

        assert_eq!(record[IDENTIFIER], json!(extended));
        assert_ne!(
            record[IDENTIFIER],
            json!(minted),
            "the record carries a locally minted identifier rather than the one the registry wrote"
        );
        assert!(
            record[IDENTIFIER]
                .as_str()
                .is_some_and(|identifier| { identifier.starts_with(&format!("{}:", Kind::Doc)) }),
            "a document is cited as something other than a document: {record}"
        );
    }

    /// The record the model reads and the citation a marker resolves to are the
    /// same registry row, so a marker written against a document lands on the
    /// citation beside it rather than on a second, differently named copy of
    /// the same source.
    #[test]
    fn a_documents_citation_carries_the_identifier_the_document_does() {
        let document = document(URI, TITLE);
        let source = registered(&document, &identifier::mint(Kind::Doc, &document.uri));
        let record = identified_record(&document, &source.identifier);

        let citation = citations::from_source(
            CitationKind::WorkspaceDocument,
            &source.identifier,
            &source.title,
            &source.uri,
            source.first_observed_at,
        );

        assert_eq!(
            record[IDENTIFIER],
            json!(
                citation
                    .identifier
                    .expect("a cited source carries its identifier")
            )
        );
        assert_eq!(record["uri"], json!(citation.url));
        assert_eq!(record["title"], json!(citation.title));
    }

    /// An identifier the write never produced would resolve to nothing, leaving
    /// the reader an inert marker for a document that genuinely exists. The
    /// registry here accepts the connection and answers nothing, so the write
    /// fails after it was unmistakably attempted.
    #[tokio::test]
    async fn a_failed_registry_write_leaves_the_document_bare_and_the_turn_intact() {
        let registry = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a registry that never answers still needs a port");
        let port = registry.local_addr().expect("a bound port").port();
        let document = document(URI, TITLE);

        let record = tool(Some(Uuid::new_v4()), port).identify(&document).await;

        assert!(
            timeout(ACCEPT_TIMEOUT, registry.accept()).await.is_ok(),
            "the read never reached the chat's source registry"
        );
        assert!(
            !identified(&record),
            "a document the registry never accepted must render bare: {record}"
        );
        let envelope = reading(json!({"complete": true, "observed_at": OBSERVED}), record);
        assert_eq!(
            envelope.get(NOTE),
            None,
            "the envelope asks for a marker no document in it can supply: {envelope}"
        );
        let citations = citations::from_tool_at(READ_TOOL, &envelope.to_string(), OBSERVED);
        assert_eq!(
            citations.len(),
            1,
            "a failed registry write cost the turn its citation: {citations:?}"
        );
        assert_eq!(citations[0].url, URI);
    }

    /// A background task run has no chat, and the registry is per-chat exactly
    /// so a citation names something this conversation retrieved. Minting into
    /// any other chat's registry would break that, so a run without a chat
    /// reaches no registry at all.
    #[tokio::test]
    async fn a_run_without_a_chat_mints_nothing() {
        let registry = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a registry no run should reach still needs a port");
        let port = registry.local_addr().expect("a bound port").port();
        let documents = [
            document(URI, TITLE),
            document(
                "knowledge://8c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f",
                "Runbook",
            ),
        ];

        let records = tool(None, port).identify_all(&documents).await;

        assert!(
            timeout(ACCEPT_TIMEOUT, registry.accept()).await.is_err(),
            "a run with no chat wrote into some other chat's registry"
        );
        assert!(
            !records.iter().any(identified),
            "a run with no chat minted an identifier: {records:?}"
        );
        assert_eq!(
            listing(json!({"observed_at": OBSERVED}), &records).get(NOTE),
            None,
            "a run with no chat is told to cite identifiers it was never given"
        );
    }

    /// A document that no longer fits the envelope takes its identifier and its
    /// citation with it. The model must not read a marker for a document that
    /// is no longer in front of it, and the reply must not carry a citation for
    /// one either.
    #[test]
    fn a_trimmed_document_takes_its_identifier_and_citation_with_it() {
        let documents = oversized();
        let records: Vec<Value> = documents
            .iter()
            .map(|document| {
                identified_record(document, &identifier::mint(Kind::Doc, &document.uri))
            })
            .collect();
        let base = json!({"offset": 0, "limit": LIST_LIMIT, "observed_at": OBSERVED});

        let envelope = listing(base.clone(), &records);

        let kept = envelope[DOCUMENTS]
            .as_array()
            .expect("a listing carries its documents")
            .len();
        let omitted = envelope[OMITTED]
            .as_u64()
            .expect("a trimmed listing says how many documents it dropped")
            as usize;
        let mut untrimmed = base;
        untrimmed[DOCUMENTS] = json!(records);
        assert!(
            json_chars(&untrimmed) > MAX_TOOL_OUTPUT_CHARS,
            "the fixture fits the budget whole, so nothing here is ever trimmed"
        );
        assert!(omitted > 0 && kept > 0, "{kept} kept, {omitted} omitted");
        assert_eq!(kept + omitted, records.len());
        assert!(
            json_chars(&envelope) <= MAX_TOOL_OUTPUT_CHARS,
            "a trimmed listing is still over the output budget at {} characters",
            json_chars(&envelope)
        );

        let rendered = envelope.to_string();
        let citations = citations::from_tool_at(LIST_TOOL, &rendered, OBSERVED);
        assert_eq!(
            citations.len(),
            kept,
            "the trimmed envelope no longer parses, so every citation was lost: {rendered}"
        );
        for dropped in &documents[kept..] {
            let identifier = identifier::mint(Kind::Doc, &dropped.uri);
            assert!(
                !rendered.contains(&identifier),
                "{identifier} outlived the record it named"
            );
            assert!(
                !citations.iter().any(|citation| citation.url == dropped.uri),
                "a document trimmed out of the envelope kept its citation"
            );
        }
        for surviving in &documents[..kept] {
            let identifier = identifier::mint(Kind::Doc, &surviving.uri);
            assert!(
                rendered.contains(&identifier),
                "{identifier} was trimmed off a document that survived"
            );
            assert!(
                citations
                    .iter()
                    .any(|citation| citation.url == surviving.uri),
                "a document still in the envelope lost its citation"
            );
        }
    }

    /// Trimming can take the last document that carried an identifier. What is
    /// left is uncitable, and an envelope that went on asking for a marker
    /// would be asking the model to invent one.
    #[test]
    fn a_listing_that_trims_away_every_identified_document_asks_for_no_marker() {
        let documents = oversized();
        let identified_from = documents.len() - 5;
        let records: Vec<Value> = documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                if index < identified_from {
                    json!(document)
                } else {
                    identified_record(document, &identifier::mint(Kind::Doc, &document.uri))
                }
            })
            .collect();

        let envelope = listing(json!({"observed_at": OBSERVED}), &records);

        let listed = envelope[DOCUMENTS]
            .as_array()
            .expect("a listing carries its documents");
        assert!(
            listed.len() < identified_from,
            "the fixture kept an identified document, so nothing here trims one away"
        );
        assert!(!listed.iter().any(identified));
        assert_eq!(
            envelope.get(NOTE),
            None,
            "the envelope asks for a marker no document left in it can supply: {envelope}"
        );
    }

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
