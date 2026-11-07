//! `search_knowledge` takes a fixed number of nearest candidates before ranking,
//! so entries that can never be returned must not take up those slots.

use uuid::Uuid;
use zone_server::db::knowledge;

use crate::common::{create_test_pool, discard, setup_test_data};

const DIMENSION: usize = 1024;
const LIMIT: i64 = 1;
const CANDIDATES: usize = 32;
const THRESHOLD: f32 = 0.7;
const MODEL: &str = "test-embedding";

fn query() -> Vec<f32> {
    vec![1.0; DIMENSION]
}

fn near_query(flipped: usize) -> Vec<f32> {
    let mut vector = query();
    vector[..flipped].fill(-1.0);
    vector
}

#[tokio::test]
async fn an_active_entry_behind_a_full_candidate_window_of_inactive_ones_is_found() {
    let pool = create_test_pool().await;
    let (_, workspace, user) = setup_test_data(&pool).await;

    for index in 0..CANDIDATES {
        let inactive = knowledge::create_knowledge(
            &pool,
            workspace,
            &format!("Retired {index}"),
            "An entry that was deleted after it was embedded.",
            None,
            &[],
            8,
            user,
        )
        .await
        .expect("creating an entry to retire");
        knowledge::store_knowledge_embedding(&pool, inactive, workspace, &query(), MODEL)
            .await
            .expect("embedding the entry to retire");
        assert!(
            knowledge::delete_knowledge(&pool, inactive)
                .await
                .expect("retiring the entry"),
            "the entry was active, so retiring it changes a row"
        );
    }

    let active: Uuid = knowledge::create_knowledge(
        &pool,
        workspace,
        "Current",
        "The entry the search should return.",
        None,
        &[],
        8,
        user,
    )
    .await
    .expect("creating the active entry");
    knowledge::store_knowledge_embedding(&pool, active, workspace, &near_query(16), MODEL)
        .await
        .expect("embedding the active entry");

    let hits =
        knowledge::search_knowledge_entries(&pool, &query(), workspace, LIMIT, THRESHOLD).await;
    discard(&pool, workspace, &[user]).await;
    let hits = hits.expect("searching knowledge by vector");

    assert_eq!(
        hits.iter().map(|hit| hit.entry_id).collect::<Vec<_>>(),
        vec![active],
        "every candidate nearer than the active entry is inactive, so the active entry is the \
         only match and must not be crowded out of the candidate window"
    );
}
