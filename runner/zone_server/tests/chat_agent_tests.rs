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
    exercise_responses(rounds.into_iter().map(|round| (200, round)).collect()).await
}

async fn exercise_responses(rounds: Vec<(u16, Vec<Value>)>) -> (Vec<AgentEvent>, Vec<Value>) {
    let provider = MockServer::start().await;
    let responses = Arc::new(Mutex::new(VecDeque::from(rounds)));
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |_request: &Request| {
            let (status, deltas) = responses.lock().unwrap().pop_front().expect("unexpected extra completion");
            if status != 200 {
                return ResponseTemplate::new(status).set_body_json(&deltas[0]);
            }
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
    let (events, requests) = exercise(vec![text(MALFORMED); MAX_ITERATIONS + 1]).await;
    assert_eq!(answer(&events), "");
    assert!(started(&events).is_empty());
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed(_)))
    );
    assert!(requests.len() > 1 && requests.len() <= MAX_ITERATIONS + 1);
}

#[tokio::test]
async fn repeated_native_and_text_ids_have_unique_replay_pairs() {
    for id in [Some("call_0"), None] {
        let (events, requests) = exercise(vec![
            native(id),
            text(&call(id).replace("{}", "{\"unused\":true}")),
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

#[tokio::test]
async fn repeated_calls_stop_and_synthesize_without_tools() {
    let (events, requests) = exercise(vec![
        text(&call(None)),
        text(&call(None)),
        text("Please provide a path."),
    ])
    .await;
    assert_eq!(answer(&events), "Please provide a path.");
    assert_eq!(started(&events).len(), 1);
    assert!(requests.last().unwrap()["tools"].is_null());
    assert_replay(requests.last().unwrap(), 2);
}

#[tokio::test]
async fn final_synthesis_rejects_tool_calls_and_empty_answers() {
    for final_round in [text(&call(None)), native(None), text(""), text(MALFORMED)] {
        let (events, requests) =
            exercise(vec![text(&call(None)), text(&call(None)), final_round]).await;
        assert_eq!(started(&events).len(), 1);
        assert_eq!(answer(&events), "");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentEvent::Failed(_)))
        );
        assert!(requests.last().unwrap()["tools"].is_null());
    }
}

#[tokio::test]
async fn repeated_arguments_ignore_json_object_key_order() {
    let (events, requests) = exercise(vec![
        text(r#"{"name":"read_file","arguments":{"unused":{"b":2,"a":1},"other":0}}"#),
        text(r#"{"name":"read_file","arguments":{"other":0,"unused":{"a":1,"b":2}}}"#),
        text("Please provide a path."),
    ])
    .await;
    assert_eq!(started(&events).len(), 1);
    assert_eq!(answer(&events), "Please provide a path.");
    assert!(requests.last().unwrap()["tools"].is_null());
}

#[tokio::test]
async fn exhausted_rounds_get_a_final_answer() {
    let mut rounds: Vec<_> = (0..MAX_ITERATIONS)
        .map(|index| text(&json!({"name":"read_file","arguments":{"unused":index}}).to_string()))
        .collect();
    rounds.push(text("Please provide a path."));
    let (events, requests) = exercise(rounds).await;
    assert_eq!(started(&events).len(), MAX_ITERATIONS);
    assert_eq!(answer(&events), "Please provide a path.");
    assert!(requests.last().unwrap()["tools"].is_null());
    assert_replay(requests.last().unwrap(), MAX_ITERATIONS);
}

#[tokio::test]
async fn exhausted_batch_pairs_every_call_before_final_answer() {
    let calls: Vec<_> = (0..zone_server::agent::MAX_TOOL_CALLS + 2)
        .map(|index| json!({"name":"read_file","arguments":{"unused":index}}))
        .collect();
    let (events, requests) = exercise(vec![
        text(&json!(calls).to_string()),
        text("Please provide a path."),
    ])
    .await;
    assert_eq!(started(&events).len(), zone_server::agent::MAX_TOOL_CALLS);
    assert_eq!(answer(&events), "Please provide a path.");
    assert!(requests.last().unwrap()["tools"].is_null());
    assert_replay(requests.last().unwrap(), 1);
}

#[tokio::test]
async fn reads_after_a_mutation_execute_again() {
    let path = std::env::temp_dir().join(format!("zone-loop-{}.txt", Uuid::new_v4()));
    std::fs::write(&path, "before").unwrap();
    let read = json!({"name":"read_file","arguments":{"path":path}}).to_string();
    let write =
        json!({"name":"write_file","arguments":{"path":path,"content":"after"}}).to_string();
    let (events, requests) = exercise(vec![
        text(&read),
        text(&write),
        text(&read),
        text("Updated."),
    ])
    .await;
    std::fs::remove_file(path).unwrap();
    assert_eq!(started(&events).len(), 3);
    assert_eq!(answer(&events), "Updated.");
    assert!(requests.last().unwrap()["tools"].is_array());
    let results: Vec<_> = requests.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect();
    assert!(results[0]["content"].as_str().unwrap().contains("before"));
    assert!(results[2]["content"].as_str().unwrap().contains("after"));
}

#[tokio::test]
async fn unsupported_tools_retry_only_as_a_final_answer() {
    for response in [text("Hello!"), text(&call(None))] {
        let (events, requests) = exercise_responses(vec![
            (
                400,
                vec![json!({"error":{"message":"model does not support tools"}})],
            ),
            (200, response.clone()),
        ])
        .await;
        assert_eq!(requests.len(), 2);
        assert!(requests[0]["tools"].is_array());
        assert!(requests[1]["tools"].is_null());
        assert!(started(&events).is_empty());
        if response == text("Hello!") {
            assert_eq!(answer(&events), "Hello!");
        } else {
            assert_eq!(answer(&events), "");
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, AgentEvent::Failed(_)))
            );
        }
    }
}

#[tokio::test]
async fn image_only_answers_remain_valid() {
    let image = vec![
        json!({"images":[{"image_url":{"url":"data:image/png;base64,abc"},"type":"image_url","index":0}]}),
    ];
    for rounds in [
        vec![image.clone()],
        vec![text(&call(None)), text(&call(None)), image],
    ] {
        let (events, _) = exercise(rounds).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentEvent::Image(_)))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::Failed(_)))
        );
    }
}

#[tokio::test]
async fn empty_first_response_is_an_explicit_failure() {
    let (events, _) = exercise(vec![text("  ")]).await;
    assert_eq!(answer(&events), "");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed(_)))
    );
}
