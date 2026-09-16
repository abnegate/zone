//! Run against an explicitly selected disposable database, never the application's DB.

mod common;

use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;
use zone_chat::history::{NewEntry, ReplayMessage, Summary, fingerprint};
use zone_core::llm::{FunctionCall, Message, Role, ToolCall};
use zone_server::db::chats;
use zone_server::db::context::{Error, Lease, Store};

const LIFETIME: Duration = Duration::from_secs(30);

async fn fixture() -> (PgPool, Store, Uuid, Uuid) {
    let address = common::context_database_url();
    let pool = PgPool::connect(&address).await.unwrap();
    let organization = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    sqlx::query("INSERT INTO organizations (id,name,slug) VALUES ($1,'Context test',$2)")
        .bind(organization)
        .bind(organization.to_string())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO workspaces (id,organization_id,name,slug) VALUES ($1,$2,'Context test',$3)",
    )
    .bind(workspace)
    .bind(organization)
    .bind(workspace.to_string())
    .execute(&pool)
    .await
    .unwrap();
    let chat = chats::create_chat(&pool, Some(workspace), "Context test", "model", true, true)
        .await
        .unwrap();
    (
        pool.clone(),
        Store::new(pool, chat.id, Some(workspace)),
        chat.id,
        workspace,
    )
}

async fn begin(store: &Store, lease: &Lease) -> (Uuid, Uuid) {
    let turn = Uuid::new_v4();
    let user = Uuid::new_v4();
    store
        .begin(
            lease,
            turn,
            user,
            "Latest request",
            None,
            ReplayMessage::from(&Message::user("Latest request")),
        )
        .await
        .unwrap();
    (turn, user)
}

fn envelope(id: &str, call: &str, mutating: bool) -> NewEntry {
    let mut message = Message::assistant_with_tools(vec![ToolCall {
        id: call.into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: if mutating { "write_file" } else { "read_file" }.into(),
            arguments: r#"{"path":"notes.txt"}"#.into(),
        },
    }]);
    message.content = Some("I will inspect the saved evidence.".into());
    NewEntry {
        id: id.into(),
        message: ReplayMessage::from(&message),
        mutations: if mutating {
            vec![call.into()]
        } else {
            Vec::new()
        },
    }
}

async fn expire(pool: &PgPool, chat: Uuid) {
    sqlx::query(
        "UPDATE chat_leases SET expires_at=clock_timestamp()-interval '1 second' WHERE chat_id=$1",
    )
    .bind(chat)
    .execute(pool)
    .await
    .unwrap();
}

async fn turn_state(pool: &PgPool, chat: Uuid, turn: Uuid) -> (String, Option<String>) {
    sqlx::query_as("SELECT status, completed_at::text FROM chat_turns WHERE chat_id=$1 AND id=$2")
        .bind(chat)
        .bind(turn)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn visible(pool: &PgPool, chat: Uuid, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT content FROM messages WHERE chat_id=$1 AND id=$2")
        .bind(chat)
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

fn result(id: &str, call: &str, content: &str) -> NewEntry {
    NewEntry {
        id: id.into(),
        message: ReplayMessage::from(&Message::tool_result(call, content)),
        mutations: Vec::new(),
    }
}

fn assistant(id: &str, content: &str) -> NewEntry {
    NewEntry {
        id: id.into(),
        message: ReplayMessage::from(&Message::assistant(content)),
        mutations: Vec::new(),
    }
}

async fn summary(store: &Store, ids: &[&str], revision: u64) -> Summary {
    let history = store.load().await.unwrap();
    let entries = ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
    Summary {
        content:
            "Objective: finish the request. Evidence: retained by stable ids. Pending: answer user."
                .into(),
        fingerprint: fingerprint(&history.entries, &entries).unwrap(),
        entries,
        revision,
    }
}

#[tokio::test]
async fn full_envelope_and_multimodal_result_survive_without_visible_assistant_row() {
    let (pool, store, chat, workspace) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    let output = format!("Error: failed\n{}\nCRITICAL SUFFIX", "x".repeat(20_000));
    let mut evidence = result("result", "call", &output);
    evidence
        .message
        .images
        .push("/api/artifacts/immutable.png".into());
    store
        .append(
            &lease,
            turn,
            &[envelope("envelope", "call", false), evidence],
        )
        .await
        .unwrap();
    let restarted = Store::new(pool.clone(), chat, Some(workspace));
    let history = restarted.load().await.unwrap();
    assert_eq!(history.entries.len(), 3);
    assert_eq!(
        history.entries[1].message.content.as_deref(),
        Some("I will inspect the saved evidence.")
    );
    assert_eq!(
        history.entries[2].message.content.as_deref(),
        Some(output.as_str())
    );
    assert_eq!(
        history.entries[2].message.images,
        vec!["/api/artifacts/immutable.png"]
    );
    assert_eq!(chats::list_messages(&pool, chat).await.unwrap().len(), 1);
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn interrupt_marks_tool_evidence_consumed() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .append(
            &lease,
            turn,
            &[
                envelope("envelope", "call", false),
                result("result", "call", "file body"),
            ],
        )
        .await
        .unwrap();
    let before = store.load().await.unwrap();
    assert!(
        before
            .entries
            .iter()
            .any(|entry| !entry.consumed && entry.message.role == Role::Tool)
    );
    store.interrupt(&lease, turn).await.unwrap();
    let after = store.load().await.unwrap();
    assert!(
        after
            .entries
            .iter()
            .filter(|entry| entry.message.role != Role::User)
            .all(|entry| entry.consumed),
        "cancelled tool evidence must be consumed so the next turn can compact"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn publish_makes_streamed_assistant_visible_before_finish() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .publish(&lease, turn, "partial reply", None)
        .await
        .unwrap();
    let listed = chats::list_messages(&pool, chat).await.unwrap();
    let assistant = listed
        .iter()
        .find(|message| message.role == "assistant")
        .expect("live snapshot");
    assert_eq!(assistant.id, turn);
    assert_eq!(assistant.content, "partial reply");
    store
        .publish(&lease, turn, "partial reply continues", None)
        .await
        .unwrap();
    store
        .complete(&lease, turn, "partial reply continues. Done.", None)
        .await
        .unwrap();
    let listed = chats::list_messages(&pool, chat).await.unwrap();
    assert_eq!(
        listed
            .iter()
            .filter(|message| message.role == "assistant")
            .count(),
        1
    );
    assert_eq!(
        listed
            .iter()
            .find(|message| message.role == "assistant")
            .unwrap()
            .content,
        "partial reply continues. Done."
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn independent_app_states_cannot_save_competing_user_turns() {
    let (pool, store, chat, workspace) = fixture().await;
    let first = common::create_test_state(common::test_config(), pool.clone());
    let second = common::create_test_state(common::test_config(), pool.clone());
    let a = Store::new(first.db().clone(), chat, Some(workspace));
    let b = Store::new(second.db().clone(), chat, Some(workspace));
    let (left, right) = tokio::join!(
        a.acquire(Uuid::new_v4(), LIFETIME),
        b.acquire(Uuid::new_v4(), LIFETIME)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let lease = left.or(right).unwrap();
    begin(&store, &lease).await;
    assert_eq!(chats::list_messages(&pool, chat).await.unwrap().len(), 1);
    store.release(&lease).await.unwrap();
    let next = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    assert!(next.fence > lease.fence);
    assert!(!store.release(&lease).await.unwrap());
    assert!(matches!(
        store.renew(&lease, LIFETIME).await,
        Err(Error::LeaseLost)
    ));
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn takeover_fences_old_writes_and_recovers_uncertain_mutation_without_retry() {
    let (pool, store, chat, _) = fixture().await;
    let old = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &old).await;
    store
        .append(&old, turn, &[envelope("mutation", "call", true)])
        .await
        .unwrap();
    expire(&pool, chat).await;
    let current = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    assert!(matches!(
        store
            .append(&old, turn, &[result("stale", "call", "success")])
            .await,
        Err(Error::LeaseLost)
    ));
    assert!(matches!(
        store.complete(&old, turn, "done", None).await,
        Err(Error::LeaseLost)
    ));
    assert_eq!(store.recover(&current).await.unwrap(), 1);
    assert_eq!(store.recover(&current).await.unwrap(), 0);
    let history = store.load().await.unwrap();
    assert_eq!(history.entries.len(), 3);
    assert!(
        history.entries[2]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("Outcome unknown")
    );
    assert_eq!(
        history.entries[2].message.tool_call_id.as_deref(),
        Some("call")
    );
    assert!(!history.entries[2].consumed);
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn stopping_a_turn_leaves_only_the_uncertain_mutation_notice_unconsumed() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .append(&lease, turn, &[envelope("mutation", "call", true)])
        .await
        .unwrap();
    store
        .finish(&lease, turn, "Stopped.", None, true, None)
        .await
        .unwrap();
    let history = store.load().await.unwrap();
    let notice = history
        .entries
        .iter()
        .find(|entry| {
            entry
                .message
                .content
                .as_deref()
                .is_some_and(|content| content.contains("may have changed external state"))
        })
        .expect("stopping a pending mutation must record an uncertain outcome");
    assert_eq!(notice.message.role, Role::Tool);
    assert_eq!(notice.message.tool_call_id.as_deref(), Some("call"));
    let unconsumed: Vec<&str> = history
        .entries
        .iter()
        .filter(|entry| !entry.consumed && entry.message.role != Role::User)
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(
        unconsumed,
        vec![notice.id.as_str()],
        "a stopped turn must fold away everything the model already read and retain exactly the uncertain outcome it has not, so compaction cannot summarize the warning away before the next turn repeats the mutation"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn renewal_runs_independently_and_loss_wakes_cancellation() {
    let (pool, store, chat, _) = fixture().await;
    // Renewal fires every `lifetime / 3`, and each one is a database round
    // trip. At 450ms that was a 150ms budget, and three missed in a row lose
    // the lease -- which is what the whole workspace's test binaries sharing
    // one Postgres does to it, failing here with LeaseLost. The claim under
    // test is only that a sleep longer than one lifetime still holds the
    // lease, which nothing but renewal can achieve, so the ratio is what
    // matters and the absolute numbers should be nowhere near the machine's
    // scheduling noise.
    let lifetime = Duration::from_secs(3);
    let lease = store.acquire(Uuid::new_v4(), lifetime).await.unwrap();
    let mut guard = store.keep_alive(lease.clone(), lifetime).unwrap();
    // Model/socket task may be suspended longer than the initial lease lifetime.
    tokio::time::sleep(lifetime + lifetime / 2).await;
    store.assert_current(&lease).await.unwrap();
    assert!(!guard.is_lost());
    sqlx::query("UPDATE chat_leases SET owner=$2,fence=fence+1 WHERE chat_id=$1")
        .bind(chat)
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), guard.lost())
        .await
        .unwrap();
    assert!(guard.is_lost());
    drop(guard);
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn checkpoints_require_consumed_whole_groups_and_preserve_current_user() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, user) = begin(&store, &lease).await;
    store
        .append(
            &lease,
            turn,
            &[
                envelope("envelope", "call", false),
                result("result", "call", "Error: preserve failure"),
            ],
        )
        .await
        .unwrap();
    let proposed = summary(&store, &["envelope", "result"], 1).await;
    assert!(matches!(
        store.checkpoint(&lease, None, &proposed).await,
        Err(Error::Integrity(_))
    ));
    store
        .consumed(&lease, &["envelope".into(), "result".into()])
        .await
        .unwrap();
    let partial = summary(&store, &["result"], 1).await;
    assert!(matches!(
        store.checkpoint(&lease, None, &partial).await,
        Err(Error::Integrity(_))
    ));
    let latest = summary(&store, &[&user.to_string()], 1).await;
    assert!(matches!(
        store.checkpoint(&lease, None, &latest).await,
        Err(Error::Integrity(_))
    ));
    store.checkpoint(&lease, None, &proposed).await.unwrap();
    assert_eq!(store.load().await.unwrap().summary, Some(proposed.clone()));
    assert!(matches!(
        store.checkpoint(&lease, None, &proposed).await,
        Err(Error::Conflict)
    ));
    store
        .append(&lease, turn, &[assistant("later", "New evidence")])
        .await
        .unwrap();
    let next = summary(&store, &["envelope", "result", "later"], 2).await;
    store
        .checkpoint(&lease, Some(&proposed), &next)
        .await
        .unwrap();
    let mut corrupt = next.clone();
    corrupt.revision = 3;
    corrupt.fingerprint = "wrong".into();
    assert!(matches!(
        store.checkpoint(&lease, Some(&next), &corrupt).await,
        Err(Error::Integrity(_))
    ));
    assert_eq!(store.load().await.unwrap().summary, Some(next));
    assert_eq!(
        store.evidence("result", 0, u64::MAX).await.unwrap().content,
        "Error: preserve failure"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn corrupt_checkpoint_is_reported_and_canonical_history_is_never_discarded() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .append(&lease, turn, &[assistant("past", "Evidence")])
        .await
        .unwrap();
    let checkpoint = summary(&store, &["past"], 1).await;
    store.checkpoint(&lease, None, &checkpoint).await.unwrap();
    sqlx::query("UPDATE chat_checkpoints SET fingerprint='corrupted' WHERE chat_id=$1")
        .bind(chat)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(store.load().await, Err(Error::Integrity(_))));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_entries WHERE chat_id=$1")
        .bind(chat)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn invalid_or_duplicate_results_roll_back_the_entire_append() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    assert!(
        store
            .append(
                &lease,
                turn,
                &[
                    assistant("rolledback", "must rollback"),
                    result("orphan", "missing", "result")
                ]
            )
            .await
            .is_err()
    );
    assert_eq!(store.load().await.unwrap().entries.len(), 1);
    store
        .append(
            &lease,
            turn,
            &[
                envelope("envelope", "call", false),
                result("result", "call", "original"),
            ],
        )
        .await
        .unwrap();
    assert!(
        store
            .append(&lease, turn, &[result("duplicate", "call", "replacement")])
            .await
            .is_err()
    );
    assert_eq!(store.load().await.unwrap().entries.len(), 3);
    assert_eq!(
        store.evidence("result", 0, u64::MAX).await.unwrap().content,
        "original"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn evidence_is_chat_workspace_scoped_and_unicode_paging_is_lossless() {
    let (pool, store, chat, workspace) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .append(
            &lease,
            turn,
            &[
                envelope("envelope", "call", false),
                result("evidence", "call", "α🙂\nfinal"),
            ],
        )
        .await
        .unwrap();
    let first = store.evidence("evidence", 0, 2).await.unwrap();
    assert_eq!(first.content, "α🙂");
    assert_eq!(first.next, Some(2));
    assert_eq!(first.total, 8);
    let last = store.evidence("evidence", 2, u64::MAX).await.unwrap();
    assert_eq!(last.content, "\nfinal");
    assert_eq!(last.next, None);
    assert!(store.evidence("evidence", 9, 1).await.is_err());
    assert!(
        Store::new(pool.clone(), chat, Some(Uuid::new_v4()))
            .evidence("evidence", 0, 100)
            .await
            .is_err()
    );
    assert!(
        Store::new(pool.clone(), chat, None)
            .evidence("evidence", 0, 100)
            .await
            .is_err()
    );
    let other = chats::create_chat(&pool, Some(workspace), "other", "model", false, true)
        .await
        .unwrap();
    assert!(
        Store::new(pool.clone(), other.id, Some(workspace))
            .evidence("evidence", 0, 100)
            .await
            .is_err()
    );
    chats::delete_chat(&pool, chat).await.unwrap();
    chats::delete_chat(&pool, other.id).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM chat_entries WHERE chat_id=$1) + (SELECT count(*) FROM chat_turns WHERE chat_id=$1) + (SELECT count(*) FROM chat_calls WHERE chat_id=$1) + (SELECT count(*) FROM chat_leases WHERE chat_id=$1) + (SELECT count(*) FROM chat_checkpoints WHERE chat_id=$1)").bind(chat).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn all_legacy_messages_and_image_references_keep_stable_ids_after_materialization() {
    let (pool, store, chat, _) = fixture().await;
    for index in 0..55 {
        chats::create_message(&pool, chat, "user", &format!("Message {index}"), None)
            .await
            .unwrap();
    }
    let legacy=chats::create_message(&pool,chat,"assistant","Old answer",Some(json!({"attachments":[{"mime":"image/png","url":"/api/artifacts/old.png"}],"tool_calls":[{"name":"read_file","success":false,"detail":"file missing"}]}))).await.unwrap();
    let before = store.load().await.unwrap();
    assert_eq!(before.entries.len(), 56);
    assert!(before.incomplete);
    let previous = before
        .entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(before.entries.last().unwrap().id, legacy.id.to_string());
    assert_eq!(
        before.entries.last().unwrap().message.images,
        vec!["/api/artifacts/old.png"]
    );
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .append(&lease, turn, &[assistant("canonical-final", "New answer")])
        .await
        .unwrap();
    store
        .complete(&lease, turn, "Intermediate prose plus new answer", None)
        .await
        .unwrap();
    let after = store.load().await.unwrap();
    assert!(after.incomplete);
    assert_eq!(after.entries.len(), 58);
    assert_eq!(
        after.entries[..56]
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        previous
    );
    assert_eq!(
        after.entries.last().unwrap().message.content.as_deref(),
        Some("New answer")
    );
    assert!(!after.entries.iter().any(
        |entry| entry.message.content.as_deref() == Some("Intermediate prose plus new answer")
    ));
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn turn_qualified_provider_call_ids_can_repeat_across_generations() {
    let (pool, store, chat, _) = fixture().await;
    for index in 0..2 {
        let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
        let (turn, _) = begin(&store, &lease).await;
        let call = format!("{turn}:round1:call_0");
        store
            .append(
                &lease,
                turn,
                &[
                    envelope(&format!("envelope-{index}"), &call, false),
                    result(&format!("result-{index}"), &call, "saved"),
                ],
            )
            .await
            .unwrap();
        store.complete(&lease, turn, "done", None).await.unwrap();
        store.release(&lease).await.unwrap();
    }
    assert_eq!(store.load().await.unwrap().entries.len(), 6);
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn later_legacy_public_rows_follow_canonical_history_before_and_after_import() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    store
        .append(&lease, turn, &[assistant("first-answer", "First answer")])
        .await
        .unwrap();
    store
        .complete(&lease, turn, "First answer", None)
        .await
        .unwrap();
    let later = chats::create_message(&pool, chat, "user", "Later media request", None)
        .await
        .unwrap();
    let before = store.load().await.unwrap();
    assert_eq!(before.entries.last().unwrap().id, later.id.to_string());
    begin(&store, &lease).await;
    let after = store.load().await.unwrap();
    assert_eq!(after.entries[2].id, later.id.to_string());
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn filtered_normal_stop_and_interrupted_image_are_canonical() {
    let (pool, store, chat, _) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lease).await;
    let partial = ReplayMessage::from(&Message::assistant("Filtered response"));
    store
        .finish(
            &lease,
            turn,
            "Filtered response",
            None,
            false,
            Some(&partial),
        )
        .await
        .unwrap();
    let history = store.load().await.unwrap();
    assert_eq!(
        history.entries.last().unwrap().message.content.as_deref(),
        Some("Filtered response")
    );
    let (turn, _) = begin(&store, &lease).await;
    let mut image = Message::assistant("");
    image.images = vec!["https://example.com/output.png".into()];
    let partial = ReplayMessage::from(&image);
    store
        .finish(
            &lease,
            turn,
            "",
            Some(json!({"attachments":[{"mime":"image/png","url":image.images[0]}]})),
            true,
            Some(&partial),
        )
        .await
        .unwrap();
    let history = store.load().await.unwrap();
    assert_eq!(history.entries.last().unwrap().message.images, image.images);
    store.release(&lease).await.unwrap();
    sqlx::query("DELETE FROM chats WHERE id=$1")
        .bind(chat)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn preview_does_not_initialize_mcp_in_real_application_state() {
    use zone_server::services::chat::session::{self, Mode};
    use zone_server::state::AppState;
    let (pool, _, chat, _) = fixture().await;
    let mut config = common::test_config();
    config.litellm_host = "http://127.0.0.1:1".into();
    config.ollama_host = "http://127.0.0.1:1".into();
    let state = AppState::new(config, pool.clone(), None);
    assert!(state.existing_mcp().is_none());
    for enabled in [false, true] {
        let row = chats::update_chat(&pool, chat, None, Some(enabled), None, None, None)
            .await
            .unwrap()
            .unwrap();
        session::build(
            &state,
            &row,
            Uuid::new_v4(),
            Some(("A draft", None)),
            Mode::Preview,
        )
        .await
        .unwrap();
        assert!(
            state.existing_mcp().is_none(),
            "Preview initialized MCP children"
        );
    }
    sqlx::query("DELETE FROM chats WHERE id=$1")
        .bind(chat)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn catalog_pages_all_references_without_exposing_result_bodies_or_other_chats() {
    let (pool, store, chat, workspace) = fixture().await;
    let lease = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, user) = begin(&store, &lease).await;
    for index in 0..40 {
        let call = format!("call-{index}");
        store
            .append(
                &lease,
                turn,
                &[
                    envelope(&format!("envelope-{index}"), &call, false),
                    result(
                        &format!("資料-{index}"),
                        &call,
                        if index == 0 {
                            "Error: original sensitive error detail"
                        } else {
                            "original sensitive success body"
                        },
                    ),
                ],
            )
            .await
            .unwrap();
    }
    let mut content = String::new();
    let mut offset = 0;
    let mut cursor = None::<String>;
    loop {
        let page = match &cursor {
            Some(id) => store.evidence(id, offset, 7).await.unwrap(),
            None => store.catalog(offset, 7).await.unwrap(),
        };
        if cursor.is_none() {
            store
                .append(
                    &lease,
                    turn,
                    &[
                        envelope("later-envelope", "later-call", false),
                        result(
                            "later-result",
                            "later-call",
                            "A result appended during pagination",
                        ),
                    ],
                )
                .await
                .unwrap();
            cursor = Some(page.id.clone());
        }
        assert!(page.id.starts_with("catalog:"));
        assert!(page.content.chars().count() <= 7);
        content.push_str(&page.content);
        match page.next {
            Some(next) => offset = next,
            None => {
                assert_eq!(content.chars().count() as u64, page.total);
                break;
            }
        }
    }
    assert!(!content.contains("sensitive"));
    let rows = content
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 40);
    assert_eq!(rows[0]["id"], "資料-0");
    assert_eq!(rows[0]["name"], "read_file");
    assert_eq!(rows[0]["outcome"], "error");
    assert_eq!(rows[39]["id"], "資料-39");
    assert_eq!(
        store
            .catalog(0, u64::MAX)
            .await
            .unwrap()
            .content
            .lines()
            .count(),
        41
    );
    assert!(
        Store::new(pool.clone(), chat, Some(Uuid::new_v4()))
            .catalog(0, 100)
            .await
            .is_err()
    );
    assert!(
        Store::new(pool.clone(), Uuid::new_v4(), Some(workspace))
            .catalog(0, 100)
            .await
            .is_err()
    );
    assert!(
        Store::new(pool.clone(), chat, None)
            .catalog(0, 100)
            .await
            .is_err()
    );
    assert!(store.catalog(0, 0).await.is_err());
    store.delete_message(&lease, user).await.unwrap();
    assert!(
        store
            .evidence(cursor.as_ref().unwrap(), 0, 100)
            .await
            .is_err(),
        "Deletion invalidates the snapshot rather than shifting pages"
    );
    store.release(&lease).await.unwrap();
    sqlx::query("DELETE FROM chats WHERE id=$1")
        .bind(chat)
        .execute(&pool)
        .await
        .unwrap();
}

/// The live failure this fixes: a generation that lost its lease left
/// `chat_turns` running with no `completed_at`, for ever, and the prose it had
/// streamed was never written as an interruption writes it.
#[tokio::test]
async fn a_lost_lease_still_closes_its_own_turn_and_keeps_what_it_produced() {
    let (pool, store, chat, _) = fixture().await;
    let lost = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lost).await;
    store
        .append(&lost, turn, &[envelope("mutation", "call", true)])
        .await
        .unwrap();
    store
        .publish(&lost, turn, "Half an ans", None)
        .await
        .unwrap();
    expire(&pool, chat).await;
    store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    assert!(
        matches!(
            store
                .finish(&lost, turn, "Half an answer.", None, true, None)
                .await,
            Err(Error::LeaseLost)
        ),
        "the fenced close is the one that is refused, and it is why the row was left running"
    );

    let partial = ReplayMessage::from(&Message::assistant("Half an answer."));
    assert!(
        store
            .settle(
                turn,
                Some("Half an answer.\n\n[Response interrupted]"),
                None,
                Some(&partial),
            )
            .await
            .unwrap(),
        "a running turn whose lease is gone must still be closable"
    );

    let (status, completed) = turn_state(&pool, chat, turn).await;
    assert_eq!(
        status, "interrupted",
        "a turn nobody owns must not stay running"
    );
    assert!(
        completed.is_some(),
        "a closed turn must carry the time it stopped"
    );
    assert_eq!(
        visible(&pool, chat, turn).await.as_deref(),
        Some("Half an answer.\n\n[Response interrupted]"),
        "the reader must keep the prose the generation had already streamed"
    );
    let history = store.load().await.unwrap();
    let kept = history
        .entries
        .iter()
        .find(|entry| entry.message.content.as_deref() == Some("Half an answer."))
        .expect("the partial must be stored the way an ordinary interruption stores it");
    assert!(
        kept.consumed,
        "the model has already seen the prose it wrote"
    );
    let notice = history
        .entries
        .iter()
        .find(|entry| {
            entry
                .message
                .content
                .as_deref()
                .is_some_and(|content| content.contains("may have changed external state"))
        })
        .expect("a call with no result must still get its uncertain outcome");
    assert_eq!(notice.message.role, Role::Tool);
    assert!(!notice.consumed, "the next run has to replay the warning");
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn settling_never_unfences_the_writes_the_lost_lease_owned() {
    let (pool, store, chat, _) = fixture().await;
    let lost = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lost).await;
    expire(&pool, chat).await;
    store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    for refused in refusals(&store, &lost, turn).await {
        assert!(
            matches!(refused, Some(Error::LeaseLost)),
            "a lost lease must never be allowed to write: {refused:?}"
        );
    }
    assert!(
        store
            .settle(turn, Some("Stopped."), None, None)
            .await
            .unwrap()
    );
    for refused in refusals(&store, &lost, turn).await {
        assert!(
            matches!(refused, Some(Error::LeaseLost)),
            "closing a turn must not hand its lost lease the right to write: {refused:?}"
        );
    }
    let history = store.load().await.unwrap();
    assert!(
        !history.entries.iter().any(|entry| entry.id == "stale"),
        "a fenced-out write must never reach the conversation"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

async fn refusals(store: &Store, lease: &Lease, turn: Uuid) -> Vec<Option<Error>> {
    vec![
        store
            .append(lease, turn, &[envelope("stale", "call", false)])
            .await
            .err(),
        store.publish(lease, turn, "Stale prose", None).await.err(),
        store.complete(lease, turn, "Stale prose", None).await.err(),
        store
            .finish(lease, turn, "Stale prose", None, true, None)
            .await
            .err(),
        store
            .create_message(lease, "assistant", "Stale prose", None)
            .await
            .err(),
    ]
}

#[tokio::test]
async fn settling_the_same_turn_twice_changes_nothing() {
    let (pool, store, chat, _) = fixture().await;
    let lost = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (turn, _) = begin(&store, &lost).await;
    store
        .append(&lost, turn, &[envelope("mutation", "call", true)])
        .await
        .unwrap();
    expire(&pool, chat).await;
    let partial = ReplayMessage::from(&Message::assistant("Half an answer."));
    assert!(
        store
            .settle(turn, Some("Stopped."), None, Some(&partial))
            .await
            .unwrap()
    );
    let closed = turn_state(&pool, chat, turn).await;
    let entries = store.load().await.unwrap().entries.len();
    assert!(
        !store
            .settle(turn, Some("Stopped again."), None, Some(&partial))
            .await
            .unwrap(),
        "a turn that is already closed must be left exactly as it is, not closed twice"
    );
    assert_eq!(
        turn_state(&pool, chat, turn).await,
        closed,
        "a second close must not move the time the turn stopped"
    );
    assert_eq!(
        store.load().await.unwrap().entries.len(),
        entries,
        "a second close must not duplicate the partial or the uncertain outcomes"
    );
    assert_eq!(
        visible(&pool, chat, turn).await.as_deref(),
        Some("Stopped."),
        "a second close must not overwrite what the reader already has"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

/// The safety argument for closing without the lease: a turn id belongs to one
/// generation, so this must never reach a turn a new owner is running.
#[tokio::test]
async fn a_lost_lease_settles_only_the_turn_it_opened() {
    let (pool, store, chat, _) = fixture().await;
    let lost = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (mine, _) = begin(&store, &lost).await;
    expire(&pool, chat).await;
    let current = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let theirs = Uuid::new_v4();
    let user = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO messages (id,chat_id,role,content) VALUES ($1,$2,'user','Next request')",
    )
    .bind(user)
    .bind(chat)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO chat_turns (id,chat_id,user_message_id,fence) VALUES ($1,$2,$3,$4)")
        .bind(theirs)
        .bind(chat)
        .bind(user)
        .bind(current.fence)
        .execute(&pool)
        .await
        .unwrap();
    assert!(store.settle(mine, None, None, None).await.unwrap());
    assert_eq!(
        turn_state(&pool, chat, theirs).await.0,
        "running",
        "closing an abandoned turn must never stop the generation that took the chat over"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

/// The race the scoping test above does not run. A successor's `begin`
/// recovers this turn — closes it `interrupted`, writes the uncertain outcome
/// for the call with no result, consumes the rest — before the generation that
/// lost the lease gets to `settle`. Recovery has no prose to keep and the old
/// generation does, so what it streamed must still land, exactly once, with the
/// outcome recovery already wrote left alone and the successor's turn untouched.
#[tokio::test]
async fn a_turn_recovered_by_its_successor_still_keeps_what_it_streamed() {
    let (pool, store, chat, _) = fixture().await;
    let lost = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (mine, _) = begin(&store, &lost).await;
    store
        .append(&lost, mine, &[envelope("mutation", "call", true)])
        .await
        .unwrap();
    store
        .publish(&lost, mine, "Half an ans", None)
        .await
        .unwrap();
    expire(&pool, chat).await;

    let current = store.acquire(Uuid::new_v4(), LIFETIME).await.unwrap();
    let (theirs, _) = begin(&store, &current).await;
    let recovered = turn_state(&pool, chat, mine).await;
    assert_eq!(
        recovered.0, "interrupted",
        "the successor's begin recovers every running turn, this one included"
    );

    let partial = ReplayMessage::from(&Message::assistant("Half an answer."));
    assert!(
        store
            .settle(
                mine,
                Some("Half an answer.\n\n[Response interrupted]"),
                None,
                Some(&partial),
            )
            .await
            .unwrap(),
        "a turn recovery closed first must still take the prose its generation streamed"
    );

    assert_eq!(
        visible(&pool, chat, mine).await.as_deref(),
        Some("Half an answer.\n\n[Response interrupted]"),
        "the reader must keep what was streamed, not the snapshot recovery left"
    );
    assert_eq!(
        turn_state(&pool, chat, mine).await,
        recovered,
        "settling after recovery must not move the time recovery stopped the turn"
    );
    let history = store.load().await.unwrap();
    let kept = history
        .entries
        .iter()
        .find(|entry| entry.message.content.as_deref() == Some("Half an answer."))
        .expect("the partial is stored the way an ordinary interruption stores it");
    assert!(
        kept.consumed,
        "the model has already seen the prose it wrote"
    );
    let notices: Vec<_> = history
        .entries
        .iter()
        .filter(|entry| {
            entry
                .message
                .content
                .as_deref()
                .is_some_and(|content| content.contains("may have changed external state"))
        })
        .collect();
    assert_eq!(
        notices.len(),
        1,
        "recovery already wrote the uncertain outcome; settling must not write it again"
    );
    assert!(
        !notices[0].consumed,
        "the next run still has to replay the warning recovery left"
    );
    assert_eq!(
        turn_state(&pool, chat, theirs).await.0,
        "running",
        "the successor's own turn is not this generation's to touch"
    );

    let entries = history.entries.len();
    assert!(
        !store
            .settle(mine, Some("Stopped again."), None, Some(&partial))
            .await
            .unwrap(),
        "a turn already given its prose has nothing left to take"
    );
    assert_eq!(
        visible(&pool, chat, mine).await.as_deref(),
        Some("Half an answer.\n\n[Response interrupted]"),
        "a second close must not overwrite what the reader already has"
    );
    assert_eq!(
        store.load().await.unwrap().entries.len(),
        entries,
        "a second close must not duplicate the partial"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}

#[tokio::test]
async fn closing_a_session_whose_lease_was_lost_settles_its_turn() {
    use zone_server::services::chat::session::Session;
    use zone_server::state::AppState;
    let (pool, _, chat, workspace) = fixture().await;
    let mut config = common::test_config();
    config.litellm_host = "http://127.0.0.1:1".into();
    config.ollama_host = "http://127.0.0.1:1".into();
    let state = AppState::new(config, pool.clone(), None);
    let mut session = Session::acquire(&state, chat, workspace, Uuid::new_v4())
        .await
        .unwrap();
    session
        .store
        .begin(
            &session.lease,
            session.turn,
            Uuid::new_v4(),
            "Latest request",
            None,
            ReplayMessage::from(&Message::user("Latest request")),
        )
        .await
        .unwrap();
    expire(&pool, chat).await;
    session
        .close()
        .await
        .expect("a session whose lease is gone must still close");
    let (status, completed) = turn_state(&pool, chat, session.turn).await;
    assert_eq!(
        status, "interrupted",
        "the one exit every generation takes must settle the row a lost lease left running"
    );
    assert!(
        completed.is_some(),
        "a closed turn must carry the time it stopped"
    );
    chats::delete_chat(&pool, chat).await.unwrap();
}
