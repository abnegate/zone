//! Memory belongs to one person, and `knowledge_entries` has no per-user scoping.
//!
//! `created_by` is written by three inserts and read by nothing: no reader
//! selects it, no predicate filters on it, and `KnowledgeRow` does not expose
//! it. So a row stored under a private category is, before the predicate every
//! query here exercises, an ordinary workspace entry -- listed, searched,
//! readable by id, editable by any member, and offered to the embedder by the
//! recovery pass.
//!
//! Each test names the path it closes and carries its own control: an ordinary
//! document the same call still returns, so an assertion cannot pass because
//! the query stopped working.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use common::context::{Harness, answer, usage};
use common::{TestClient, test_email, test_password};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_context::embeddings::EmbeddingService;
use zone_context::error::Result as ContextResult;
use zone_server::agent::ChatTools;
use zone_server::db::knowledge::{self, DocumentUpdate, NOT_MEMORY, PRIVATE_CATEGORIES};
use zone_server::db::memory::{
    FACT_CATEGORY, MEMORY_CATEGORY_PREFIX, PREFERENCE_CATEGORY, PROFILE_CATEGORY, PROFILE_TITLE,
};
use zone_server::db::workspace_members::{WorkspaceRole, add_member};
use zone_server::workers::knowledge_refresh;

/// A word that appears only in the memory entry.
const MEMORY_ONLY: &str = "barnholomew";
/// A word the memory entry and an ordinary document share, so a search that
/// returns neither is a broken search rather than a closed path.
const SHARED: &str = "zolgatrine";

const MEMORY_CONTENT: &str = "Answers to Barnholomew. Drinks zolgatrine coffee, never decaf.";
const DOCUMENT_TITLE: &str = "Office orders";
const DOCUMENT_CONTENT: &str = "The zolgatrine roast is ordered by the case for the office.";

/// An embedder that always answers, so the only reason an entry stays out of
/// the index is the predicate under test.
struct Embedding;

#[async_trait]
impl EmbeddingService for Embedding {
    async fn embed(&self, text: &str) -> ContextResult<Vec<f32>> {
        let seed = text.len() as f32;
        Ok((0..32).map(|slot| (seed + slot as f32) / 1024.0).collect())
    }

    async fn embed_batch(&self, texts: &[&str]) -> ContextResult<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            vectors.push(self.embed(text).await?);
        }
        Ok(vectors)
    }

    fn dimension(&self) -> usize {
        32
    }

    fn model(&self) -> &str {
        "test-embedder"
    }
}

/// One workspace, two members, one memory row owned by the first, and one
/// ordinary document. The document is created through `create_document`, which
/// writes no category at all -- the `category IS NULL` half of the predicate is
/// what keeps it readable.
struct Fixture {
    client: TestClient,
    workspace: Uuid,
    owner: Uuid,
    owner_token: String,
    other: Uuid,
    other_token: String,
    memory: Uuid,
    /// A memory kind inside the prefix that this build has no name for, which
    /// is what every read-path predicate matches on and what the routes have to
    /// match on too.
    later: Uuid,
    document: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        Self::around(TestClient::with_db().await).await
    }

    async fn embedding() -> Self {
        Self::around(TestClient::with_embedding(Arc::new(Embedding)).await).await
    }

    async fn around(client: TestClient) -> Self {
        let (owner_token, owner) = register(&client).await;
        let (other_token, other) = register(&client).await;
        let workspace = workspace_for(&client, &owner_token).await;
        add_member(
            client.state().db(),
            workspace,
            other,
            WorkspaceRole::Member,
            None,
        )
        .await
        .expect("the second person joins the workspace");

        let memory = seed_memory(
            client.state().db(),
            workspace,
            owner,
            PROFILE_CATEGORY,
            PROFILE_TITLE,
            MEMORY_CONTENT,
        )
        .await;
        let later = seed_memory(
            client.state().db(),
            workspace,
            owner,
            &format!("{MEMORY_CATEGORY_PREFIX}something-later"),
            "Later",
            MEMORY_CONTENT,
        )
        .await;
        let document = knowledge::create_document(
            client.state().db(),
            workspace,
            owner,
            DOCUMENT_TITLE,
            DOCUMENT_CONTENT,
        )
        .await
        .expect("creating an ordinary document")
        .expect("the owner may write in its own workspace");

        Self {
            client,
            workspace,
            owner,
            owner_token,
            other,
            other_token,
            memory,
            later,
            document,
        }
    }

    fn pool(&self) -> &PgPool {
        self.client.state().db()
    }

    /// Every organization this fixture opened, closed again: its own and the
    /// personal one registration minted for each person. `knowledge_entries`
    /// hangs off the workspace and the workspace off the organization, so the
    /// memory row and the document go with them.
    async fn clean(self) {
        common::discard(self.pool(), self.workspace, &[self.owner, self.other]).await;
    }
}

async fn register(client: &TestClient) -> (String, Uuid) {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
                "display_name": "Isolation Tester"
            }),
        )
        .await;
    let body = registered.json_value();
    let token = body["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let user = Uuid::parse_str(
        body["user"]["id"]
            .as_str()
            .expect("registration returns the user"),
    )
    .expect("user id is a uuid");
    (token, user)
}

async fn workspace_for(client: &TestClient, token: &str) -> Uuid {
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": format!("Isolation Org {}", Uuid::new_v4()),
                "slug": format!("isolation-org-{}", Uuid::new_v4())
            }),
            token,
        )
        .await;
    let organization = organization.json_value()["organization"]["id"]
        .as_str()
        .expect("organization id")
        .to_string();

    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({
                "name": format!("Isolation Workspace {}", Uuid::new_v4()),
                "slug": format!("isolation-ws-{}", Uuid::new_v4())
            }),
            token,
        )
        .await;
    Uuid::parse_str(
        workspace.json_value()["workspace"]["id"]
            .as_str()
            .expect("workspace id"),
    )
    .expect("workspace id is a uuid")
}

/// The store that writes these rows is a parallel subtask on this base, so the
/// row is seeded the way that store will write it: a private category, an
/// owner in `created_by`, and no `source_url` -- which is exactly what makes it
/// reachable from `update_document` and `editable` through the document union.
async fn seed_memory(
    pool: &PgPool,
    workspace: Uuid,
    owner: Uuid,
    category: &str,
    title: &str,
    content: &str,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO knowledge_entries (workspace_id, title, content, category, token_count, created_by)
         VALUES ($1, $2, $3, $4, ceil(length($3::text)::numeric / 4)::integer, $5)
         RETURNING id",
    )
    .bind(workspace)
    .bind(title)
    .bind(content)
    .bind(category)
    .bind(owner)
    .fetch_one(pool)
    .await
    .expect("seeding a memory row")
}

#[derive(Debug, PartialEq, Eq)]
struct Stored {
    title: String,
    content: String,
    is_active: bool,
    version: Option<i64>,
    vectors: i64,
}

/// `version` arrives with the memory migration, which is a parallel subtask on
/// this base. Probing for the column keeps the assertion honest either way:
/// the counter a document writer must never move is read once it exists, and
/// its absence is not mistaken for an unchanged value.
async fn stored(pool: &PgPool, id: Uuid) -> Stored {
    let (title, content, is_active) = sqlx::query_as::<_, (String, String, bool)>(
        "SELECT title, content, is_active FROM knowledge_entries WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("reading the stored row");

    let versioned: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns
         WHERE table_name = 'knowledge_entries' AND column_name = 'version')",
    )
    .fetch_one(pool)
    .await
    .expect("probing for the version column");
    let version = if versioned {
        Some(
            sqlx::query_scalar("SELECT version FROM knowledge_entries WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .expect("reading the version"),
        )
    } else {
        None
    };

    let vectors = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_embeddings WHERE knowledge_entry_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("counting stored vectors");

    Stored {
        title,
        content,
        is_active,
        version,
        vectors,
    }
}

/// The recovery scan is server-wide and the test database is shared, so pull
/// the rows under test to the front of its `created_at ASC` queue.
async fn sort_first(pool: &PgPool, ids: &[Uuid]) {
    sqlx::query(
        "UPDATE knowledge_entries SET created_at = '1970-01-01 00:00:00'::timestamp
         WHERE id = ANY($1)",
    )
    .bind(ids)
    .execute(pool)
    .await
    .expect("backdating the rows under test");
}

#[tokio::test]
async fn the_workspace_list_skips_memory_under_every_category_it_is_asked_for() {
    let fixture = Fixture::new().await;

    let all = knowledge::list_knowledge(fixture.pool(), fixture.workspace, None, 200, 0)
        .await
        .expect("listing a workspace");
    assert!(
        all.iter().any(|row| row.id == fixture.document),
        "an ordinary document has no category, and the predicate must still return it"
    );
    assert!(
        !all.iter().any(|row| row.id == fixture.memory),
        "the unfiltered branch of the workspace list reached a memory row"
    );

    for category in [PROFILE_CATEGORY, PREFERENCE_CATEGORY, FACT_CATEGORY] {
        let asked =
            knowledge::list_knowledge(fixture.pool(), fixture.workspace, Some(category), 200, 0)
                .await
                .expect("listing one category");
        assert!(
            asked.is_empty(),
            "asking the workspace list for {category} by name answered with {} rows",
            asked.len()
        );
    }

    fixture.clean().await;
}

#[tokio::test]
async fn keyword_search_returns_the_workspace_entry_and_never_the_memory_row() {
    let fixture = Fixture::new().await;

    let shared = knowledge::search_knowledge_keyword(fixture.pool(), SHARED, fixture.workspace, 50)
        .await
        .expect("keyword search");
    assert!(
        shared.iter().any(|hit| hit.entry_id == fixture.document),
        "the ordinary document holds {SHARED} and has to be found, or this test proves nothing"
    );
    assert!(
        !shared.iter().any(|hit| hit.entry_id == fixture.memory),
        "keyword search returned a memory row for a word it shares with a document"
    );

    let private =
        knowledge::search_knowledge_keyword(fixture.pool(), MEMORY_ONLY, fixture.workspace, 50)
            .await
            .expect("keyword search");
    assert!(
        private.is_empty(),
        "{MEMORY_ONLY} appears only inside memory, so keyword search must answer with nothing"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn the_scan_for_unembedded_entries_does_not_offer_memory_to_the_embedder() {
    let fixture = Fixture::new().await;
    sort_first(fixture.pool(), &[fixture.memory, fixture.document]).await;

    let missing = knowledge::list_entries_missing_embeddings(fixture.pool(), 500, 0)
        .await
        .expect("scanning for entries with no vector");

    assert!(
        missing.iter().any(|entry| entry.id == fixture.document),
        "a genuinely unembedded document is what this scan exists to find"
    );
    assert!(
        !missing.iter().any(|entry| entry.id == fixture.memory),
        "embedding memory would put it into the vector path this change leaves alone"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn reading_a_document_by_id_misses_memory_for_the_owner_and_for_another_member() {
    let fixture = Fixture::new().await;

    let readable = knowledge::read_document(
        fixture.pool(),
        fixture.workspace,
        fixture.owner,
        fixture.document,
    )
    .await
    .expect("reading an ordinary document");
    assert!(
        readable.is_some_and(|document| document.content.as_deref() == Some(DOCUMENT_CONTENT)),
        "the document union still has to return an ordinary document in full"
    );

    for reader in [fixture.owner, fixture.other] {
        let found =
            knowledge::read_document(fixture.pool(), fixture.workspace, reader, fixture.memory)
                .await
                .expect("reading by id");
        assert!(
            found.is_none(),
            "this route is the workspace's, so the owner is refused with everybody else"
        );
    }

    fixture.clean().await;
}

#[tokio::test]
async fn listing_and_searching_documents_never_surfaces_memory() {
    let fixture = Fixture::new().await;

    let listed = knowledge::list_documents(
        fixture.pool(),
        fixture.workspace,
        fixture.owner,
        None,
        200,
        0,
    )
    .await
    .expect("listing documents");
    assert!(
        listed
            .iter()
            .any(|document| document.id == fixture.document),
        "the ordinary document has to be listed"
    );
    assert!(
        !listed.iter().any(|document| document.id == fixture.memory),
        "a memory row was listed as a document, and it advertises itself as editable"
    );

    let shared = knowledge::list_documents(
        fixture.pool(),
        fixture.workspace,
        fixture.owner,
        Some(SHARED),
        200,
        0,
    )
    .await
    .expect("searching documents");
    assert!(
        shared
            .iter()
            .any(|document| document.id == fixture.document),
        "document search builds its own to_tsvector, so it has to find {SHARED}"
    );
    assert!(
        !shared.iter().any(|document| document.id == fixture.memory),
        "document search reached a memory row"
    );

    let private = knowledge::list_documents(
        fixture.pool(),
        fixture.workspace,
        fixture.owner,
        Some(MEMORY_ONLY),
        200,
        0,
    )
    .await
    .expect("searching documents");
    assert!(
        private.is_empty(),
        "{MEMORY_ONLY} is only in memory, and document search answered with {} rows",
        private.len()
    );

    fixture.clean().await;
}

/// `documents::register` sits outside the chat gate, so a background run whose
/// initiating actor is a workspace writer resolves `read_document`. That is the
/// path that made this the widest of the reads, and it is closed in SQL rather
/// than by withholding the tool.
#[tokio::test]
async fn a_task_tool_set_built_for_a_writer_cannot_read_memory_through_read_document() {
    let fixture = Fixture::new().await;
    let tools = ChatTools::for_task(
        fixture.client.state(),
        std::env::temp_dir(),
        fixture.workspace,
        Some(fixture.owner),
    )
    .await;

    assert!(
        tools.has("read_document"),
        "a task run with an initiating writer resolves the document tools"
    );

    let reachable = tools
        .execute(
            "read_document",
            &json!({"id": fixture.document}).to_string(),
        )
        .await;
    assert!(
        reachable.success,
        "an ordinary document still has to be readable from a task run: {:?}",
        reachable.error
    );

    let refused = tools
        .execute("read_document", &json!({"id": fixture.memory}).to_string())
        .await;
    assert!(
        !refused.success,
        "a task run read a memory entry: {:?}",
        refused.output
    );
    let answered = format!("{:?}{:?}", refused.output, refused.error);
    assert!(
        !answered.contains(MEMORY_ONLY) && !answered.contains(SHARED),
        "the refusal carried memory content back to the model: {answered}"
    );

    fixture.clean().await;
}

/// `update_document`'s predicate is exactly `source_url IS NULL`, which every
/// memory row satisfies, and it does not touch `version` -- so before this
/// change one member could overwrite another's profile and the compare-and-set
/// the memory writer relies on would never see it happen.
#[tokio::test]
async fn the_document_writer_cannot_overwrite_memory_for_anyone() {
    let fixture = Fixture::new().await;
    let before = stored(fixture.pool(), fixture.memory).await;

    for writer in [fixture.owner, fixture.other] {
        let changed = knowledge::update_document(
            fixture.pool(),
            fixture.workspace,
            writer,
            fixture.memory,
            DocumentUpdate {
                title: Some("Overwritten"),
                content: Some("Answers to nothing at all."),
            },
        )
        .await
        .expect("attempting a document write");
        assert!(
            !changed,
            "the document writer reported changing a memory row for {writer}"
        );
    }

    assert_eq!(
        stored(fixture.pool(), fixture.memory).await,
        before,
        "title, content, is_active, version and the absent vector all have to be untouched"
    );

    let changed = knowledge::update_document(
        fixture.pool(),
        fixture.workspace,
        fixture.owner,
        fixture.document,
        DocumentUpdate {
            title: Some("Office orders, revised"),
            content: None,
        },
    )
    .await
    .expect("attempting a document write");
    assert!(
        changed,
        "the predicate must not stop a member editing an ordinary note"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn the_knowledge_list_route_omits_memory_for_another_member_and_for_the_owner() {
    let fixture = Fixture::new().await;

    for token in [&fixture.owner_token, &fixture.other_token] {
        let response = fixture
            .client
            .get_auth(
                &format!("/api/knowledge?workspace_id={}", fixture.workspace),
                token,
            )
            .await;
        response.assert_status(StatusCode::OK);
        let rows = response.json_value();
        let rows = rows.as_array().expect("the list route answers an array");
        assert!(
            rows.iter()
                .any(|row| row["id"].as_str() == Some(&fixture.document.to_string())),
            "the ordinary document has to be listed"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row["id"].as_str() == Some(&fixture.memory.to_string())),
            "the list route answered with a memory row: {rows:?}"
        );
    }

    fixture.clean().await;
}

#[tokio::test]
async fn reading_a_memory_entry_by_id_is_not_found_for_another_member_and_for_the_owner() {
    let fixture = Fixture::new().await;

    let readable = fixture
        .client
        .get_auth(
            &format!("/api/knowledge/{}", fixture.document),
            &fixture.owner_token,
        )
        .await;
    readable.assert_status(StatusCode::OK);

    for token in [&fixture.owner_token, &fixture.other_token] {
        for private in [fixture.memory, fixture.later] {
            let response = fixture
                .client
                .get_auth(&format!("/api/knowledge/{private}"), token)
                .await;
            assert_eq!(
                response.status,
                StatusCode::NOT_FOUND,
                "a member must learn nothing from the refusal, not even that the id exists: {}",
                response.text()
            );
        }
    }

    fixture.clean().await;
}

#[tokio::test]
async fn deleting_a_memory_entry_is_not_found_for_another_member_and_for_the_owner() {
    let fixture = Fixture::new().await;

    for token in [&fixture.other_token, &fixture.owner_token] {
        for private in [fixture.memory, fixture.later] {
            let response = fixture
                .client
                .delete_auth(&format!("/api/knowledge/{private}"), token)
                .await;
            assert_eq!(
                response.status,
                StatusCode::NOT_FOUND,
                "memory is nobody's business through this route: {}",
                response.text()
            );
            assert!(
                stored(fixture.pool(), private).await.is_active,
                "the refusal has to leave the row alone"
            );
        }
    }

    let deletable = fixture
        .client
        .delete_auth(
            &format!("/api/knowledge/{}", fixture.document),
            &fixture.owner_token,
        )
        .await;
    deletable.assert_status(StatusCode::NO_CONTENT);

    fixture.clean().await;
}

#[tokio::test]
async fn the_knowledge_route_refuses_every_memory_category_by_name() {
    let fixture = Fixture::new().await;

    let later = format!("{MEMORY_CATEGORY_PREFIX}something-later");
    for category in [
        PROFILE_CATEGORY,
        PREFERENCE_CATEGORY,
        FACT_CATEGORY,
        later.as_str(),
    ] {
        let response = fixture
            .client
            .post_json_auth(
                "/api/knowledge",
                &json!({
                    "workspace_id": fixture.workspace,
                    "title": "Posted profile",
                    "content": "Answers to whoever asks.",
                    "category": category
                }),
                &fixture.owner_token,
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "this route is the workspace's, and the read paths exclude the whole prefix rather \
             than the three names, so a category posted under it would be a row nothing can \
             list, read, edit or delete: {}",
            response.text()
        );
        assert_eq!(
            response.json_value()["error"].as_str(),
            Some(
                format!(
                    "The category {category} is written elsewhere and cannot be set through this route"
                )
                .as_str()
            ),
            "the refusal names the category and neither mechanism that owns one"
        );
    }

    fixture.clean().await;
}

#[tokio::test]
async fn a_recovery_pass_embeds_an_orphaned_entry_and_leaves_memory_unembedded() {
    let fixture = Fixture::embedding().await;
    sort_first(fixture.pool(), &[fixture.memory, fixture.document]).await;

    knowledge_refresh::recover_unindexed(
        fixture.client.state(),
        &knowledge_refresh::permits(),
        &knowledge_refresh::recovery(),
    )
    .await
    .expect("a recovery pass");

    assert_eq!(
        stored(fixture.pool(), fixture.document).await.vectors,
        1,
        "the pass exists to bring an entry with no vector into the index"
    );
    assert_eq!(
        stored(fixture.pool(), fixture.memory).await.vectors,
        0,
        "the pass selects exactly the rows memory consists of, and embedding one would \
         undo the decision that memory is never embedded"
    );

    fixture.clean().await;
}

#[tokio::test]
async fn the_predicate_and_the_private_categories_agree_on_the_prefix() {
    assert!(
        NOT_MEMORY.contains(MEMORY_CATEGORY_PREFIX),
        "the predicate matches on the prefix, not on the list: {NOT_MEMORY}"
    );
    for category in PRIVATE_CATEGORIES {
        assert!(
            category.starts_with(MEMORY_CATEGORY_PREFIX),
            "{category} would be invisible to NOT_MEMORY"
        );
    }
    assert_eq!(
        PRIVATE_CATEGORIES.len(),
        3,
        "a fourth private category has to be minted inside the prefix, and said so here"
    );
}

/// A memory row must not flip a plain chat's context note for everyone in the
/// workspace. The note is what the preview says about retrieval it could not
/// count yet, and it is read by every member of the workspace, not by the
/// person the row belongs to.
#[tokio::test]
async fn a_workspace_holding_only_memory_reports_no_knowledge_in_a_plain_chat_preview() {
    let harness = Harness::new(Some(32_768), false, vec![answer("Understood")]).await;
    let owner = sole_member(&harness.pool, harness.workspace).await;
    let memory = seed_memory(
        &harness.pool,
        harness.workspace,
        owner,
        PROFILE_CATEGORY,
        PROFILE_TITLE,
        MEMORY_CONTENT,
    )
    .await;

    let response = harness.preview("An ordinary question", None).await;
    response.assert_status(StatusCode::OK);
    let quiet = usage(&response.json_value()["context"]);
    assert!(
        !quiet.incomplete && quiet.reason.is_none(),
        "a workspace whose only entry is one person's memory has no workspace retrieval to \
         wait for: {:?}",
        quiet.reason
    );

    knowledge::create_document(
        &harness.pool,
        harness.workspace,
        owner,
        DOCUMENT_TITLE,
        DOCUMENT_CONTENT,
    )
    .await
    .expect("creating an ordinary document")
    .expect("the owner may write in its own workspace");

    let response = harness.preview("An ordinary question", None).await;
    response.assert_status(StatusCode::OK);
    let noted = usage(&response.json_value()["context"]);
    assert!(
        noted.incomplete
            && noted
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("Workspace retrieval results")),
        "an ordinary entry in the same workspace still has to raise the note, or the quiet \
         preview above proves nothing: {noted:?}"
    );
    assert_eq!(
        stored(&harness.pool, memory).await.vectors,
        0,
        "nothing in a preview embeds memory"
    );

    common::discard(&harness.pool, harness.workspace, &[owner]).await;
}

/// The conversation fixture builds its own workspace around its own person, and
/// both the memory row's owner and the control document need that person: a
/// stranger's `create_document` writes nothing.
async fn sole_member(pool: &PgPool, workspace: Uuid) -> Uuid {
    zone_server::db::workspace_members::list_members(pool, workspace)
        .await
        .expect("listing workspace members")
        .first()
        .expect("the fixture's workspace has its creator in it")
        .user_id
}
