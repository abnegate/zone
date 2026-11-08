//! A person cannot pass a message they typed off as one an automation sent.
//!
//! The console badges a message whose metadata carries `source`, telling the
//! reader nobody in the chat typed it. That is only true if the server is the
//! one writing `source`, so both ways a client stores a message refuse it.

use crate::common;
use crate::common::context::{Harness, answer, finish, successful};
use crate::reminder_turn::{dispatcher, make_due, until_stored};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

const TYPED: &str = "I typed this myself.";

async fn stored(harness: &Harness) -> Vec<(String, String, Value)> {
    sqlx::query_as::<_, (String, String, Value)>(
        "SELECT role, content, COALESCE(metadata, '{}'::jsonb) FROM messages \
         WHERE chat_id = $1 ORDER BY created_at, id",
    )
    .bind(harness.chat)
    .fetch_all(&harness.pool)
    .await
    .expect("the chat's messages are readable")
}

fn claims_automation(metadata: &Value) -> bool {
    metadata.get("source").is_some() || metadata.get("reminder_id").is_some()
}

#[tokio::test]
async fn a_message_posted_over_http_cannot_claim_a_reminder_sent_it() {
    let harness = Harness::new(Some(32_768), false, vec![]).await;
    let path = format!("/api/chats/{}/messages", harness.chat);

    for metadata in [
        json!({"source": "reminder"}),
        json!({"reminder_id": Uuid::new_v4()}),
        json!({"source": "auto_project", "attachments": []}),
    ] {
        let response = harness
            .client
            .post_json_auth(
                &path,
                &json!({"role": "user", "content": TYPED, "metadata": metadata}),
                &harness.token,
            )
            .await;
        response.assert_status(StatusCode::BAD_REQUEST);
    }
    assert!(
        stored(&harness).await.is_empty(),
        "a refused message is not stored at all"
    );

    harness
        .client
        .post_json_auth(
            &path,
            &json!({"role": "user", "content": TYPED, "metadata": {"web_search": true}}),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    let messages = stored(&harness).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, TYPED);
    assert!(
        !claims_automation(&messages[0].2),
        "a message with no automation key claims none: {:?}",
        messages[0].2
    );
    assert_eq!(
        messages[0].2.get("web_search"),
        Some(&json!(true)),
        "ordinary client metadata is still stored: {:?}",
        messages[0].2
    );
}

#[tokio::test]
async fn a_message_sent_over_the_socket_cannot_claim_a_reminder_sent_it() {
    let harness = Harness::new(
        Some(32_768),
        false,
        vec![
            answer("Answered what you typed."),
            answer("Answered the reminder."),
        ],
    )
    .await;

    let mut socket = harness.connect().await;
    common::context::send(
        &mut socket,
        json!({
            "type": "send",
            "content": TYPED,
            "metadata": {"source": "reminder", "reminder_id": Uuid::new_v4()},
        }),
    )
    .await;
    let frames = finish(&mut socket).await;
    let last = frames.last().expect("the socket answers the send");
    assert_eq!(last["type"], "error", "the send is refused: {frames:?}");
    assert_eq!(last["message"], "Message metadata may not set `source`");
    assert!(
        !frames.iter().any(|frame| frame["type"] == "message_saved"),
        "nothing is saved for a refused send: {frames:?}"
    );
    assert!(stored(&harness).await.is_empty());

    common::context::send(
        &mut socket,
        json!({"type": "send", "content": TYPED, "metadata": {"reminder_id": Uuid::new_v4()}}),
    )
    .await;
    let frames = finish(&mut socket).await;
    let last = frames.last().expect("the socket answers the send");
    assert_eq!(last["type"], "error", "the send is refused: {frames:?}");
    assert_eq!(
        last["message"],
        "Message metadata may not set `reminder_id`"
    );
    assert!(stored(&harness).await.is_empty());

    common::context::send(&mut socket, json!({"type": "send", "content": TYPED})).await;
    successful(&finish(&mut socket).await);
    let _ = socket.close(None).await;

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
            content: "Deploy queue".into(),
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: Some("Check the deploy queue.".into()),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .expect("a prompt-bearing reminder is storable");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    make_due(&harness, reminder).await;
    let worker = dispatcher(&harness);
    let messages = until_stored(&harness, reminder, 4).await;
    worker.abort();

    let (role, content, metadata) = &messages[0];
    assert_eq!((role.as_str(), content.as_str()), ("user", TYPED));
    assert!(
        !claims_automation(metadata),
        "what the person typed is theirs: {metadata}"
    );

    let (role, content, metadata) = &messages[2];
    assert_eq!(
        (role.as_str(), content.as_str()),
        ("user", "Check the deploy queue.")
    );
    assert_eq!(
        metadata["source"], "reminder",
        "the server still marks the turn a reminder sent: {metadata}"
    );
    assert_eq!(metadata["reminder_id"], Value::from(reminder.to_string()));
}
