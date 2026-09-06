//! Conversation context acceptance against the authenticated router and WebSocket.
mod common;

use axum::http::StatusCode;
use common::context::{
    Harness, MODEL, answer, finish, ordinary, send, state, successful, summaries, summary_response,
    usage,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;
use zone_core::context::{
    self, ContextBreakdown, ContextSource, ContextStatus, ContextUsage, Entry, Policy,
};
use zone_core::llm::Message;

#[test]
fn shared_context_fixture_matches_rust_serialization() {
    let expected = ContextUsage {
        model: "qwen3.8:27b".into(),
        used: 3064,
        limit: Some(32768),
        reserved: 4096,
        threshold: Some(22938),
        remaining: Some(19874),
        estimated: true,
        incomplete: false,
        source: ContextSource::Configured,
        status: ContextStatus::Ready,
        breakdown: ContextBreakdown {
            instructions: 800,
            conversation: 1200,
            tools: 600,
            results: 400,
            summary: 0,
            attachments: Some(0),
            overhead: 64,
        },
        revision: 0,
        compacted_messages: 0,
        updated_at: "2026-09-06T00:00:00Z".into(),
        reason: None,
    };
    let fixture: Value = serde_json::from_str(include_str!("fixtures/context.json")).unwrap();
    assert_eq!(serde_json::to_value(&expected).unwrap(), fixture);
    assert_eq!(usage(&fixture), expected);
}

#[tokio::test]
async fn preview_is_authorized_read_only_and_reserves_usable_output_at_4096_context() {
    let harness = Harness::new(Some(4096), false, vec![answer("Done")]).await;
    let preview = harness.preview("A short ordinary text draft", None).await;
    preview.assert_status(StatusCode::OK);
    let context = usage(&preview.json_value()["context"]);
    assert_eq!(context.limit, Some(4096));
    assert_eq!(context.reserved, 1024);
    assert_eq!(context.threshold, Some(2458));
    assert_eq!(context.source, ContextSource::Configured);
    assert_eq!(context.status, ContextStatus::Ready);
    assert!(context.estimated);
    assert!(
        !context.incomplete,
        "ordinary text preview must be usable: {context:?}"
    );
    assert!(harness.history().await.entries.is_empty());
    assert!(
        harness.requests().await.is_empty(),
        "preview cannot call models"
    );
    harness
        .client
        .post_json_auth(
            &format!("/api/chats/{}/context", harness.chat),
            &json!({"content":"private"}),
            "invalid-token",
        )
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    let frames = harness.turn("A short ordinary text draft").await;
    successful(&frames);
    let requests = harness.requests().await;
    let inference = ordinary(&requests);
    assert_eq!(inference.len(), 1);
    assert_eq!(inference[0]["max_tokens"], context.reserved);
    assert_eq!(inference[0]["num_ctx"], 4096);
    let live = frames
        .iter()
        .find(|frame| frame["type"] == "context" && !frame["message_id"].is_null())
        .expect("live context frame");
    let live = usage(&live["usage"]);
    assert_eq!(live.reserved, context.reserved);
    assert_eq!(
        live.used, context.used,
        "preview and actual first request must share preparation"
    );
}

#[tokio::test]
async fn cold_text_history_compacts_before_inference_with_bounded_batched_summaries() {
    let harness = Harness::new(Some(4096), false, vec![answer("Done")]).await;
    let mut originals = Vec::new();
    for index in 0..80 {
        let content = format!("CANONICAL_ROW_{index}: {}", "historical detail ".repeat(5));
        let id = harness
            .seed(if index % 2 == 0 { "user" } else { "assistant" }, &content)
            .await;
        originals.push((id, content));
    }
    let current = "Actual current request: use corrected path and do not repeat completed work.";
    let frames = harness.turn(current).await;
    successful(&frames);
    let requests = harness.requests().await;
    let summaries = summaries(&requests);
    assert!(
        !summaries.is_empty(),
        "cold model must compact before ordinary inference"
    );
    assert!(
        summaries.len() < 80,
        "small historical messages must be batched"
    );
    assert_eq!(requests.first().unwrap()["stream"], false);
    let mut batched = false;
    for request in summaries {
        assert_eq!(request["num_ctx"], 4096);
        assert_eq!(request["max_tokens"], 1024);
        assert!(request.get("tools").is_none());
        let sources: Value =
            serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
        batched |= sources["sources"].as_array().unwrap().len() > 1;
        let entries: Vec<_> = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, value)| Entry {
                id: index.to_string(),
                message: serde_json::from_value::<Message>(value.clone()).unwrap(),
                preserve: true,
                consumed: false,
            })
            .collect();
        let budget = Policy {
            limit: Some(4096),
            reserved: 1024,
            source: ContextSource::Configured,
        };
        assert!(
            context::estimate(MODEL, &entries, None, &budget, None).used
                <= budget.threshold().unwrap()
        );
    }
    assert!(batched);
    let inference = ordinary(&requests);
    assert_eq!(inference.len(), 1);
    assert_eq!(inference[0]["num_ctx"], 4096);
    let messages = inference[0]["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message["role"] == "user" && message["content"] == current)
    );
    assert!(messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|content| content.contains("Historical conversation record"))
    }));
    assert!(
        !messages
            .iter()
            .filter(|message| message["role"] == "system")
            .any(|message| message["content"]
                .as_str()
                .unwrap_or_default()
                .contains("CANONICAL_ROW_")),
        "history must never become trusted instructions"
    );
    let history = harness.history().await;
    let checkpoint = history.summary.as_ref().expect("durable checkpoint");
    assert!(
        !checkpoint
            .entries
            .contains(history.latest_user.as_ref().unwrap())
    );
    for (_, original) in &originals {
        assert!(
            history
                .entries
                .iter()
                .any(|entry| entry.message.content.as_ref() == Some(original)),
            "canonical row lost: {original}"
        );
    }
    assert_context_lifecycle(&frames, harness.chat);
    assert!(
        frames
            .iter()
            .any(|frame| frame["type"] == "context" && frame["usage"]["status"] == "compacting")
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame["type"] == "context" && frame["usage"]["status"] == "compacted")
    );
}

fn assert_context_lifecycle(frames: &[Value], chat: Uuid) {
    let start = frames
        .iter()
        .position(|frame| frame["type"] == "message_start")
        .unwrap();
    let finish = frames
        .iter()
        .position(|frame| frame["type"] == "message_end")
        .unwrap();
    let id = &frames[start]["message_id"];
    let mut count = 0;
    for (index, frame) in frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame["type"] == "context")
    {
        assert_eq!(frame["chat_id"], chat.to_string());
        if !frame["message_id"].is_null() {
            assert_eq!(&frame["message_id"], id);
            assert!(index > start && index < finish);
        }
        usage(&frame["usage"]);
        let public = frame.to_string();
        assert!(!public.contains("CANONICAL_ROW_"));
        assert!(!public.contains("Historical exchanges retained"));
        count += 1;
    }
    assert!(
        count >= 2,
        "pre-request and completed replay usage required"
    );
}

#[tokio::test]
async fn more_than_fifty_plain_messages_are_replayed_without_a_row_window() {
    let harness = Harness::new(Some(100_000), false, vec![answer("Done")]).await;
    for index in 0..64 {
        harness
            .seed(
                if index % 2 == 0 { "user" } else { "assistant" },
                &format!("UNIQUE_HISTORY_{index}"),
            )
            .await;
    }
    successful(&harness.turn("Read the entire conversation").await);
    let requests = harness.requests().await;
    assert!(summaries(&requests).is_empty());
    let request = ordinary(&requests)[0];
    for index in 0..64 {
        assert!(
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["content"] == format!("UNIQUE_HISTORY_{index}")),
            "row {index} silently dropped"
        );
    }
}

#[tokio::test]
async fn failed_and_oversized_summaries_preserve_full_history_without_advancing_checkpoint() {
    for malformed in [
        String::new(),
        "{}".into(),
        state().replace("Continue the actual current request", &"x".repeat(9_000)),
    ] {
        let harness = Harness::new(Some(4096), false, vec![answer("Should not run")]).await;
        let evidence = format!("PERSISTENT_OLD_EVIDENCE {}", "x".repeat(12_000));
        harness.seed("user", &evidence).await;
        harness.script.summarize(summary_response(malformed));
        let frames = harness.turn("Current short request").await;
        assert!(frames.iter().any(|frame| frame["type"] == "error"));
        let history = harness.history().await;
        assert!(history.summary.is_none());
        assert!(
            history
                .entries
                .iter()
                .any(|entry| entry.message.content.as_ref() == Some(&evidence))
        );
        assert!(ordinary(&harness.requests().await).is_empty());
    }
}

#[tokio::test]
async fn cancelled_summary_leaves_no_checkpoint_and_can_resume_from_canonical_evidence() {
    let harness = Harness::new(Some(4096), false, vec![answer("Resumed")]).await;
    let evidence = "CANCEL_SOURCE ".repeat(1_000);
    harness.seed("user", &evidence).await;
    harness
        .script
        .summarize(summary_response(state()).set_delay(Duration::from_secs(30)));
    let mut socket = harness.connect().await;
    send(
        &mut socket,
        json!({"type":"send","content":"Compact this history"}),
    )
    .await;
    harness.until_requests(1).await;
    assert_eq!(harness.requests().await[0]["stream"], false);
    send(&mut socket, json!({"type":"cancel"})).await;
    let frames = finish(&mut socket).await;
    assert!(
        frames.iter().any(|frame| frame["type"] == "cancelled"),
        "{frames:?}"
    );
    assert!(harness.history().await.summary.is_none());
    assert!(
        harness
            .history()
            .await
            .entries
            .iter()
            .any(|entry| entry.message.content.as_ref() == Some(&evidence))
    );
    harness.script.summarize(summary_response(state()));
    send(
        &mut socket,
        json!({"type":"send","content":"Resume without repeating completed work"}),
    )
    .await;
    successful(&finish(&mut socket).await);
}

#[tokio::test]
async fn unknown_capacity_and_images_are_reported_honestly_without_model_calls_from_preview() {
    let harness = Harness::new(None, false, vec![answer("Done")]).await;
    let response = harness.preview("Ordinary draft", None).await;
    response.assert_status(StatusCode::OK);
    let context = usage(&response.json_value()["context"]);
    assert_eq!(context.limit, None);
    assert_eq!(context.threshold, None);
    assert_eq!(context.source, ContextSource::Unknown);
    assert_eq!(context.status, ContextStatus::Unavailable);
    let response=harness.preview("Inspect this",Some(json!({"attachments":[{"name":"pixel.png","mime":"image/png","url":"data:image/png;base64,AA=="}]}))).await;
    response.assert_status(StatusCode::OK);
    let context = usage(&response.json_value()["context"]);
    assert!(context.incomplete);
    assert_eq!(context.breakdown.attachments, None);
    assert!(harness.requests().await.is_empty());
    successful(&harness.turn("Hello").await);
    assert!(
        harness
            .requests()
            .await
            .iter()
            .all(|request| request.get("num_ctx").is_none())
    );
}

#[tokio::test]
async fn database_lease_blocks_socket_and_rest_writes_before_a_user_message_is_saved() {
    let harness = Harness::new(Some(32768), false, vec![answer("After release")]).await;
    let store = harness.store();
    let lease = store
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .unwrap();
    let frames = harness
        .turn("Must not be accepted under another owner")
        .await;
    assert!(frames.iter().any(|frame| frame["type"] == "error"));
    assert!(harness.requests().await.is_empty());
    assert!(harness.history().await.entries.is_empty());
    harness
        .client
        .post_json_auth(
            &format!("/api/chats/{}/messages", harness.chat),
            &json!({"role":"user","content":"Cannot splice into active generation"}),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::CONFLICT);
    assert!(harness.history().await.entries.is_empty());
    store.release(&lease).await.unwrap();
    successful(&harness.turn("Now accepted").await);
}

#[tokio::test]
async fn parallel_socket_generations_serialize_before_persistence_and_allow_immediate_followup() {
    let harness = Harness::new(
        Some(32768),
        false,
        vec![
            answer("First complete").set_delay(Duration::from_secs(2)),
            answer("Followup complete"),
        ],
    )
    .await;
    let mut first = harness.connect().await;
    let mut second = harness.connect().await;
    send(
        &mut first,
        json!({"type":"send","content":"Accepted first user"}),
    )
    .await;
    harness.until_requests(1).await;
    send(
        &mut second,
        json!({"type":"send","content":"Rejected racing user"}),
    )
    .await;
    let rejected = finish(&mut second).await;
    assert!(rejected.iter().any(|frame| frame["type"] == "error"));
    assert!(
        !harness
            .history()
            .await
            .entries
            .iter()
            .any(|entry| { entry.message.content.as_deref() == Some("Rejected racing user") })
    );
    assert_eq!(ordinary(&harness.requests().await).len(), 1);
    successful(&finish(&mut first).await);
    send(
        &mut first,
        json!({"type":"send","content":"Accepted immediate followup"}),
    )
    .await;
    successful(&finish(&mut first).await);
    let requests = harness.requests().await;
    let requests = ordinary(&requests);
    assert_eq!(requests.len(), 2);
    assert!(!requests[1].to_string().contains("Rejected racing user"));
    assert!(requests[1].to_string().contains("Accepted first user"));
}

#[tokio::test]
async fn deleting_covered_legacy_message_invalidates_summary_before_next_request() {
    let harness = Harness::new(Some(4096), false, vec![answer("First"), answer("Second")]).await;
    let deleted = harness
        .seed(
            "user",
            &format!("DELETED_SECRET {}", "evidence ".repeat(1_000)),
        )
        .await;
    let mut summary: Value = serde_json::from_str(&state()).unwrap();
    summary["evidence"] = json!(["DELETED_SECRET"]);
    harness
        .script
        .summarize(summary_response(summary.to_string()));
    successful(&harness.turn("First current request").await);
    assert!(harness.history().await.summary.is_some());
    harness
        .client
        .delete_auth(
            &format!("/api/chats/{}/messages/{deleted}", harness.chat),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let count = harness.requests().await.len();
    harness.script.summarize(summary_response(state()));
    successful(&harness.turn("Continue after deleting that message").await);
    for request in &harness.requests().await[count..] {
        assert!(
            !request.to_string().contains("DELETED_SECRET"),
            "deleted context leaked into request: {request}"
        );
    }
    assert!(!harness.history().await.entries.iter().any(|entry| {
        entry
            .message
            .content
            .as_deref()
            .is_some_and(|content| content.contains("DELETED_SECRET"))
    }));
}

#[test]
fn explicit_database_selection_supports_ci_without_a_production_fallback() {
    let configured = "postgres://test:only@127.0.0.1/context_test".to_owned();
    let override_database = "postgres://test:only@127.0.0.1/override_test".to_owned();
    assert_eq!(
        common::select_context_database(None, Some(configured.clone())).unwrap(),
        configured
    );
    assert_eq!(
        common::select_context_database(Some(override_database.clone()), Some(configured)).unwrap(),
        override_database
    );
    let error = common::select_context_database(None, None).unwrap_err();
    assert!(error.contains("TEST_DATABASE_URL"));
    assert!(error.contains("isolated migrated test database"));
    assert!(common::select_context_database(Some(String::new()), None).is_err());
}
