//! What a chat turn spends is recorded once, after the turn has ended, and
//! read back only by members of the chat's workspace.

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use wiremock::ResponseTemplate;

use crate::common::context::{Harness, MODEL, answer, calls, successful, tool};
use crate::common::eventually::eventually;
use crate::common::{test_email, test_password};

const PROMPT_TOKENS: i64 = 37;
const TOTAL_TOKENS: i64 = 46;

#[derive(Debug, sqlx::FromRow)]
struct Stored {
    turn_id: Uuid,
    model: String,
    prompt_tokens: Option<i64>,
    total_tokens: Option<i64>,
    rounds: i32,
    tool_calls: i32,
    context_tokens: Option<i64>,
}

async fn stored(harness: &Harness) -> Vec<Stored> {
    sqlx::query_as(
        "SELECT turn_id, model, prompt_tokens, total_tokens, rounds, tool_calls, context_tokens \
         FROM chat_token_usage WHERE chat_id = $1 ORDER BY recorded_at, model",
    )
    .bind(harness.chat)
    .fetch_all(&harness.pool)
    .await
    .expect("the chat's recorded spend")
}

/// The chat's rows once `turns` turns have been recorded.
async fn recorded(harness: &Harness, turns: usize) -> Vec<Stored> {
    let settled = eventually(|| async {
        let rows = stored(harness).await;
        rows.iter()
            .map(|row| row.turn_id)
            .collect::<BTreeSet<_>>()
            .len()
            >= turns
    })
    .await;
    let rows = stored(harness).await;
    assert!(settled, "{turns} turns were never recorded: {rows:?}");
    rows
}

fn answered(frames: &[Value]) -> Uuid {
    frames
        .iter()
        .find(|frame| frame["type"] == "message_end")
        .and_then(|frame| frame["message_id"].as_str())
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("the turn ends with its message: {frames:?}"))
}

/// A streamed reply from a provider that reports no usage.
fn unreported(delta: Value) -> ResponseTemplate {
    let reason = if delta["tool_calls"].is_array() {
        "tool_calls"
    } else {
        "stop"
    };
    let content = json!({"model":MODEL,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let finish = json!({"model":MODEL,"choices":[{"index":0,"delta":{},"finish_reason":reason}]});
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(format!(
            "data: {content}\n\ndata: {finish}\n\ndata: [DONE]\n\n"
        ))
}

fn read_file(id: &str) -> Value {
    tool(id, "read_file", json!({"path":"missing.txt"}))
}

#[tokio::test]
async fn each_turn_records_its_rounds_tool_calls_and_tokens_once() {
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    harness
        .script
        .push(calls("Reading", vec![read_file("first")]));
    harness.script.push(answer("Read it"));
    let frames = harness.turn("Read the file").await;
    successful(&frames);
    let first = answered(&frames);

    let rows = recorded(&harness, 1).await;
    let [row] = rows.as_slice() else {
        panic!("one turn on one model is one row: {rows:?}");
    };
    assert_eq!(row.turn_id, first);
    assert_eq!(row.model, MODEL);
    assert_eq!(row.rounds, 2, "the call and the answer are two requests");
    assert_eq!(row.tool_calls, 1);
    assert_eq!(row.prompt_tokens, Some(2 * PROMPT_TOKENS));
    assert_eq!(row.total_tokens, Some(2 * TOTAL_TOKENS));
    assert!(row.context_tokens.is_some(), "{row:?}");

    harness.script.push(answer("Again"));
    let frames = harness.turn("Once more").await;
    successful(&frames);
    let second = answered(&frames);

    let rows = recorded(&harness, 2).await;
    for turn in [first, second] {
        assert_eq!(
            rows.iter().filter(|row| row.turn_id == turn).count(),
            1,
            "each turn is recorded once: {rows:?}"
        );
    }
    assert_eq!(rows.len(), 2, "{rows:?}");
}

#[tokio::test]
async fn rounds_are_counted_when_the_provider_reports_no_usage() {
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    harness.script.push(unreported(json!({
        "content": "Reading",
        "tool_calls": [{"index": 0, "id": "quiet", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"missing.txt\"}"}}],
    })));
    harness
        .script
        .push(unreported(json!({"content": "Read it"})));
    successful(&harness.turn("Read the file").await);

    let rows = recorded(&harness, 1).await;
    let [row] = rows.as_slice() else {
        panic!("{rows:?}");
    };
    assert_eq!(row.rounds, 2, "rounds are zone's own count: {row:?}");
    assert_eq!(row.tool_calls, 1);
    assert_eq!(
        row.total_tokens, None,
        "an unreported turn is not a free one"
    );
}

#[tokio::test]
async fn deleting_the_answer_keeps_what_it_cost_in_the_workspace() {
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    harness.script.push(answer("Spent"));
    let frames = harness.turn("Spend something").await;
    successful(&frames);
    let message = answered(&frames);
    recorded(&harness, 1).await;

    harness
        .client
        .delete_auth(
            &format!("/api/chats/{}/messages/{message}", harness.chat),
            &harness.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let response = harness
        .client
        .get_auth(
            &format!("/api/chats/token-usage?workspace_id={}", harness.workspace),
            &harness.token,
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(
        body["models"],
        json!([{
            "model": MODEL,
            "totals": body["models"][0]["totals"],
        }]),
        "{body}"
    );
    assert_eq!(body["models"][0]["totals"]["turns"], 1, "{body}");
    assert_eq!(
        body["models"][0]["totals"]["total_tokens"], TOTAL_TOKENS,
        "{body}"
    );
}

#[tokio::test]
async fn a_member_reads_the_totals_alone_and_a_stranger_reads_neither_endpoint() {
    let harness = Harness::new(Some(100_000), true, vec![]).await;
    harness.script.push(answer("Spent"));
    successful(&harness.turn("Spend something").await);
    recorded(&harness, 1).await;
    let chat = format!("/api/chats/{}/token-usage", harness.chat);
    let workspace = format!("/api/chats/token-usage?workspace_id={}", harness.workspace);

    let response = harness
        .client
        .get_auth(&format!("{chat}?limit=0"), &harness.token)
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["totals"]["turns"], 1, "{body}");
    assert_eq!(body["turns"], json!([]), "{body}");
    assert_eq!(body["truncated"], true, "{body}");
    harness
        .client
        .get_auth(&workspace, &harness.token)
        .await
        .assert_status(StatusCode::OK);

    let stranger = harness
        .client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value()["access_token"]
        .as_str()
        .expect("the stranger signs up")
        .to_string();
    for uri in [chat, workspace] {
        let response = harness.client.get_auth(&uri, &stranger).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "a stranger read {uri}: {}",
            response.text()
        );
    }
}
