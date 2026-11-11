//! An entry whose embedding failed has to be able to get into the index later.
//!
//! Creating a knowledge entry stores it whether or not its embedding succeeds,
//! which is right -- failing the write would lose the text the user had just
//! given us. What was wrong is that nothing came back for it: the refresh
//! worker only revisits entries with a `source_url`, the manual `/refresh`
//! route refuses an entry without one, and there is no update route. A single
//! transient embedding outage took an entry out of semantic search for good,
//! and said nothing about it, because keyword search kept finding it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::common::{TestClient, test_email, test_password};
use async_trait::async_trait;
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::Mutex;
use uuid::Uuid;
use zone_context::embeddings::EmbeddingService;
use zone_context::error::{ContextError, Result as ContextResult};
use zone_server::db::knowledge;
use zone_server::db::knowledge::discovery::{self, Discovery, Kind, Provenance, Source};
use zone_server::services;
use zone_server::workers::knowledge_refresh;

/// An embedding service that can be taken away and given back, and that counts
/// what it was asked for.
///
/// The reproduction is an outage, not a missing configuration: the service is
/// there and answering everything else, and the embedding call is what fails.
struct Embedding {
    reachable: AtomicBool,
    calls: AtomicUsize,
}

impl Embedding {
    fn unreachable() -> Arc<Self> {
        Arc::new(Self {
            reachable: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
        })
    }

    fn becomes_reachable(&self) {
        self.reachable.store(true, Ordering::SeqCst);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn vector(&self, text: &str) -> ContextResult<Vec<f32>> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        if !self.reachable.load(Ordering::SeqCst) {
            return Err(ContextError::Embedding(
                "Request failed: error sending request for url (http://127.0.0.1:1/api/embeddings)"
                    .to_string(),
            ));
        }

        let seed = text.len() as f32;
        Ok((0..32).map(|slot| (seed + slot as f32) / 1024.0).collect())
    }
}

#[async_trait]
impl EmbeddingService for Embedding {
    async fn embed(&self, text: &str) -> ContextResult<Vec<f32>> {
        self.vector(text)
    }

    async fn embed_batch(&self, texts: &[&str]) -> ContextResult<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.vector(text)).collect()
    }

    fn dimension(&self) -> usize {
        32
    }

    fn model(&self) -> &str {
        "test-embedder"
    }
}

/// The first entry for `stale` that the service is asked to embed is
/// replaced while the service is still answering, the way an agent recording
/// a revised statement lands while recovery is embedding the old one.
struct Replacing {
    stale: &'static str,
    replacement: Mutex<Option<(PgPool, Discovery)>>,
}

impl Replacing {
    fn of(stale: &'static str) -> Arc<Self> {
        Arc::new(Self {
            stale,
            replacement: Mutex::new(None),
        })
    }

    async fn replaces_with(&self, pool: PgPool, revised: Discovery) {
        *self.replacement.lock().await = Some((pool, revised));
    }

    fn vector(text: &str) -> Vec<f32> {
        let seed = text.len() as f32;
        (0..32).map(|slot| (seed + slot as f32) / 1024.0).collect()
    }
}

#[async_trait]
impl EmbeddingService for Replacing {
    async fn embed(&self, text: &str) -> ContextResult<Vec<f32>> {
        let replacement = self
            .replacement
            .lock()
            .await
            .take_if(|_| text == self.stale);
        if let Some((pool, revised)) = replacement {
            discovery::record(&pool, &revised)
                .await
                .expect("replacing the statement mid-embedding");
        }
        Ok(Self::vector(text))
    }

    async fn embed_batch(&self, texts: &[&str]) -> ContextResult<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|text| Self::vector(text)).collect())
    }

    fn dimension(&self) -> usize {
        32
    }

    fn model(&self) -> &str {
        "test-embedder"
    }
}

async fn setup_user_and_workspace(client: &TestClient) -> (String, Uuid, Uuid) {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
                "display_name": "Recovery Tester"
            }),
        )
        .await;

    let registration = registered.json_value();
    let token = registration["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let user_id = Uuid::parse_str(
        registration["user"]["id"]
            .as_str()
            .expect("registration returns the user"),
    )
    .expect("user id is a uuid");

    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({
                "name": format!("Recovery Org {}", Uuid::new_v4()),
                "slug": format!("recovery-org-{}", Uuid::new_v4())
            }),
            &token,
        )
        .await;
    let organization_id = organization.json_value()["organization"]["id"]
        .as_str()
        .expect("organization id")
        .to_string();

    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization_id}/workspaces"),
            &json!({
                "name": format!("Recovery Workspace {}", Uuid::new_v4()),
                "slug": format!("recovery-ws-{}", Uuid::new_v4())
            }),
            &token,
        )
        .await;
    let workspace_id = Uuid::parse_str(
        workspace.json_value()["workspace"]["id"]
            .as_str()
            .expect("workspace id"),
    )
    .expect("workspace id is a uuid");

    (token, workspace_id, user_id)
}

/// The recovery pass is server-wide and the test database is shared, so pull
/// the entries under test to the front of its `created_at ASC` queue. Without
/// this, what a bounded pass happens to look at depends on what every other
/// test has left behind.
async fn sort_first(client: &TestClient, ids: &[Uuid], epoch: &str) {
    sqlx::query("UPDATE knowledge_entries SET created_at = $2::timestamp WHERE id = ANY($1)")
        .bind(ids)
        .bind(epoch)
        .execute(client.state().db())
        .await
        .expect("backdating an entry under test");
}

async fn stored_vectors(client: &TestClient, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM knowledge_embeddings WHERE knowledge_entry_id = $1")
        .bind(id)
        .fetch_one(client.state().db())
        .await
        .expect("counting stored vectors")
}

async fn entry(client: &TestClient, id: Uuid, token: &str) -> serde_json::Value {
    let response = client
        .get_auth(&format!("/api/knowledge/{id}"), token)
        .await;
    response.assert_status(StatusCode::OK);
    response.json_value()
}

async fn listed(
    client: &TestClient,
    workspace_id: Uuid,
    id: Uuid,
    token: &str,
) -> serde_json::Value {
    let response = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={workspace_id}"),
            token,
        )
        .await;
    response.assert_status(StatusCode::OK);

    response
        .json_value()
        .as_array()
        .expect("the list route answers with an array")
        .iter()
        .find(|row| row["id"].as_str() == Some(&id.to_string()))
        .cloned()
        .expect("the created entry is in its workspace's list")
}

#[tokio::test]
async fn an_entry_stored_while_embedding_is_unreachable_says_so_and_is_recovered_by_a_pass() {
    let embedding = Embedding::unreachable();
    let client =
        TestClient::with_embedding(Arc::clone(&embedding) as Arc<dyn EmbeddingService>).await;
    let (token, workspace_id, _user_id) = setup_user_and_workspace(&client).await;

    let created = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": workspace_id,
                "title": "Outage entry",
                "content": "The Marlowe-9 regulator is torqued to 63 newton-metres.",
                "category": "outage"
            }),
            &token,
        )
        .await;

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "an embedding outage must not lose the text the caller just sent: {}",
        created.text()
    );

    let body = created.json_value();
    let id = Uuid::parse_str(body["id"].as_str().expect("created id")).expect("id is a uuid");

    assert_eq!(
        body["indexed"], false,
        "the entry was stored with no vector, so the response that stored it has to say the \
         entry is not in the semantic index"
    );
    assert_eq!(
        stored_vectors(&client, id).await,
        0,
        "the outage means no row in knowledge_embeddings"
    );
    assert_eq!(
        entry(&client, id, &token).await["indexed"],
        false,
        "reading the entry back has to report the same state"
    );
    assert_eq!(
        listed(&client, workspace_id, id, &token).await["indexed"],
        false,
        "the wiki reads the list, so an unindexed entry has to be distinguishable there"
    );

    embedding.becomes_reachable();
    sort_first(&client, &[id], "1970-01-01 00:00:01").await;

    knowledge_refresh::recover_unindexed(
        client.state(),
        &knowledge_refresh::permits(),
        &knowledge_refresh::recovery(),
    )
    .await
    .expect("a recovery pass");

    assert_eq!(
        stored_vectors(&client, id).await,
        1,
        "the entry an outage left behind has to be embedded once the service answers again"
    );
    assert_eq!(
        entry(&client, id, &token).await["indexed"],
        true,
        "and it has to report itself indexed afterwards"
    );
    assert_eq!(
        listed(&client, workspace_id, id, &token).await["indexed"],
        true
    );
}

#[tokio::test]
async fn a_recovery_pass_is_bounded_and_backs_off_while_embedding_stays_down() {
    let embedding = Embedding::unreachable();
    let client =
        TestClient::with_embedding(Arc::clone(&embedding) as Arc<dyn EmbeddingService>).await;
    let (_token, workspace_id, user_id) = setup_user_and_workspace(&client).await;

    let mut ids = Vec::new();
    for note in 0..40 {
        ids.push(
            knowledge::create_knowledge(
                client.state().db(),
                workspace_id,
                &format!("Backlog note {note}"),
                "Every one of these is waiting for a vector.",
                None,
                &[],
                8,
                user_id,
            )
            .await
            .expect("seeding an unindexed entry"),
        );
    }
    sort_first(&client, &ids, "1970-01-01 00:00:02").await;

    let permits = knowledge_refresh::permits();
    let recovery = knowledge_refresh::recovery();

    knowledge_refresh::recover_unindexed(client.state(), &permits, &recovery)
        .await
        .expect("the first pass");
    let after_first = embedding.calls();

    assert!(
        after_first > 0,
        "a pass with a backlog of {} entries has to try at least one",
        ids.len()
    );
    assert!(
        after_first <= knowledge_refresh::MAX_UNINDEXED_PER_CYCLE as usize,
        "a pass asked the embedding service {after_first} times for a backlog of {}; it has to \
         take a bounded bite instead of the whole backlog",
        ids.len()
    );

    knowledge_refresh::recover_unindexed(client.state(), &permits, &recovery)
        .await
        .expect("the second pass");

    assert_eq!(
        embedding.calls(),
        after_first,
        "the first pass embedded nothing, so the next one has to sit out rather than ask a \
         service that is down another {after_first} times five minutes later"
    );
}

/// The first element of the vector stored for `id`, which is the length of the
/// text it was computed from at every embedder in this file. `None` when the
/// entry has no vector.
async fn computed_from(pool: &PgPool, id: Uuid) -> Option<f32> {
    sqlx::query_scalar(
        "SELECT (vector::real[])[1] FROM knowledge_embeddings WHERE knowledge_entry_id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .expect("reading a stored vector")
}

/// Recovery reads an entry's text, and the entry is replaced while that text
/// is being embedded. The vector that comes back is of what the entry no
/// longer says: stored, it would also mark the entry indexed, and no pass
/// would look at it again.
///
/// Every test's recovery pass shares this database, and a pass reads at most
/// [`knowledge_refresh::MAX_UNINDEXED_PER_CYCLE`] entries from a backlog other
/// tests fill, so both halves run through the indexing a pass does per entry:
/// the race, then the retry a pass makes of an entry that still has no vector,
/// with the text the entry holds when it is read again.
#[tokio::test]
async fn a_vector_of_text_replaced_mid_embedding_is_not_stored_and_the_next_pass_indexes_the_new() {
    const STALE: &str = "Deploys go out from the release branch.";
    const REVISED: &str = "Deploys go out from main, tagged, once the checklist is green.";

    let embedding = Replacing::of(STALE);
    let client =
        TestClient::with_embedding(Arc::clone(&embedding) as Arc<dyn EmbeddingService>).await;
    let (_token, workspace_id, user_id) = setup_user_and_workspace(&client).await;
    let pool = client.state().db().clone();

    let recorded = |statement: &str| Discovery {
        workspace_id,
        subject: "Where deploys come from".to_string(),
        statement: statement.to_string(),
        author: user_id,
        provenance: Provenance {
            kind: Kind::Lesson,
            source: Source::Run(Uuid::new_v4()),
            last_confirmed: chrono::NaiveDate::from_ymd_opt(2026, 11, 11).expect("a real date"),
        },
    };
    let (id, _) = discovery::record(&pool, &recorded(STALE))
        .await
        .expect("recording a discovery");
    embedding
        .replaces_with(pool.clone(), recorded(REVISED))
        .await;

    let indexed = services::knowledge::index(client.state(), id, workspace_id, STALE).await;

    assert!(
        matches!(indexed, Err(services::knowledge::Unindexed::Changed)),
        "{indexed:?}"
    );
    assert_eq!(
        stored_vectors(&client, id).await,
        0,
        "the vector of the replaced statement must not be stored, so the entry is still \
         one a recovery pass looks for"
    );

    let current: String = sqlx::query_scalar("SELECT content FROM knowledge_entries WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("reading the entry as a pass would");
    assert_eq!(current, REVISED);
    services::knowledge::index(client.state(), id, workspace_id, &current)
        .await
        .expect("indexing the text the entry holds now");

    assert_eq!(
        computed_from(&pool, id).await,
        Some(Replacing::vector(REVISED)[0]),
        "the vector stored is of the statement the entry holds now"
    );
}
