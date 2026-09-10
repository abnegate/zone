//! Run against an explicitly selected disposable database, never the application's DB.

mod common;

use chrono::Utc;
use uuid::Uuid;
use zone_server::agent::identifier::Kind;
use zone_server::agent::{CitationKind, citations};
use zone_server::db::{chat_sources, chats};

const PASSAGE_TITLE: &str = "Onboarding handbook";
const PASSAGE_URI: &str = "https://handbook.test/onboarding#week-one";

async fn chat() -> (sqlx::PgPool, Uuid) {
    let pool = common::create_test_pool().await;
    let row = chats::create_chat(&pool, None, "Citation registry", "model", false, true)
        .await
        .expect("the test database accepts a chat");
    (pool, row.id)
}

/// A knowledge passage is named by its entry and addressed by a URL, and the
/// registry has to keep both: it hashes the key so the identifier survives the
/// next turn, and stores the address so a citation built from the registry
/// lands on the same URL the retrieval envelope already cited. Storing the key
/// as the address made the two disagree, and the deduplication that keys on
/// address kept both, so one passage arrived as two citations.
#[tokio::test]
async fn a_passage_cited_from_the_registry_and_from_the_envelope_is_one_citation() {
    let (pool, chat_id) = chat().await;
    let key = format!("knowledge:{}", Uuid::new_v4());

    let source = chat_sources::observe(&pool, chat_id, Kind::Kb, &key, PASSAGE_URI, PASSAGE_TITLE)
        .await
        .expect("the registry accepts a knowledge passage");

    assert_eq!(
        source.uri, PASSAGE_URI,
        "the registry must store the address a reader opens, not the key it hashes"
    );
    assert_eq!(source.key, key, "the registry must keep the key it hashed");

    let observed_at = Utc::now().to_rfc3339();
    let mut citations = vec![citations::from_retrieved(
        PASSAGE_TITLE,
        PASSAGE_URI,
        true,
        &observed_at,
    )];

    let resolved = chat_sources::resolve(&pool, chat_id, std::slice::from_ref(&source.identifier))
        .await
        .expect("the registry resolves an identifier it just minted");
    let row = resolved
        .first()
        .expect("the passage the registry just wrote resolves by its identifier");

    citations::merge(
        &mut citations,
        [citations::from_source(
            CitationKind::WorkspaceDocument,
            &row.identifier,
            &row.title,
            &row.uri,
            row.first_observed_at,
        )],
    );

    assert_eq!(
        citations.len(),
        1,
        "one passage must yield one citation, but the registry and the envelope \
         disagreed about its address: {:?}",
        citations.iter().map(|one| &one.url).collect::<Vec<_>>()
    );
}

/// The identifier follows the key, so a passage whose title or address moves
/// between turns keeps the name an earlier reply already cited.
#[tokio::test]
async fn a_passage_observed_again_keeps_the_identifier_its_key_minted() {
    let (pool, chat_id) = chat().await;
    let key = format!("knowledge:{}", Uuid::new_v4());

    let first = chat_sources::observe(&pool, chat_id, Kind::Kb, &key, PASSAGE_URI, PASSAGE_TITLE)
        .await
        .expect("the registry accepts a knowledge passage");
    let again = chat_sources::observe(
        &pool,
        chat_id,
        Kind::Kb,
        &key,
        PASSAGE_URI,
        "Onboarding handbook, revised",
    )
    .await
    .expect("the registry accepts the same passage twice");

    assert_eq!(
        first.identifier, again.identifier,
        "the identifier is derived from the key, so re-observing must not move it"
    );
}
