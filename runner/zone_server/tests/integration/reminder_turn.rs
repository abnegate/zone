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
use crate::common;

use crate::common::context::{Harness, answer, calls, successful, tool};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;
use zone_server::db::reminders::Sweep;
use zone_server::workers::reminders::{Lease, run};

/// The production worker, sweeping only the harness's own workspace.
pub(crate) fn dispatcher(harness: &Harness) -> tokio::task::JoinHandle<()> {
    zone_server::workers::reminders::spawn(
        common::create_test_state(harness.config.clone(), harness.pool.clone()),
        Sweep::Workspace(harness.workspace),
    )
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
pub(crate) async fn until_stored(
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
pub(crate) async fn make_due(harness: &Harness, reminder: Uuid) {
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
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: Some("FREQ=DAILY".into()),
            prompt: Some("Check whether the release branch is green.".into()),
            timing_mode: Some("condition_watch".into()),
            time_zone: None,
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
    let worker = dispatcher(&harness);

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
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: Some("Report any dependency PR that has been red for a week.".into()),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .expect("a prompt-bearing reminder is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

    make_due(&harness, reminder).await;

    // A second instance of the server, which is what a worker is: it shares the
    // database and the provider and knows nothing about the socket the harness
    // would use.
    let worker = dispatcher(&harness);

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

async fn origins(harness: &Harness) -> Vec<String> {
    sqlx::query_scalar("SELECT origin FROM tasks WHERE chat_id = $1 ORDER BY created_at")
        .bind(harness.chat)
        .fetch_all(&harness.pool)
        .await
        .expect("the chat's tasks")
}

fn filing(id: &str) -> wiremock::ResponseTemplate {
    calls(
        "",
        vec![tool(
            id,
            "create_task",
            json!({"title": "Dependency sweep", "description": "Bump what is red"}),
        )],
    )
}

/// A task the agent files during a firing is the reminder's doing, so the
/// project it lands in is not one a member used; one filed during a typed
/// turn is the member's.
#[tokio::test]
async fn a_task_a_firing_files_is_recorded_as_the_reminders() {
    let harness = Harness::new(
        Some(200_000),
        true,
        vec![filing("typed"), answer("Filed it.")],
    )
    .await;
    successful(&harness.turn("File a dependency sweep").await);
    assert_eq!(origins(&harness).await, ["member"]);

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
            content: "Dependency sweep".into(),
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: Some("File a dependency sweep.".into()),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .expect("a prompt-bearing reminder is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    harness.script.push(filing("fired"));
    harness.script.push(answer("Filed another."));
    make_due(&harness, reminder).await;

    let worker = dispatcher(&harness);
    until_stored(&harness, reminder, 4).await;
    worker.abort();

    assert_eq!(
        origins(&harness).await,
        ["member", "reminder"],
        "the firing's task carries the reminder as its origin"
    );
}

/// A watch's turn on its last attempt, still generating when its claim lapsed
/// and the schedule's next firing took over. It is stopped through the chat as
/// a cancelled turn, so the chat's queue is free for the firing that took
/// over, and nothing it might have said becomes the baseline: the firing that
/// took over is compared with the reading recorded before either of them ran.
#[tokio::test]
async fn a_watch_turn_that_lost_its_claim_is_stopped_and_its_successor_keeps_the_baseline() {
    const PRECEDING: &str = "Three jobs, all green.";
    const SUCCESSOR: &str = "The lint job is red; it was green last time.";
    const HANG: Duration = Duration::from_secs(120);
    const STOPPED_WITHIN: Duration = Duration::from_secs(20);
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![
            answer("Never delivered.").set_delay(HANG),
            answer(SUCCESSOR),
        ],
    )
    .await;
    let sweep = Sweep::Workspace(harness.workspace);
    let lease = Lease {
        duration: Duration::from_secs(60),
        renewal: Duration::from_secs(1),
    };

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
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: Some("FREQ=DAILY".into()),
            prompt: Some("Check whether the release branch is green.".into()),
            timing_mode: Some("condition_watch".into()),
            time_zone: None,
        },
    )
    .await
    .expect("a watch is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    sqlx::query("UPDATE reminders SET last_observation = $2 WHERE id = $1")
        .bind(reminder)
        .bind(PRECEDING)
        .execute(&harness.pool)
        .await
        .unwrap();
    for _ in 0..2 {
        make_due(&harness, reminder).await;
        zone_server::db::reminders::deliver_next(&harness.pool, sweep)
            .await
            .expect("the due firing is written down as an owed turn");
    }
    assert_eq!(owed(&harness, reminder).await, 2, "two firings are owed");

    let claim = || async {
        zone_server::db::reminders::claim_turn(&harness.pool, sweep, lease.duration)
            .await
            .unwrap()
            .expect("an owed firing is offered")
    };
    let mut first = claim().await;
    for _ in 1..3 {
        lapse(&harness, first.id, lease).await;
        let retaken = claim().await;
        assert_eq!(
            retaken.id, first.id,
            "a lapsed firing is offered again first"
        );
        first = retaken;
    }
    assert_eq!(
        first.attempts, 3,
        "the first firing is out for its last attempt"
    );

    let state = common::create_test_state(harness.config.clone(), harness.pool.clone());
    let running = tokio::spawn({
        let state = state.clone();
        let first = first.clone();
        async move { run(&state, first, lease).await }
    });
    until_requests(&harness, 1).await;
    lapse(&harness, first.id, lease).await;
    let successor = claim().await;
    assert_ne!(successor.id, first.id, "the next firing is the one taken");

    tokio::time::timeout(STOPPED_WITHIN, running)
        .await
        .expect("a turn whose claim was taken over is stopped rather than left generating")
        .expect("the stopped turn ends without panicking");
    let after_stop = messages(&harness).await;
    let baseline_after_stop = baseline(&harness, reminder).await;

    tokio::time::timeout(STOPPED_WITHIN, run(&state, successor, lease))
        .await
        .expect("the stopped turn released the chat's queue for the firing that took over");
    let stored = messages(&harness).await;
    let baseline_after_successor = baseline(&harness, reminder).await;

    assert!(
        after_stop
            .iter()
            .all(|(role, content, _)| role != "assistant" || content.is_empty()),
        "the stopped turn ended as cancelled, with no answer: {after_stop:?}"
    );
    assert_eq!(
        baseline_after_stop.as_deref(),
        Some(PRECEDING),
        "a stopped turn records no reading"
    );
    let (role, successor_prompt, _) = stored
        .iter()
        .filter(|(role, _, _)| role == "user")
        .nth(1)
        .expect("the firing that took over asked its question");
    assert_eq!(role, "user");
    assert!(
        successor_prompt.contains(PRECEDING),
        "the firing that took over is compared with the reading before it: {successor_prompt}"
    );
    assert!(
        stored
            .iter()
            .any(|(role, content, _)| role == "assistant" && content == SUCCESSOR),
        "the firing that took over answered in the chat: {stored:?}"
    );
    assert_eq!(
        baseline_after_successor.as_deref(),
        Some(SUCCESSOR),
        "the firing that took over records its own reading"
    );
    assert_eq!(
        owed(&harness, reminder).await,
        0,
        "both firings are settled"
    );
}

/// Leaves a turn's claim unrenewed for longer than `lease`.
async fn lapse(harness: &Harness, turn: Uuid, lease: Lease) {
    sqlx::query(
        "UPDATE reminder_turns SET claimed_at = NOW() - make_interval(secs => $2) WHERE id = $1",
    )
    .bind(turn)
    .bind((lease.duration * 2).as_secs_f64())
    .execute(&harness.pool)
    .await
    .unwrap();
}

/// Waits for the provider to have been asked for `count` streamed answers, so
/// every scripted reply up to then has gone to the turn it was meant for.
async fn until_requests(harness: &Harness, count: usize) {
    for _ in 0..300 {
        if streamed_requests(harness).await >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "the provider was asked for {} streamed answers, not {count}",
        streamed_requests(harness).await
    );
}

async fn streamed_requests(harness: &Harness) -> usize {
    harness
        .provider
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| {
            request.url.path() == "/chat/completions"
                && serde_json::from_slice::<Value>(&request.body)
                    .is_ok_and(|body| body["stream"] == true)
        })
        .count()
}

/// A turn whose claim lapsed with nobody yet taking the firing over is stopped
/// all the same, and the firing it never delivered stays owed: the next claim
/// is offered it again as its next attempt, and that one runs it through.
#[tokio::test]
async fn a_turn_stopped_for_a_lapsed_claim_leaves_its_firing_owed_for_the_next_attempt() {
    const DELIVERED: &str = "Two dependency PRs have been red since Tuesday.";
    const HANG: Duration = Duration::from_secs(120);
    const SETTLED_WITHIN: Duration = Duration::from_secs(20);
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![
            answer("Never delivered.").set_delay(HANG),
            answer(DELIVERED),
        ],
    )
    .await;
    let sweep = Sweep::Workspace(harness.workspace);
    let lease = Lease {
        duration: Duration::from_secs(60),
        renewal: Duration::from_secs(1),
    };

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
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: Some("Report any dependency PR that has been red for a week.".into()),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .expect("a prompt-bearing reminder is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    make_due(&harness, reminder).await;
    zone_server::db::reminders::deliver_next(&harness.pool, sweep)
        .await
        .expect("the due firing is written down as an owed turn");
    let claim = || async {
        zone_server::db::reminders::claim_turn(&harness.pool, sweep, lease.duration)
            .await
            .unwrap()
            .expect("the owed firing is offered")
    };
    let first = claim().await;

    let state = common::create_test_state(harness.config.clone(), harness.pool.clone());
    let running = tokio::spawn({
        let state = state.clone();
        let first = first.clone();
        async move { run(&state, first, lease).await }
    });
    until_requests(&harness, 1).await;
    lapse(&harness, first.id, lease).await;
    tokio::time::timeout(SETTLED_WITHIN, running)
        .await
        .expect("a turn whose claim lapsed is stopped rather than left generating")
        .expect("the stopped turn ends without panicking");
    let still_owed = owed(&harness, reminder).await;
    let after_stop = messages(&harness).await;

    let retaken = claim().await;
    tokio::time::timeout(SETTLED_WITHIN, run(&state, retaken.clone(), lease))
        .await
        .expect("the stopped turn released the chat's queue for the next attempt");
    let stored = messages(&harness).await;

    assert_eq!(
        still_owed, 1,
        "a turn stopped for a lapsed claim leaves its firing owed"
    );
    assert!(
        after_stop
            .iter()
            .all(|(role, content, _)| role != "assistant" || content.is_empty()),
        "the stopped turn ended as cancelled, with no answer: {after_stop:?}"
    );
    assert_eq!(retaken.id, first.id, "the same firing is offered again");
    assert_eq!(
        retaken.attempts,
        first.attempts + 1,
        "the firing is offered again as its next attempt"
    );
    assert!(
        stored
            .iter()
            .any(|(role, content, _)| role == "assistant" && content == DELIVERED),
        "the next attempt delivered the firing: {stored:?}"
    );
    assert_eq!(
        owed(&harness, reminder).await,
        0,
        "the attempt that ran the firing through finished it"
    );
}

/// A watch's turn that outlived its lease before any renewal could notice:
/// its answer reached the chat, but the claim it would be kept under had
/// already lapsed, so the reading is refused. The firing is left owed rather
/// than finished, and the next attempt takes the reading the baseline keeps.
#[tokio::test]
async fn a_watch_turn_that_outlived_its_lease_leaves_its_firing_owed() {
    const PRECEDING: &str = "Three jobs, all green.";
    const LATE: &str = "The lint job is red.";
    const RETRIED: &str = "The lint job is red; it was green last time.";
    const SETTLED_WITHIN: Duration = Duration::from_secs(20);
    let short = Lease {
        duration: Duration::from_secs(1),
        renewal: Duration::from_secs(600),
    };
    let long = Lease {
        duration: Duration::from_secs(600),
        renewal: Duration::from_secs(600),
    };
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![answer(LATE).set_delay(short.duration * 3), answer(RETRIED)],
    )
    .await;
    let sweep = Sweep::Workspace(harness.workspace);

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
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: Some("FREQ=DAILY".into()),
            prompt: Some("Check whether the release branch is green.".into()),
            timing_mode: Some("condition_watch".into()),
            time_zone: None,
        },
    )
    .await
    .expect("a watch is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    sqlx::query("UPDATE reminders SET last_observation = $2 WHERE id = $1")
        .bind(reminder)
        .bind(PRECEDING)
        .execute(&harness.pool)
        .await
        .unwrap();
    make_due(&harness, reminder).await;
    zone_server::db::reminders::deliver_next(&harness.pool, sweep)
        .await
        .expect("the due firing is written down as an owed turn");
    let claim = || async {
        zone_server::db::reminders::claim_turn(&harness.pool, sweep, short.duration)
            .await
            .unwrap()
            .expect("the owed firing is offered")
    };
    let state = common::create_test_state(harness.config.clone(), harness.pool.clone());

    let first = claim().await;
    tokio::time::timeout(SETTLED_WITHIN, run(&state, first.clone(), short))
        .await
        .expect("the turn runs to its own end");
    let still_owed = owed(&harness, reminder).await;
    let baseline_after_late = baseline(&harness, reminder).await;
    let after_late = messages(&harness).await;

    let retaken = claim().await;
    tokio::time::timeout(SETTLED_WITHIN, run(&state, retaken.clone(), long))
        .await
        .expect("the next attempt runs to its own end");
    let stored = messages(&harness).await;

    assert!(
        after_late
            .iter()
            .any(|(role, content, _)| role == "assistant" && content == LATE),
        "the late turn's answer reached the chat: {after_late:?}"
    );
    assert_eq!(
        baseline_after_late.as_deref(),
        Some(PRECEDING),
        "a reading taken under a lapsed claim is refused"
    );
    assert_eq!(
        still_owed, 1,
        "a firing whose reading was refused is left owed"
    );
    assert_eq!(retaken.id, first.id, "the same firing is offered again");
    assert_eq!(
        retaken.attempts,
        first.attempts + 1,
        "the firing is offered again as its next attempt"
    );
    let (_, retried_prompt, _) = stored
        .iter()
        .filter(|(role, _, _)| role == "user")
        .nth(1)
        .expect("the next attempt asked its question");
    assert!(
        retried_prompt.contains(PRECEDING),
        "the next attempt is compared with the reading before it: {retried_prompt}"
    );
    assert_eq!(
        baseline(&harness, reminder).await.as_deref(),
        Some(RETRIED),
        "the next attempt keeps its reading"
    );
    assert_eq!(
        owed(&harness, reminder).await,
        0,
        "the attempt that kept its reading finished the firing"
    );
}
