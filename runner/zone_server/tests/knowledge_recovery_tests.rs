//! An entry whose embedding failed has to be able to get into the index later.
//!
//! Creating a knowledge entry stores it whether or not its embedding succeeds,
//! which is right -- failing the write would lose the text the user had just
//! given us. What was wrong is that nothing came back for it: the refresh
//! worker only revisits entries with a `source_url`, the manual `/refresh`
//! route refuses an entry without one, and there is no update route. A single
//! transient embedding outage took an entry out of semantic search for good,
//! and said nothing about it, because keyword search kept finding it.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::http::StatusCode;
use common::{TestClient, test_email, test_password};
use serde_json::json;
use uuid::Uuid;
use zone_context::embeddings::EmbeddingService;
use zone_context::error::{ContextError, Result as ContextResult};
use zone_server::db::knowledge;
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
        &knowledge_refresh::backoff(),
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
    let backoff = knowledge_refresh::backoff();

    knowledge_refresh::recover_unindexed(client.state(), &permits, &backoff)
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

    knowledge_refresh::recover_unindexed(client.state(), &permits, &backoff)
        .await
        .expect("the second pass");

    assert_eq!(
        embedding.calls(),
        after_first,
        "the first pass embedded nothing, so the next one has to sit out rather than ask a \
         service that is down another {after_first} times five minutes later"
    );
}
