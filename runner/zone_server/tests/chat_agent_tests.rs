//! Exercise the real streaming loop against deterministic provider responses.
mod common;

use futures::StreamExt;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_core::llm::{LlmClient, LlmConfig, Message};
use zone_server::agent::{AgentEvent, AgentRun, ChatTools, MAX_ITERATIONS, WorkspaceScope, run};

const MALFORMED: &str =
    r#"{"id":"call_0","type":"function","function":{"name":"read_file","arguments":{}}"#;

fn text(content: &str) -> Vec<Value> {
    vec![json!({"content": content})]
}

fn call(id: Option<&str>) -> String {
    let mut value = json!({"type":"function","function":{"name":"read_file","arguments":{}}});
    if let Some(id) = id {
        value["id"] = json!(id);
    }
    value.to_string()
}

fn native(id: Option<&str>) -> Vec<Value> {
    vec![
        json!({"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":"read_","arguments":"{"}}]}),
        json!({"tool_calls":[{"index":0,"function":{"name":"file","arguments":"}"}}]}),
    ]
}

async fn exercise(rounds: Vec<Vec<Value>>) -> (Vec<AgentEvent>, Vec<Value>) {
    let provider = MockServer::start().await;
    let responses = Arc::new(Mutex::new(VecDeque::from(rounds)));
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |_request: &Request| {
            let deltas = responses.lock().unwrap().pop_front().expect("unexpected extra completion");
            let mut body = String::new();
            for delta in deltas {
                let chunk = json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
                body.push_str(&format!("data: {chunk}\n\n"));
            }
            let end = json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":"test","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
            body.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_string(body)
        })
        .mount(&provider)
        .await;
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let state = common::create_test_state(common::test_config(), pool);
    let tools = ChatTools::build(
        WorkspaceScope {
            state,
            workspace_id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
        },
        false,
    );
    // Missing `path` fails validation before read_file accesses the filesystem.
    let events = tokio::time::timeout(
        Duration::from_secs(10),
        run(AgentRun {
            llm: LlmClient::new(LlmConfig {
                base_url: provider.uri(),
                ..LlmConfig::default()
            }),
            model: "test".to_string(),
            tools,
            messages: vec![Message::user("Help me inspect a file.")],
        })
        .collect::<Vec<_>>(),
    )
    .await
    .expect("bounded agent loop");
    let requests = provider
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    (events, requests)
}

fn started(events: &[AgentEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolCallStarted { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

fn answer(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Chunk(content) => Some(content.as_str()),
            _ => None,
        })
        .collect()
}

fn assert_replay(request: &Value, count: usize) {
    let messages = request["messages"].as_array().unwrap();
    let calls: Vec<_> = messages
        .iter()
        .filter(|message| message["tool_calls"].is_array())
        .collect();
    assert_eq!(calls.len(), count);
    let mut identifiers = HashSet::new();
    for message in calls {
        assert!(
            message["content"].is_null() || message["content"] == "",
            "tool JSON replayed as prose: {message}"
        );
        for call in message["tool_calls"].as_array().unwrap() {
            let id = call["id"].as_str().unwrap();
            assert!(identifiers.insert(id), "duplicate replay ID: {id}");
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| message["role"] == "tool" && message["tool_call_id"] == id)
                    .count(),
                1
            );
        }
    }
}

#[tokio::test]
async fn malformed_tool_reply_is_corrected_before_execution() {
    let (events, requests) = exercise(vec![
        text(MALFORMED),
        text(&call(Some("call_0"))),
        text("Please provide the file path."),
    ])
    .await;
    assert_eq!(answer(&events), "Please provide the file path.");
    assert_eq!(started(&events).len(), 1);
    assert_eq!(requests.len(), 3);
    assert!(
        !requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
    );
}

#[tokio::test]
async fn malformed_reply_after_a_tool_does_not_end_the_loop() {
    let (events, requests) = exercise(vec![
        text(&json!({"name":"read_file","arguments":{"path":concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")}}).to_string()),
        text(MALFORMED),
        text("Please provide the file path."),
    ])
    .await;
    assert_eq!(answer(&events), "Please provide the file path.");
    assert_eq!(started(&events).len(), 1);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallCompleted { success: true, .. }))
    );
    assert_eq!(requests.len(), 3);
}

#[tokio::test]
async fn repeated_malformed_replies_fail_without_rendering_or_executing_them() {
    let (events, requests) = exercise(vec![text(MALFORMED); MAX_ITERATIONS]).await;
    assert_eq!(answer(&events), "");
    assert!(started(&events).is_empty());
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed(_)))
    );
    assert!(requests.len() > 1 && requests.len() <= MAX_ITERATIONS);
}

#[tokio::test]
async fn repeated_native_and_text_ids_have_unique_replay_pairs() {
    for id in [Some("call_0"), None] {
        let (events, requests) = exercise(vec![
            native(id),
            text(&call(id)),
            text("Please provide a path."),
        ])
        .await;
        assert_eq!(answer(&events), "Please provide a path.");
        let ids = started(&events);
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_replay(requests.last().unwrap(), 2);
    }
}

#[tokio::test]
async fn fenced_and_array_calls_are_not_replayed_as_prose() {
    for content in [
        format!("```json\n{}\n```", call(None)),
        format!("[{}]", call(None)),
    ] {
        let (events, requests) =
            exercise(vec![text(&content), text("Please provide a path.")]).await;
        assert_eq!(answer(&events), "Please provide a path.");
        assert_eq!(started(&events).len(), 1);
        assert_replay(requests.last().unwrap(), 1);
    }
}

#[tokio::test]
async fn invalid_array_member_prevents_partial_execution() {
    let invalid = format!(
        "[{}, {{\"type\":\"function\",\"function\":{{\"arguments\":{{}}}}}}]",
        call(None)
    );
    let (events, requests) = exercise(vec![text(&invalid), text("Please provide a path.")]).await;
    assert_eq!(answer(&events), "Please provide a path.");
    assert!(started(&events).is_empty());
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn ordinary_prose_and_json_remain_answers() {
    for content in [
        "Hello!",
        r#"{"name":"Jake","age":30}"#,
        r#"{"function":"a mathematical mapping"}"#,
        "Here is an example: {\"name\":\"read_file\",\"arguments\":{}}",
    ] {
        let (events, requests) = exercise(vec![text(content)]).await;
        assert_eq!(answer(&events), content);
        assert!(started(&events).is_empty());
        assert_eq!(requests.len(), 1);
    }
}
