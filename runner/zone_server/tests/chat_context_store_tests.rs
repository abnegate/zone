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
    sqlx::query(
        "UPDATE chat_leases SET expires_at=clock_timestamp()-interval '1 second' WHERE chat_id=$1",
    )
    .bind(chat)
    .execute(&pool)
    .await
    .unwrap();
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
async fn renewal_runs_independently_and_loss_wakes_cancellation() {
    let (pool, store, chat, _) = fixture().await;
    let lifetime = Duration::from_millis(450);
    let lease = store.acquire(Uuid::new_v4(), lifetime).await.unwrap();
    let mut guard = store.keep_alive(lease.clone(), lifetime).unwrap();
    // Model/socket task may be suspended longer than the initial lease lifetime.
    tokio::time::sleep(Duration::from_millis(700)).await;
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
