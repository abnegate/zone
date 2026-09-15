//! A schedule's firing, run end to end by the worker that owns it.
//!
//! Everything either side of the generation is covered by unit tests: that a
//! prompt leaves a turn written down and no message, that a claimed turn is not
//! offered twice inside its lease, that one whose worker died is offered again.
//! What none of them can show is that the turn a firing opens actually reaches
//! the chat, because that needs a model to answer it. Here one does — a scripted
//! one, through the same provider mock every other acceptance test uses — and
//! nothing about the path is stubbed: the worker sweeps on its own tick, claims
//! its own row, and runs the turn through the handler a typed message runs
//! through.
mod common;

use common::context::{Harness, answer};
use serde_json::Value;
use sqlx::Connection;
use std::time::Duration;
use uuid::Uuid;

/// The key `db::actions`'s dispatching tests take, taken here for the same
/// reason: the claim is global, so two tests that both have a firing due steal
/// each other's. In the database rather than in the process, because CI runs
/// these under `cargo nextest`, which gives every test a process of its own.
const DISPATCH: i64 = 0x7A6F_6E65_5245_4D44;

/// Held for the length of the test, on a connection of its own so that
/// returning it to a pool cannot return the lock with it.
async fn dispatching() -> sqlx::PgConnection {
    let mut connection = sqlx::PgConnection::connect(&common::context_database_url())
        .await
        .expect("a connection of its own to hold the dispatch lock");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(DISPATCH)
        .execute(&mut connection)
        .await
        .expect("the dispatch lock is takeable");
    connection
}

/// Every message the chat holds, oldest first.
async fn messages(harness: &Harness) -> Vec<(String, String, Value)> {
    sqlx::query_as::<_, (String, String, Value)>(
        "SELECT role, content, COALESCE(metadata, '{}'::jsonb) FROM messages \
         WHERE chat_id = $1 ORDER BY created_at, id",
    )
    .bind(harness.chat)
    .fetch_all(&harness.pool)
    .await
    .expect("the chat's messages are readable")
}

/// How many turns the firing still owes.
async fn owed(harness: &Harness, reminder: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM reminder_turns WHERE reminder_id = $1")
        .bind(reminder)
        .fetch_one(&harness.pool)
        .await
        .expect("the queue is readable")
}

/// Waits for the firing to be *finished* — the answer in the chat and nothing
/// left owed — or gives up saying which half it was still missing.
///
/// Both halves, because they do not land together. `finish_turn` runs after
/// `run_turn` has returned, and `run_turn` stores the answer before it returns,
/// so between the words appearing and the queue clearing there is a window the
/// dispatch is still inside. That window is the whole reason delivery is
/// at-least-once, and it is wide enough to read: this test asserted the queue
/// the moment the words landed, passed on every local run, and failed in CI
/// where the machine was busy enough to hold the two apart.
///
/// Polled rather than signalled: the worker is the production one, and a test
/// hook into it would be a different worker from the one shipping.
async fn until_finished(harness: &Harness, reminder: Uuid) -> Vec<(String, String, Value)> {
    for _ in 0..300 {
        let stored = messages(harness).await;
        if stored.iter().any(|(role, _, _)| role == "assistant")
            && owed(harness, reminder).await == 0
        {
            return stored;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!(
        "the firing never finished: {} still owed, and the chat holds {:?}",
        owed(harness, reminder).await,
        messages(harness).await
    );
}

/// Waits for the chat to hold `count` messages with nothing left owed.
///
/// `until_finished` cannot serve a second firing: its first condition — any
/// assistant message at all — is already true the moment the first one lands.
async fn until_stored(
    harness: &Harness,
    reminder: Uuid,
    count: usize,
) -> Vec<(String, String, Value)> {
    for _ in 0..300 {
        let stored = messages(harness).await;
        if stored.len() >= count && owed(harness, reminder).await == 0 {
            return stored;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let stored = messages(harness).await;
    panic!(
        "the chat never reached {count} messages: {} still owed, and it holds {stored:?}",
        owed(harness, reminder).await,
    );
}

/// Due now rather than when it was asked for. The create refuses a past
/// `due_at`, which is the right refusal for a person asking for one and the
/// wrong thing for a test that needs the sweep to find something.
async fn make_due(harness: &Harness, reminder: Uuid) {
    sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(reminder)
        .execute(&harness.pool)
        .await
        .unwrap();
}

/// What a watch keeps to compare the next firing against.
async fn baseline(harness: &Harness, reminder: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT last_observation FROM reminders WHERE id = $1")
        .bind(reminder)
        .fetch_one(&harness.pool)
        .await
        .expect("the watch is readable")
}

/// A watch, over two firings: the first establishes a reading and the second is
/// handed it.
///
/// This is the test the design argument rests on. The comparison could have
/// been left to the chat — both firings land in it, so the previous answer is
/// already in the history the model is given — and that shortcut fails silently
/// once the history is compacted, which an hourly watch outlives within a day.
/// So the reading is passed in explicitly, and what proves it is passed in is
/// that the second firing's *user message* contains the first firing's answer.
#[tokio::test]
async fn a_watch_hands_its_next_firing_the_reading_the_last_one_took() {
    let _dispatch = dispatching().await;
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![
            answer("Three jobs, all green."),
            answer("The lint job is red; it was green last time."),
        ],
    )
    .await;

    let user: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' \
         AND is_active LIMIT 1",
    )
    .bind(harness.workspace)
    .fetch_one(&harness.pool)
    .await
    .expect("the harness workspace has an owner");

    let created = zone_server::db::reminders::create(
        &harness.pool,
        harness.workspace,
        user,
        harness.chat,
        zone_server::db::reminders::Reminder {
            content: "Release branch".into(),
            due_at: chrono::Utc::now() + chrono::Duration::hours(1),
            rrule: Some("FREQ=DAILY".into()),
            prompt: Some("Check whether the release branch is green.".into()),
            timing_mode: Some("condition_watch".into()),
        },
    )
    .await
    .expect("a watch carrying both halves of its comparison is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert_eq!(
        baseline(&harness, reminder).await,
        None,
        "a watch starts with nothing to compare against"
    );

    make_due(&harness, reminder).await;
    let worker = zone_server::workers::reminders::spawn(common::create_test_state(
        harness.config.clone(),
        harness.pool.clone(),
    ));

    let stored = until_stored(&harness, reminder, 2).await;
    let (role, first_turn, _) = &stored[0];
    assert_eq!(role, "user");
    assert!(
        first_turn.starts_with("Check whether the release branch is green."),
        "the stored prompt still leads the firing: {first_turn}"
    );
    assert!(
        first_turn.contains("first firing"),
        "the first firing is told it has nothing to compare against: {first_turn}"
    );
    assert_eq!(stored[1].0, "assistant");
    assert_eq!(stored[1].1, "Three jobs, all green.");
    assert_eq!(
        baseline(&harness, reminder).await.as_deref(),
        Some("Three jobs, all green."),
        "what the firing answered is what the next one is compared against"
    );

    make_due(&harness, reminder).await;
    let stored = until_stored(&harness, reminder, 4).await;
    worker.abort();

    let (role, second_turn, metadata) = &stored[2];
    assert_eq!(role, "user");
    assert!(
        second_turn.contains("Three jobs, all green."),
        "the reading is handed to the firing rather than left in a history that \
         compaction will take: {second_turn}"
    );
    assert!(
        !second_turn.contains("first firing"),
        "a firing with a reading behind it is not told it is the first: {second_turn}"
    );
    assert_eq!(
        metadata["reminder_id"],
        Value::from(reminder.to_string()),
        "a watch's turn is marked like any other firing's: {metadata}"
    );
    assert_eq!(stored[3].0, "assistant");
    assert_eq!(
        baseline(&harness, reminder).await.as_deref(),
        Some("The lint job is red; it was green last time."),
        "each firing's answer replaces the reading, so the next compares against \
         the most recent one rather than against the first"
    );
}

/// The whole path, from a row that has come due to words in the chat: the
/// worker's own sweep claims it, writes the turn down, runs it through
/// `ws::chat::run_turn`, and marks it done — and what the model says is the
/// delivery, with the fixed content nowhere in the chat.
#[tokio::test]
async fn a_due_prompt_is_answered_in_its_chat_by_the_worker_that_claimed_it() {
    let _dispatch = dispatching().await;
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![answer("Two dependency PRs have been red since Tuesday.")],
    )
    .await;

    // A chat belongs to a workspace rather than to a person, so the actor a
    // firing runs as is the member who asked for it — here the one the harness
    // registered and made owner.
    let user: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' \
         AND is_active LIMIT 1",
    )
    .bind(harness.workspace)
    .fetch_one(&harness.pool)
    .await
    .expect("the harness workspace has an owner");

    let created = zone_server::db::reminders::create(
        &harness.pool,
        harness.workspace,
        user,
        harness.chat,
        zone_server::db::reminders::Reminder {
            content: "Dependency check".into(),
            due_at: chrono::Utc::now() + chrono::Duration::hours(1),
            rrule: None,
            prompt: Some("Report any dependency PR that has been red for a week.".into()),
            timing_mode: None,
        },
    )
    .await
    .expect("a prompt-bearing reminder is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

    make_due(&harness, reminder).await;

    // A second instance of the server, which is what a worker is: it shares the
    // database and the provider and knows nothing about the socket the harness
    // would use.
    let worker = zone_server::workers::reminders::spawn(common::create_test_state(
        harness.config.clone(),
        harness.pool.clone(),
    ));

    let stored = until_finished(&harness, reminder).await;
    worker.abort();

    assert_eq!(
        stored.len(),
        2,
        "a firing that runs a turn stores the prompt and the answer, and nothing else: {stored:?}"
    );
    let (role, content, metadata) = &stored[0];
    assert_eq!(role, "user", "the prompt is the turn's own user message");
    assert_eq!(
        content,
        "Report any dependency PR that has been red for a week."
    );
    assert_eq!(
        metadata["source"], "reminder",
        "a reader has to be able to tell this from something the person typed: {metadata}"
    );
    assert_eq!(metadata["reminder_id"], Value::from(reminder.to_string()));
    assert_eq!(metadata["actor_id"], Value::from(user.to_string()));

    let (role, content, _) = &stored[1];
    assert_eq!(role, "assistant");
    assert_eq!(
        content, "Two dependency PRs have been red since Tuesday.",
        "what the model said is the delivery"
    );
    assert!(
        !stored
            .iter()
            .any(|(_, content, _)| content == "Dependency check"),
        "the content is the schedule's name beside a prompt, not a notice posted with it"
    );

    assert_eq!(
        owed(&harness, reminder).await,
        0,
        "a turn that has run is not owed again"
    );

    let (status, fired): (String, i32) =
        sqlx::query_as("SELECT status, fired_count FROM reminders WHERE id = $1")
            .bind(reminder)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    assert_eq!(status, "delivered", "a one-shot is finished when it fires");
    assert_eq!(fired, 1);
}
