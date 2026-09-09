//! Generation boundary contracts exercised through the real WebSocket handler.

mod common;

use common::context::{Harness, answer, calls, delta, finish, next, send, successful, tool};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn stop_tokens_never_reach_the_live_or_persisted_answer() {
    let harness = Harness::new(
        Some(200_000),
        false,
        vec![answer("Visible answer<|im_end|>leaked next turn")],
    )
    .await;

    let frames = harness.turn("Stop at the model template boundary").await;
    successful(&frames);
    let chunks = frames
        .iter()
        .filter(|frame| frame["type"] == "chunk")
        .map(|frame| frame["content"].as_str().unwrap())
        .collect::<String>();
    assert_eq!(chunks, "Visible answer");
    let end = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .unwrap();
    assert_eq!(end["content"], "Visible answer");

    let history = harness.history().await;
    let answer = history
        .entries
        .iter()
        .rev()
        .find(|entry| entry.message.role == zone_core::llm::Role::Assistant)
        .unwrap();
    assert_eq!(answer.message.content.as_deref(), Some("Visible answer"));
}

#[tokio::test]
async fn incomplete_stop_prefix_is_not_lost_at_the_end_of_a_stream() {
    let harness = Harness::new(Some(200_000), false, vec![answer("Visible<|im_")]).await;

    let frames = harness.turn("Preserve an incomplete template prefix").await;
    successful(&frames);
    let chunks = frames
        .iter()
        .filter(|frame| frame["type"] == "chunk")
        .map(|frame| frame["content"].as_str().unwrap())
        .collect::<String>();
    assert_eq!(chunks, "Visible<|im_");
    let end = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .unwrap();
    assert_eq!(end["content"], "Visible<|im_");

    let stored: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(harness.chat)
    .fetch_one(&harness.pool)
    .await
    .unwrap();
    assert_eq!(stored, "Visible<|im_");
}

#[tokio::test]
async fn workspace_knowledge_is_injected_as_untrusted_retrieved_context() {
    let harness = Harness::new(Some(200_000), false, vec![answer("Used workspace context")]).await;
    let created = harness
        .client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": harness.workspace,
                "title": "Frozen release runbook",
                "content": "The FROZEN_DEPLOY_SIGNAL confirms this release is pinned.",
                "category": "documentation",
                "tags": ["release"]
            }),
            &harness.token,
        )
        .await;
    created.assert_status(axum::http::StatusCode::CREATED);

    let frames = harness
        .turn("What does FROZEN_DEPLOY_SIGNAL confirm?")
        .await;
    successful(&frames);
    let requests = harness.requests().await;
    let prompt = requests.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "system")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(prompt.contains("<retrieved_context>"), "{prompt}");
    assert!(
        prompt.contains("[knowledge] Frozen release runbook"),
        "{prompt}"
    );
    assert!(prompt.contains("knowledge://"), "{prompt}");
    assert!(prompt.contains("FROZEN_DEPLOY_SIGNAL"), "{prompt}");
    assert!(prompt.contains("untrusted source data"), "{prompt}");
}

#[tokio::test]
async fn oversized_provider_output_is_bounded_and_marked_in_history() {
    let harness = Harness::new(Some(200_000), false, vec![answer(&"x".repeat(100_001))]).await;

    let frames = harness
        .turn("Return an answer larger than the wire limit")
        .await;
    successful(&frames);
    assert!(!frames.iter().any(|frame| frame["type"] == "chunk"));
    let end = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .unwrap();
    assert_eq!(
        end["content"],
        "\n\n[Response truncated due to length limit]"
    );

    let history = harness.history().await;
    assert!(
        history
            .entries
            .iter()
            .filter_map(|entry| entry.message.content.as_deref())
            .all(|content| content.len() <= 100_000),
        "canonical replay must not retain the rejected provider payload"
    );
    let stored: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(harness.chat)
    .fetch_one(&harness.pool)
    .await
    .unwrap();
    assert_eq!(stored, "\n\n[Response truncated due to length limit]");
}

#[tokio::test]
async fn generation_timeout_interrupts_the_turn_without_saving_a_reply() {
    let delayed = answer("This response arrives too late").set_delay(Duration::from_secs(2));
    let mut harness = Harness::new(Some(200_000), false, vec![delayed]).await;
    harness.config.chat.timeout = Duration::from_millis(50);
    harness.restart().await;

    let frames = harness.turn("Wait for the slow provider").await;
    assert_eq!(frames.last().unwrap()["type"], "error");
    assert_eq!(
        frames.last().unwrap()["message"],
        "Response generation timed out"
    );
    assert!(
        harness
            .history()
            .await
            .entries
            .iter()
            .all(|entry| entry.message.role != zone_core::llm::Role::Assistant)
    );
}

#[tokio::test]
async fn generated_media_is_validated_capped_and_persisted_with_reasoning() {
    let mut images = vec![
        json!({"image_url":{"url":"javascript:alert(1)"}}),
        json!({"image_url":{"url":"data:audio/flac;base64,ZmFrZQ=="}}),
    ];
    images.extend(
        (0..9).map(
            |index| json!({"image_url":{"url":format!("data:image/png;base64,aW1hZ2U{index}")}}),
        ),
    );
    let harness = Harness::new(
        Some(200_000),
        false,
        vec![delta(json!({
            "content":"Rendered media",
            "reasoning_content":"Validated provider output.",
            "images":images
        }))],
    )
    .await;

    let frames = harness.turn("Render mixed provider media").await;
    successful(&frames);
    assert!(frames.iter().any(|frame| {
        frame["type"] == "reasoning" && frame["content"] == "Validated provider output."
    }));
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["type"] == "audio")
            .count(),
        1
    );
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame["type"] == "image")
            .count(),
        7
    );
    assert!(
        frames
            .iter()
            .all(|frame| { frame["attachment"]["url"] != "javascript:alert(1)" })
    );
    let end = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .unwrap();
    assert_eq!(end["metadata"]["attachments"].as_array().unwrap().len(), 8);
    assert_eq!(end["metadata"]["reasoning"], "Validated provider output.");
}

#[tokio::test]
async fn workspace_writes_stream_and_persist_the_same_action_receipt() {
    let harness = Harness::new(Some(200_000), true, vec![]).await;
    harness.script.push(calls(
        "",
        vec![tool(
            "create-one",
            "create_task",
            json!({"title":"Receipt contract","description":"Created by the agent"}),
        )],
    ));
    harness.script.push(answer("Task created."));

    let frames = harness.turn("Create the task").await;
    successful(&frames);
    let receipt = frames
        .iter()
        .find(|frame| frame["type"] == "action_receipt")
        .unwrap();
    assert_eq!(receipt["receipt"]["id"], "create-one");
    assert_eq!(receipt["receipt"]["action"], "create_task");
    assert_eq!(receipt["receipt"]["target_label"], "Receipt contract");
    assert_eq!(receipt["receipt"]["success"], true);
    let end = frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .unwrap();
    assert_eq!(end["metadata"]["action_receipts"][0], receipt["receipt"]);
}

#[tokio::test]
async fn cancellation_after_a_tool_starts_persists_a_readable_placeholder() {
    let harness = Harness::new(Some(200_000), true, vec![]).await;
    harness.script.push(calls(
        "",
        vec![tool(
            "delayed",
            "run_command",
            json!({
                "command":"python3",
                "args":["-c","__import__('time').sleep(5)"],
                "timeout_secs":10
            }),
        )],
    ));
    let mut socket = harness.connect().await;
    send(
        &mut socket,
        json!({"type":"send","content":"Run the delayed command"}),
    )
    .await;
    loop {
        let frame = next(&mut socket).await;
        assert_ne!(frame["type"], "error", "{frame}");
        if frame["type"] == "tool_call" {
            send(&mut socket, json!({"type":"cancel"})).await;
            break;
        }
    }
    let frames = finish(&mut socket).await;
    assert_eq!(frames.last().unwrap()["type"], "cancelled");

    let stored: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(harness.chat)
    .fetch_one(&harness.pool)
    .await
    .unwrap();
    assert_eq!(stored, "[Stopped before answering]");
}
