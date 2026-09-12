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
use zone_core::llm::{FunctionCall, LlmClient, LlmConfig, Message, Role, ToolCall};
use zone_core::tools::job::{self, Jobs};
use zone_core::tools::{Session, Tool};
use zone_server::agent::prompt;
use zone_server::agent::wait::{self, KIND_JOB, WAIT_FOR};
use zone_server::agent::{
    AgentEvent, AgentRun, ApprovalGate, ApprovalPolicy, ChatTools, Environment, MAX_ITERATIONS,
    ToolCallRecord, WorkspaceScope, run,
};

const MALFORMED: &str =
    r#"{"id":"call_0","type":"function","function":{"name":"read_file","arguments":{}}"#;

/// What each of the six model-facing strings said before `wait_for` existed,
/// lowercased, reduced to the half no replacement could contain.
///
/// A fragment rather than the whole string because the whole string is gone: an
/// absence asserted against text nothing ever says again is vacuous the moment
/// its owner rewords anything. These are the words that mandated the poll, so a
/// revert restores them whatever else it changes around them.
const SUPERSEDED_POLL_WORDING: [&str; 6] = [
    "do not claim the runner finished; poll",
    "does not wait for completion — poll",
    "use to monitor start_task progress",
    "runner started. poll",
    "to wait longer, return and check again in a later call",
    "return without waiting and check again in a later call",
];

/// Fixed, so a rendered prompt is the same bytes on every run.
fn environment() -> Environment {
    Environment::at(
        chrono::DateTime::parse_from_rfc3339("2026-09-09T09:30:00+12:00").unwrap(),
        "Pacific/Auckland",
        std::path::PathBuf::from("/srv/zone"),
    )
}

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
    exercise_messages(rounds, vec![Message::user("Help me inspect a file.")]).await
}

async fn exercise_messages(
    rounds: Vec<(u16, Vec<Value>)>,
    messages: Vec<Message>,
) -> (Vec<AgentEvent>, Vec<Value>) {
    exercise_approved(rounds, messages, ApprovalPolicy::auto()).await
}

async fn exercise_approved(
    rounds: Vec<(u16, Vec<Value>)>,
    messages: Vec<Message>,
    approval: ApprovalPolicy,
) -> (Vec<AgentEvent>, Vec<Value>) {
    let responses = Arc::new(Mutex::new(VecDeque::from(rounds)));
    exercise_scripted(
        chat_catalog(Uuid::new_v4()).await,
        move |_request: &Request| {
            let (status, deltas) = responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected extra completion");
            stream(status, deltas)
        },
        messages,
        approval,
    )
    .await
}

/// One scripted round, as the provider streams it back.
fn stream(status: u16, deltas: Vec<Value>) -> ResponseTemplate {
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
}

/// Bounds the wait on a database nothing answers on, so a call that reaches it
/// costs a test milliseconds rather than the default acquire timeout.
const ACQUIRE_TIMEOUT: Duration = Duration::from_millis(250);

/// The catalog the server assembles for one chat turn.
///
/// The chat id is a parameter because a job or a wait is keyed on the session
/// the tool set carries, and a caller that has to clean either up afterwards
/// needs to name the same session the tools staged under.
async fn chat_catalog(chat_id: Uuid) -> ChatTools {
    let pool = PgPoolOptions::new()
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let state = common::create_test_state(common::test_config(), pool);
    ChatTools::build(WorkspaceScope {
        user_id: Uuid::new_v4(),
        state,
        workspace_id: Uuid::new_v4(),
        chat_id: Some(chat_id),
    })
    .await
}

/// Run the real loop against a script that may read the request it answers.
///
/// A round whose call names something an earlier round produced — a job id, say
/// — cannot be queued up in advance, so the script is a function of the request
/// rather than a list.
async fn exercise_scripted<S>(
    tools: ChatTools,
    script: S,
    messages: Vec<Message>,
    approval: ApprovalPolicy,
) -> (Vec<AgentEvent>, Vec<Value>)
where
    S: Fn(&Request) -> ResponseTemplate + Send + Sync + 'static,
{
    // LlmClient shares its HTTP pool across tests, but each Tokio test owns a
    // separate runtime. A pooled mock can reuse a connection whose I/O driver
    // belongs to a paused or shutting-down test runtime. Give each provider its
    // own listener instead, while retaining keep-alives within this agent run.
    let provider = MockServer::builder().start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(script)
        .mount(&provider)
        .await;
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
            messages,
            budget: zone_server::agent::LoopBudget::chat(),
            approval,
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

/// The corrective steers the rest of this turn and nothing beyond it.
///
/// Reporting it as `Canonical` would look like the way to let a consumer that
/// rebuilds a turn from the events see it, but a chat commits every `Canonical`
/// to durable turn history: `db::context` refuses a `System` role there, and
/// anything that did land would come back preserved in every later turn, so a
/// conversation would carry a complaint about a reply it no longer contains.
#[tokio::test]
async fn the_malformed_call_corrective_steers_the_turn_without_outliving_it() {
    let (events, requests) = exercise(vec![
        text(MALFORMED),
        text(&call(Some("call_0"))),
        text("Please provide the file path."),
    ])
    .await;

    let replayed = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("malformed tool call"))
        });
    assert!(
        replayed,
        "the round after a malformed reply was never told about it: {:?}",
        requests[1]["messages"]
    );

    assert!(
        events.iter().all(|event| !matches!(
            event,
            AgentEvent::Canonical(entry) if entry.message.role == Role::System
        )),
        "a system nudge reported as canonical is one a chat would persist and \
         then replay into every later turn: {events:?}"
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
async fn ordinary_prose_is_streamed_as_it_arrives() {
    let (events, _) = exercise(vec![vec![
        json!({"content": "Hel"}),
        json!({"content": "lo!"}),
    ]])
    .await;
    let chunks: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Chunk(content) => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chunks, ["Hel", "lo!"]);
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
async fn unsupported_tools_preserve_prefetched_web_context() {
    let evidence = "Zone already searched the web for this turn.\nAuckland forecast\nhttps://weather.example/auckland\nShowers, 16 degrees Celsius.";
    let context = format!("<web_search_context>\n{evidence}\n</web_search_context>");
    let (events, requests) = exercise_messages(
        vec![
            (
                400,
                vec![json!({"error": {"message": "model does not support tools"}})],
            ),
            (200, text("Finished.")),
        ],
        vec![
            Message::system("The final <web_search_context> message supplies server search data for the preceding request."),
            Message::assistant("I cannot access the web."),
            Message::user("Search for today's weather in Auckland."),
            Message::user(&context),
        ],
    )
    .await;
    assert!(started(&events).is_empty());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed(_)))
    );
    assert_eq!(requests.len(), 2);
    assert!(requests[0]["tools"].is_array());
    assert!(requests[1]["tools"].is_null());
    let original = requests[0]["messages"].as_array().unwrap();
    let fallback = requests[1]["messages"].as_array().unwrap();
    for message in original {
        assert!(
            fallback.contains(message),
            "Lost original evidence: {message}"
        );
    }
    assert!(
        fallback
            .iter()
            .any(|message| message["role"] == "user" && message["content"] == context)
    );
    assert!(
        fallback.iter().any(|message| message["role"] == "system"
            && message["content"]
                .as_str()
                .unwrap()
                .contains("Use supplied search evidence where sufficient")),
        "Tool fallback must retain completed retrieval"
    );
}

#[tokio::test]
async fn duplicate_stream_images_are_kept_once() {
    let delta = json!({"images":[{"image_url":{"url":"data:image/png;base64,abc"}}]});
    let (events, _) = exercise(vec![vec![delta.clone(), delta]]).await;
    let images = events
        .iter()
        .filter(|event| matches!(event, AgentEvent::Image(_)))
        .count();
    assert_eq!(images, 1);
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
                .any(|event| matches!(event, AgentEvent::Image(_))),
            "events: {events:?}"
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

#[tokio::test]
async fn reads_after_a_mutation_in_the_same_batch_execute_again() {
    let path = std::env::temp_dir().join(format!("zone-loop-{}.txt", Uuid::new_v4()));
    std::fs::write(&path, "before").unwrap();
    let read = json!({"name":"read_file","arguments":{"path":path}});
    let write = json!({"name":"write_file","arguments":{"path":path,"content":"after"}});
    let (events, requests) = exercise(vec![
        text(&json!([read, write]).to_string()),
        text(&read.to_string()),
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
async fn auto_approve_writes_without_an_approval_event() {
    let path = std::env::temp_dir().join(format!("zone-auto-{}.txt", Uuid::new_v4()));
    let write =
        json!({"id":"auto_write","name":"write_file","arguments":{"path":path,"content":"auto"}});
    let (events, _) = exercise(vec![text(&write.to_string()), text("Wrote.")]).await;
    let written = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(written.unwrap(), "auto");
    assert_eq!(answer(&events), "Wrote.");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolApprovalRequired { .. }))
    );
}

#[tokio::test]
async fn denied_writes_do_not_touch_the_file() {
    let path = std::env::temp_dir().join(format!("zone-deny-{}.txt", Uuid::new_v4()));
    std::fs::write(&path, "before").unwrap();
    let gate = ApprovalGate::new();
    let poll = gate.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if poll.decide("deny_write", false) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let write =
        json!({"id":"deny_write","name":"write_file","arguments":{"path":path,"content":"after"}});
    let (events, _) = exercise_approved(
        vec![(200, text(&write.to_string())), (200, text("Stopped."))],
        vec![Message::user("Overwrite the file.")],
        ApprovalPolicy::required(gate),
    )
    .await;
    let kept = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(kept, "before");
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolApprovalRequired { id, .. } if id == "deny_write"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolCallCompleted { id, success: false, .. } if id == "deny_write"
    )));
    assert_eq!(answer(&events), "Stopped.");
}

#[tokio::test]
async fn changed_read_evidence_allows_a_previous_failure_to_be_retried() {
    let read = json!({"name":"read_file","arguments":{"path":concat!(env!("CARGO_MANIFEST_DIR"),"/Cargo.toml")}}).to_string();
    let (events, requests) = exercise(vec![
        text(&call(None)),
        text(&read),
        text(&call(None)),
        text("A path is still required."),
    ])
    .await;
    assert_eq!(started(&events).len(), 3);
    assert_eq!(answer(&events), "A path is still required.");
    assert!(requests.last().unwrap()["tools"].is_array());
}

#[tokio::test]
async fn reasoning_tokens_are_streamed_and_distinct_thinking_blocks_accumulate() {
    let first = json!({"type":"thinking","thinking":"Need the capital.","signature":"a"});
    let second = json!({"type":"thinking","thinking":"Paris is the capital.","signature":"b"});
    let (events, _) = exercise(vec![vec![
        json!({"reasoning_content":"Need the capital."}),
        json!({"thinking_blocks":[first]}),
        json!({"reasoning":" Paris is the capital.","thinking_blocks":[second]}),
        json!({"content":"Paris."}),
    ]])
    .await;
    let thinking: String = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Reasoning(content) => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(thinking, "Need the capital. Paris is the capital.");
    assert_eq!(answer(&events), "Paris.");
    let assistant = events.iter().find_map(|event| match event {
        AgentEvent::Canonical(entry)
            if entry.message.role == Role::Assistant && entry.message.tool_calls.is_none() =>
        {
            Some(&entry.message)
        }
        _ => None,
    });
    let assistant = assistant.expect("final assistant message");
    assert_eq!(
        assistant.reasoning_content.as_deref(),
        Some("Need the capital. Paris is the capital.")
    );
    assert_eq!(assistant.thinking_blocks.len(), 2);
}

#[tokio::test]
async fn prose_emitted_after_native_tool_deltas_is_still_streamed() {
    let first = vec![
        json!({"tool_calls":[{"index":0,"id":"call_0","type":"function","function":{"name":"read_file","arguments":"{}"}}]}),
        json!({"content": "Looking at the file."}),
    ];
    let (events, _) = exercise(vec![first, text("A path is required.")]).await;
    let visible: String = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Chunk(content) => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        visible.contains("Looking at the file."),
        "preamble was dropped: {visible:?}"
    );
    assert_eq!(answer(&events), "Looking at the file.A path is required.");
}

#[tokio::test]
async fn reasoning_is_emitted_before_and_after_a_tool_round() {
    let mut first = vec![json!({"reasoning_content": "Need the file."})];
    first.extend(native(Some("call_0")));
    let (events, requests) = exercise(vec![
        first,
        vec![
            json!({"reasoning_content": "Ask for a path."}),
            json!({"content": "Please provide a path."}),
        ],
    ])
    .await;
    let sequence: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Reasoning(content) => Some(content.as_str()),
            AgentEvent::ToolCallStarted { .. } => Some("tool"),
            AgentEvent::Chunk(content) => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        sequence,
        [
            "Need the file.",
            "tool",
            "Ask for a path.",
            "Please provide a path."
        ],
        "events={events:?}, requests={requests:?}"
    );
}

#[tokio::test]
async fn thinking_block_without_reasoning_content_is_streamed() {
    let block = json!({"type":"thinking","thinking":"Look at the file.","signature":"a"});
    let (events, _) = exercise(vec![vec![
        json!({"thinking_blocks":[block]}),
        json!({"content":"Done."}),
    ]])
    .await;
    let thinking: String = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Reasoning(content) => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(thinking, "Look at the file.");
}

#[tokio::test]
async fn thinking_blocks_are_replayed_on_the_next_tool_turn() {
    let blocks = json!([{
        "type": "thinking",
        "thinking": "Need the file.",
        "signature": "sig"
    }]);
    let mut first = vec![json!({
        "reasoning_content": "Need the file.",
        "thinking_blocks": blocks
    })];
    first.extend(native(Some("call_0")));
    let (events, requests) = exercise(vec![first, text("Please provide a path.")]).await;
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::Reasoning(content) if content == "Need the file.")
    ));
    assert_eq!(answer(&events), "Please provide a path.");
    assert!(requests.len() >= 2);
    let replayed = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "assistant" && message.get("thinking_blocks").is_some())
        .expect("assistant thinking replay");
    assert_eq!(replayed["reasoning_content"], "Need the file.");
    assert_eq!(replayed["thinking_blocks"], blocks);
}

#[tokio::test]
async fn failed_mutations_do_not_create_a_progress_epoch() {
    let path = std::env::temp_dir().join(format!("zone-missing-{}/file", Uuid::new_v4()));
    let call=json!({"name":"apply_patch","arguments":{"patch":format!("*** Begin Patch\n*** Update File: {}\n@@\n-old\n+new\n*** End Patch",path.display())}}).to_string();
    let (events, requests) =
        exercise(vec![text(&call), text("The file could not be updated.")]).await;
    assert_eq!(started(&events).len(), 1);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallCompleted { success: false, .. }))
    );
    assert!(requests.last().unwrap()["tools"].is_null());
}

#[test]
fn provider_connections_do_not_depend_on_another_test_runtime() {
    let first = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (events, _) = first.block_on(exercise(vec![text("First response.")]));
    assert_eq!(answer(&events), "First response.");

    // Keep the first runtime alive but idle, as an independently scheduled test
    // can be. Its pooled HTTP connection must not drive this test's provider.
    let second = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (events, _) = second.block_on(exercise(vec![text("Second response.")]));
    assert_eq!(answer(&events), "Second response.", "{events:?}");
}

/// The agent loop forwards whatever system message it is handed, so this is the
/// only place the assembled prompt is checked as the provider receives it:
/// one system message, first, with the sections in the order `ORDER` fixes.
/// Relative offsets rather than a snapshot, so rewording a rule cannot fail it.
#[tokio::test]
async fn the_assembled_system_prompt_reaches_the_provider_in_section_order() {
    let tools = chat_catalog(Uuid::new_v4()).await;
    let environment = environment();

    let (_, requests) = exercise_messages(
        vec![(200, text("Ready."))],
        vec![
            Message::system(prompt::chat(&tools, false, &environment)),
            Message::user("Help me inspect a file."),
        ],
    )
    .await;

    let first = &requests[0]["messages"][0];
    assert_eq!(first["role"], "system");
    let prompt = first["content"].as_str().expect("a system prompt");

    let offset = |section: &str| {
        prompt
            .find(section)
            .unwrap_or_else(|| panic!("{section} is missing from {prompt}"))
    };
    let identity =
        offset("You are Zone's assistant, answering inside one of the user's workspaces.");
    let boundary = offset("Instructions and data:");
    let conduct = offset("Reporting outcomes: report what happened, not what you meant to happen.");
    let reply = offset("Writing the reply:");
    let refusal = offset("Declining and directness:");
    let files = offset("act in the server runtime");
    let session = offset("Session context:");

    assert_eq!(identity, 0, "{prompt}");
    assert!(identity < boundary, "{prompt}");
    assert!(boundary < conduct, "{prompt}");
    assert!(conduct < reply, "{prompt}");
    assert!(reply < refusal, "{prompt}");
    assert!(refusal < files, "{prompt}");
    assert!(files < session, "{prompt}");
    assert_eq!(
        prompt.rfind("Session context:"),
        Some(session),
        "the live block renders once, last: {prompt}"
    );

    assert_eq!(
        requests[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "system")
            .count(),
        1,
        "{prompt}"
    );
}

/// The whole instruction surface of one turn, as the provider receives it: the
/// assembled prompt, every tool definition offered beside it, and the refusal
/// the model reads when it tries to hold the turn open on a sleep.
///
/// The point of PR 6 is not that a section teaches `wait_for`; it is that
/// nothing still tells the model to poll instead. The nearest instruction is
/// the one that wins, and only one of the six strings that used to ask for a
/// poll is in a prompt at all: the rest arrive as a tool description, a schema
/// or a tool result, places no prompt test can see. So the absences are
/// asserted over the serialized body, where all four kinds sit together.
///
/// The one the body cannot carry is the reply `start_task` itself returns,
/// which needs a workspace this turn has no database for. The crate's own tests
/// pin that string beside the other three the prompt layer owns.
#[tokio::test]
async fn the_turn_offers_the_wait_and_carries_no_surviving_poll_instruction() {
    /// Over `MAX_SLEEP_SECS`, so the sleep cap refuses it and the model reads
    /// what it is told to do instead.
    const OVER_CAP: &str = "sleep 120";

    let tools = chat_catalog(Uuid::new_v4()).await;
    let blocked = json!({
        "id": "sleep_1",
        "name": "run_shell",
        "arguments": {
            "command": OVER_CAP,
            "reason": "Hold the turn open until the build lands.",
        },
    });
    let (_, requests) = exercise_messages(
        vec![
            (200, text(&blocked.to_string())),
            (200, text("Backgrounded it instead.")),
        ],
        vec![
            Message::system(prompt::chat(&tools, false, &environment())),
            Message::user("Wait for the build to finish."),
        ],
    )
    .await;

    let prompt = requests[0]["messages"][0]["content"]
        .as_str()
        .expect("a system prompt");
    assert!(prompt.contains(WAIT_FOR), "{prompt}");
    assert!(
        prompt.contains("Waiting for something to finish:"),
        "{prompt}"
    );
    assert!(
        prompt.contains("Never call tail_task_log, get_task_run or get_build_status in a loop"),
        "{prompt}"
    );
    assert!(
        prompt.contains("A wait that ends without its event is a timeout, not a result."),
        "{prompt}"
    );
    assert!(
        prompt.contains("A wait is not a way to re-read something that has not changed."),
        "{prompt}"
    );

    let offered = |name: &str| {
        requests[0]["tools"]
            .as_array()
            .expect("a tool catalog")
            .iter()
            .find(|tool| tool["function"]["name"] == name)
            .unwrap_or_else(|| panic!("{name} is missing from the catalog"))
            .to_string()
    };
    assert!(offered(WAIT_FOR).contains("Wait for something outside this loop to finish"));
    assert!(
        offered("run_shell").contains("wait for it with wait_for"),
        "{}",
        offered("run_shell")
    );
    assert!(
        offered("start_task").contains("wait for it with wait_for"),
        "{}",
        offered("start_task")
    );
    assert!(
        offered("tail_task_log").contains("wait for it with wait_for rather than calling this"),
        "{}",
        offered("tail_task_log")
    );

    let refusal = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["tool_call_id"] == "sleep_1")
        .and_then(|message| message["content"].as_str())
        .expect("the refused sleep is replayed to the model");
    assert!(refusal.contains(WAIT_FOR), "{refusal}");
    assert!(refusal.contains("background: true"), "{refusal}");

    for request in &requests {
        let body = request.to_string().to_lowercase();
        for superseded in SUPERSEDED_POLL_WORDING {
            assert!(
                !body.contains(superseded),
                "{superseded:?} survives in {request}"
            );
        }
    }
}

/// Approves `id` as soon as the loop registers it, so the run does not sit out
/// its timeout waiting for a decision that only a person would otherwise make.
fn approve(gate: &ApprovalGate, id: &'static str) {
    let poll = gate.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if poll.decide(id, true) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
}

/// The acceptance test for the action tiers: an approval has to describe the
/// call it gates, from the call, and describe it before the call runs.
///
/// The stated reason and the arguments deliberately disagree. A reader
/// deciding on the reason alone would allow an overwrite believing it was an
/// append to something else, so the preview is read out of the arguments and is
/// what settles the two. The file is read at the moment the decision is
/// answered: what it holds then is what the preview was describing rather than
/// reporting.
#[tokio::test]
async fn an_approval_previews_the_call_from_its_arguments_before_it_runs() {
    const CLAIM: &str = "Append a note to the changelog the user dictated.";
    let path = std::env::temp_dir().join(format!("zone-preview-{}.txt", Uuid::new_v4()));
    std::fs::write(&path, "before").unwrap();

    let gate = ApprovalGate::new();
    let held = Arc::new(Mutex::new(None::<String>));
    let observed = Arc::clone(&held);
    let watched = path.clone();
    let poll = gate.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let before = std::fs::read_to_string(&watched).ok();
            if poll.decide("previewed_write", true) {
                *observed.lock().unwrap() = before;
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    let write = json!({
        "id": "previewed_write",
        "name": "write_file",
        "arguments": {"path": path, "content": "after", "reason": CLAIM},
    });
    let (events, _) = exercise_approved(
        vec![(200, text(&write.to_string())), (200, text("Done."))],
        vec![Message::user("Record that.")],
        ApprovalPolicy::required(gate),
    )
    .await;
    let written = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);

    let (reason, preview) = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolApprovalRequired {
                id,
                reason,
                preview,
                ..
            } if id == "previewed_write" => Some((reason.clone(), preview.clone())),
            _ => None,
        })
        .expect("the write asked for approval");
    let preview = preview.expect("a confirmed call is previewed for the reader deciding on it");

    assert!(
        preview.contains(&path.display().to_string()),
        "the preview does not name the file the call replaces: {preview}"
    );
    assert!(
        preview.contains("replacing whatever is there"),
        "the preview does not say the write replaces what the file holds: {preview}"
    );
    assert_eq!(reason.as_deref(), Some(CLAIM));
    assert!(
        !preview.contains("changelog") && !preview.contains("Append"),
        "the preview repeats the model's claim instead of reading the call: {preview}"
    );

    assert_eq!(
        held.lock().unwrap().as_deref(),
        Some("before"),
        "the write had already happened by the time its approval was answered"
    );
    assert_eq!(
        written.unwrap(),
        "after",
        "the previewed write is not the one that ran"
    );
    assert_eq!(answer(&events), "Done.");
}

/// The decision this PR rests on, first half: a stated reason has to survive
/// the whole way to the two places a person reads it — the approval frame they
/// decide on, and the record stored on the message that the console re-renders
/// after a reload. Neither is reachable from the schema alone.
#[tokio::test]
async fn a_stated_reason_reaches_the_approval_frame_and_the_stored_record() {
    const WHY: &str = "The user asked me to save the draft they dictated.";
    let path = std::env::temp_dir().join(format!("zone-reason-{}.txt", Uuid::new_v4()));
    let gate = ApprovalGate::new();
    approve(&gate, "stated_write");
    let write = json!({
        "id": "stated_write",
        "name": "write_file",
        "arguments": {"path": path, "content": "draft", "reason": WHY},
    });
    let (events, _) = exercise_approved(
        vec![(200, text(&write.to_string())), (200, text("Saved."))],
        vec![Message::user("Save the draft.")],
        ApprovalPolicy::required(gate),
    )
    .await;
    let written = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);

    let frame = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolApprovalRequired { id, reason, .. } if id == "stated_write" => {
                Some(reason.clone())
            }
            _ => None,
        })
        .expect("the write asked for approval");
    assert_eq!(
        frame,
        Some(WHY.to_string()),
        "the approval frame decides on bare arguments without the stated intent"
    );

    // The stored record is built off ToolCallStarted's arguments, the way
    // ws::chat does when it writes messages.metadata.
    let arguments = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolCallStarted { id, arguments, .. } if id == "stated_write" => {
                Some(arguments.clone())
            }
            _ => None,
        })
        .expect("the write started");
    let record = ToolCallRecord {
        id: "stated_write".to_string(),
        name: "write_file".to_string(),
        arguments: arguments.clone(),
        success: true,
        detail: "Wrote".to_string(),
        duration_ms: 0,
        reasoning: None,
        reason: zone_server::agent::reason(&arguments),
        preview: Some("Write 5 characters to the draft, replacing whatever is there.".to_string()),
        questions: Vec::new(),
        job: None,
        waiting: None,
    };
    assert_eq!(record.reason.as_deref(), Some(WHY));
    let stored = serde_json::to_value(&record).unwrap();
    assert_eq!(stored["reason"], json!(WHY), "{stored}");
    assert_eq!(
        serde_json::from_value::<ToolCallRecord>(stored).unwrap(),
        record,
        "the record does not survive the round trip through messages.metadata"
    );

    assert_eq!(written.unwrap(), "draft");
    assert_eq!(answer(&events), "Saved.");
}

/// The decision this PR rests on, second half, and the one worth a test.
///
/// `reason` sits in seven schemas' `required` arrays, but nothing validates
/// that array at dispatch and this is deliberate: a missing annotation must
/// never fail a call. A model too small to keep the parameter straight would
/// otherwise spend a whole turn on a rejection it cannot read its way out of,
/// and the anti-loop guard would refuse the identical retry.
///
/// If a later author "tightens" this into real validation, this test fails —
/// which is the point of it.
#[tokio::test]
async fn a_side_effecting_call_omitting_its_reason_still_executes() {
    assert!(
        zone_core::tools::WriteFileTool.parameters_schema()["required"]
            .as_array()
            .expect("required array")
            .iter()
            .any(|name| name == "reason"),
        "this test is only meaningful while the schema advertises reason as required"
    );

    let path = std::env::temp_dir().join(format!("zone-noreason-{}.txt", Uuid::new_v4()));
    let gate = ApprovalGate::new();
    approve(&gate, "silent_write");
    let write = json!({
        "id": "silent_write",
        "name": "write_file",
        "arguments": {"path": path, "content": "written anyway"},
    });
    let (events, requests) = exercise_approved(
        vec![(200, text(&write.to_string())), (200, text("Saved."))],
        vec![Message::user("Save it.")],
        ApprovalPolicy::required(gate),
    )
    .await;
    let written = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        written.unwrap(),
        "written anyway",
        "a missing reason stopped the write from happening"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentEvent::ToolCallCompleted { id, success: true, .. } if id == "silent_write"
        )),
        "the call reported failure: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed(_))),
        "the turn failed over a missing annotation: {events:?}"
    );

    // The frame still goes up, saying plainly that no reason was given rather
    // than holding the call back until one is.
    assert_eq!(
        events.iter().find_map(|event| match event {
            AgentEvent::ToolApprovalRequired { id, reason, .. } if id == "silent_write" =>
                Some(reason.clone()),
            _ => None,
        }),
        Some(None),
        "an unexplained write must still reach the approver"
    );

    // One dispatch, not a rejected first attempt and a retry: a turn spent on
    // the missing parameter is the cost this non-enforcement exists to avoid.
    assert_eq!(started(&events), vec!["silent_write"]);
    assert_eq!(requests.len(), 2);
    assert_eq!(answer(&events), "Saved.");
}

fn ask(id: &str) -> Value {
    json!({
        "id": id,
        "name": "ask_user",
        "arguments": {
            "questions": [{
                "header": "Scope",
                "question": "How far back should the rewrite run?",
                "options": [
                    {"label":"Backfill","description":"Rewrite the existing rows."},
                    {"label":"Forward only","description":"Leave the existing rows alone."}
                ]
            }]
        }
    })
}

fn search(id: &str) -> Value {
    json!({"id":id,"name":"search_knowledge","arguments":{"query":"deploys"}})
}

fn read(id: &str) -> Value {
    json!({"id":id,"name":"read_file","arguments":{"unused":id}})
}

fn questioned(events: &[AgentEvent]) -> Vec<(&str, usize)> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::QuestionRequired {
                tool_call_id,
                questions,
                ..
            } => Some((tool_call_id.as_str(), questions.len())),
            _ => None,
        })
        .collect()
}

fn tool_entries(events: &[AgentEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Canonical(entry) if entry.message.role == Role::Tool => Some((
                entry.message.tool_call_id.clone().unwrap_or_default(),
                entry.message.content.clone().unwrap_or_default(),
            )),
            _ => None,
        })
        .collect()
}

/// Every call the model requested, tagged by whether it started or finished,
/// so a batch is visible as starts that precede the completions.
fn dispatch(events: &[AgentEvent]) -> Vec<(&str, &str)> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolCallStarted { id, .. } => Some(("started", id.as_str())),
            AgentEvent::ToolCallCompleted { id, .. } => Some(("completed", id.as_str())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_question_ends_the_turn_and_strands_the_calls_queued_behind_it() {
    let (events, requests) = exercise(vec![text(
        &json!([ask("ask_1"), search("search_1"), search("search_2")]).to_string(),
    )])
    .await;

    assert_eq!(questioned(&events), vec![("ask_1", 1)]);
    assert_eq!(
        started(&events),
        vec!["ask_1"],
        "nothing queued behind the question runs"
    );
    let entries = tool_entries(&events);
    assert_eq!(entries.len(), 3, "{entries:?}");
    assert_eq!(entries[0].0, "ask_1");
    assert!(
        entries[0]
            .1
            .contains("the answer arrives as the next user message"),
        "{entries:?}"
    );
    for (id, content) in &entries[1..] {
        assert!(id == "search_1" || id == "search_2", "{entries:?}");
        assert_eq!(
            content,
            "Not executed: the turn ended when the user was asked a question."
        );
    }
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Finalizing(_))),
        "a parked turn asks for no final answer"
    );
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn the_turn_after_an_answer_replays_the_question_it_answers() {
    let arguments = ask("ask_1")["arguments"].to_string();
    let messages = vec![
        Message::user("Change the tenant column."),
        Message::assistant_with_tools(vec![ToolCall {
            id: "ask_1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "ask_user".to_string(),
                arguments,
            },
        }]),
        Message::tool_result(
            "ask_1",
            "The question card was shown. Your turn ends here; the answer arrives as the next user message.",
        ),
        Message::user("Scope: Backfill"),
    ];
    let (events, requests) =
        exercise_messages(vec![(200, text("Backfilling now."))], messages).await;

    assert_eq!(answer(&events), "Backfilling now.");
    assert!(questioned(&events).is_empty());
    assert_eq!(requests.len(), 1);
    assert_replay(requests.last().unwrap(), 1);
}

#[tokio::test]
async fn reads_queued_ahead_of_a_question_batch_without_it() {
    let (events, requests) = exercise(vec![text(
        &json!([read("read_1"), read("read_2"), ask("ask_1")]).to_string(),
    )])
    .await;

    assert_eq!(
        dispatch(&events),
        vec![
            ("started", "read_1"),
            ("started", "read_2"),
            ("completed", "read_1"),
            ("completed", "read_2"),
            ("started", "ask_1"),
            ("completed", "ask_1"),
        ]
    );
    assert_eq!(questioned(&events), vec![("ask_1", 1)]);
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn a_question_the_schema_rejects_fails_and_the_turn_carries_on() {
    let malformed = json!({"id":"ask_1","name":"ask_user","arguments":{"questions":[]}});
    let (events, requests) = exercise(vec![
        text(&malformed.to_string()),
        text(&read("read_1").to_string()),
        text("Never mind."),
    ])
    .await;

    assert!(questioned(&events).is_empty());
    assert_eq!(started(&events), vec!["ask_1", "read_1"]);
    let failed: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolCallCompleted {
                id, success: false, ..
            } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(failed, vec!["ask_1", "read_1"]);
    assert_eq!(answer(&events), "Never mind.");
    assert_eq!(requests.len(), 3);
}

/// Re-reading a run that has not moved is finalised, not read again.
///
/// This is the detector the waiting section's last rule exists to protect, and
/// the two finalisers a repeated read can trip say different things. When every
/// call in a round has already failed the loop reports repeated failures and
/// executes nothing; the no-progress detector fires when a round did execute
/// and learned nothing. So the round carries a file read that comes back
/// unchanged beside the repeated status read, which is the shape the rule is
/// about: evidence that came back twice, identical.
///
/// The file read leads because a read that is novel clears the failure ledger,
/// so a run put second keeps the failure the second round needs to be stale.
#[tokio::test]
async fn a_run_reread_without_change_is_finalised_rather_than_read_again() {
    const NO_PROGRESS: &str = "Repeated tool reads returned unchanged evidence without progress.";
    const SETTLED: &str = "The run has not moved since the first read.";

    let directory = tempfile::TempDir::new().unwrap();
    let evidence = directory.path().join("run.log");
    std::fs::write(&evidence, "phase: running").unwrap();
    let run = Uuid::new_v4();
    let round = |suffix: &str| {
        text(
            &json!([
                {"id": format!("log_{suffix}"), "name": "read_file", "arguments": {"path": evidence}},
                {"id": format!("run_{suffix}"), "name": "get_task_run", "arguments": {"run_id": run}},
            ])
            .to_string(),
        )
    };

    let (events, requests) = exercise(vec![round("1"), round("2"), text(SETTLED)]).await;

    let finalised: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Finalizing(reason) => Some(reason.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(finalised, vec![NO_PROGRESS], "{events:?}");

    let instruction = requests[2]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "system")
        .filter_map(|message| message["content"].as_str())
        .find(|content| content.contains(NO_PROGRESS))
        .unwrap_or_else(|| {
            panic!(
                "the finalizing instruction never reached the model: {}",
                requests[2]
            )
        });
    assert!(
        instruction
            .contains("Answer the user in ordinary text using the evidence already available."),
        "{instruction}"
    );
    assert!(
        requests[2].get("tools").is_none(),
        "a finalizing round still offered tools: {}",
        requests[2]
    );
    assert_eq!(answer(&events), SETTLED);
    assert_eq!(requests.len(), 3);
}

/// A wait that ended without its event is never reported as a result.
///
/// `resume_with_outcome` injects the outcome as an envelope and a tool result,
/// so a resumed turn reads it as evidence like any other. The two outcomes that
/// say nothing happened are the ones a model would most readily round up to
/// "finished", and the honesty they are written for is only worth something if
/// it survives the trip: the outcome has to reach the provider as written, and
/// nothing the turn produces from it may read as success. "settled to success"
/// is a different outcome that legitimately contains the word, and is not one
/// of these two.
#[tokio::test]
async fn a_wait_that_ended_without_its_event_is_never_reported_as_a_result() {
    const SUCCESS_WORDS: [&str; 5] = ["success", "succeeded", "completed", "passed", "done"];
    const JOB: &str = "job_9f3c1a7b2e04";

    for (outcome, honesty) in [
        (
            wait::timed_out(
                &wait::job_subject(JOB),
                Duration::from_secs(wait::DEFAULT_WAIT_SECS),
            ),
            "this is a timeout, not a result",
        ),
        (
            wait::checks_unknown("main", wait::CHECK_SETTLE_GRACE),
            "This is not a pass.",
        ),
    ] {
        assert!(outcome.contains(honesty), "{outcome}");

        let (events, requests) = exercise_messages(
            vec![(200, text(&report(&outcome)))],
            settled_wait(JOB, &outcome),
        )
        .await;

        let replayed: Vec<&str> = requests[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["tool_call_id"] == SETTLED_CALL)
            .filter_map(|message| message["content"].as_str())
            .collect();
        assert_eq!(
            replayed,
            vec![outcome.as_str()],
            "the outcome reached the model as something other than what it says"
        );

        let reported = answer(&events).to_lowercase();
        for word in SUCCESS_WORDS {
            assert!(
                !outcome.to_lowercase().contains(word),
                "{word:?}: {outcome}"
            );
            assert!(!reported.contains(word), "{word:?}: {reported}");
        }
    }
}

/// The call id `resume_with_outcome` injects the settled pair under.
const SETTLED_CALL: &str = "wait_1#settled";

/// Fixed, so the receipt a transcript replays is the same bytes on every run.
const DEADLINE: &str = "2026-09-09T09:35:00Z";

/// The transcript a wait's outcome resumes into: the call that opened the wait
/// and the receipt it returned, then the pair `resume_with_outcome` appends.
fn settled_wait(job: &str, outcome: &str) -> Vec<Message> {
    let waiting = json!({"kind": KIND_JOB, "id": job, "deadline": DEADLINE}).to_string();
    vec![
        Message::user("Start the build and tell me how it went."),
        Message::assistant_with_tools(vec![wait_call("wait_1", &waiting)]),
        Message::tool_result("wait_1", wait::receipt(&wait::job_subject(job), DEADLINE)),
        Message::assistant_with_tools(vec![wait_call(SETTLED_CALL, &waiting)]),
        Message::tool_result(SETTLED_CALL, outcome),
    ]
}

fn wait_call(id: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        call_type: "function".to_string(),
        function: FunctionCall {
            name: WAIT_FOR.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

/// What the waiting section asks for: report which of the two you have. A
/// script that answered in its own words would prove only that the script was
/// honest, so the reply carries the outcome it was handed.
fn report(outcome: &str) -> String {
    format!("The wait came back: {outcome}")
}

/// The refund, through the loop that grants it.
///
/// Nothing before this could reach it. A wait is registered by the tool and
/// bound to its call by the loop, so a park needs both halves live; and a tool
/// set assembled without a chat scope stages under `Session::Detached`, where a
/// background job is refused outright. The turn therefore starts the job it
/// waits on through the same tool set, and the script reads the job id back out
/// of the spawn receipt the way the chat layer does.
///
/// Both parks happen in the same round, which is what makes the two numbers
/// comparable: a wait hands that round back because the same work continues the
/// moment its subject settles, and a question spends it because the answer
/// arrives as a turn of its own.
#[tokio::test]
async fn a_wait_hands_its_round_back_while_a_question_spends_it() {
    /// The round both parks open in. Round 0 is the call each one needs behind
    /// it: the job to wait on, and a read to fail.
    const PARKED_AT: usize = 1;
    const WAIT_CALL: &str = "wait_1";
    const QUEUED: &str = "queued_1";
    const JOB_CALL: &str = "background_1";

    let directory = tempfile::TempDir::new().unwrap();
    let cwd = directory.path().to_string_lossy().into_owned();
    let chat = Uuid::new_v4();
    let session = Session::Chat(chat);
    let opening = text(
        &json!([{
            "id": JOB_CALL,
            "name": "run_shell",
            "arguments": {
                "command": "sleep 30",
                "cwd": cwd,
                "background": true,
                "reason": "The build outlives this round, so wait for it rather than block on it.",
            },
        }])
        .to_string(),
    );

    let (events, requests) = exercise_scripted(
        chat_catalog(chat).await,
        move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let started = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|message| job::parse_receipt(message["content"].as_str()?))
                .next_back();
            match started {
                None => stream(200, opening.clone()),
                Some(job) => stream(
                    200,
                    text(
                        &json!([
                            {"id": WAIT_CALL, "name": WAIT_FOR, "arguments": {"kind": KIND_JOB, "id": job.id}},
                            read(QUEUED),
                        ])
                        .to_string(),
                    ),
                ),
            }
        },
        vec![Message::user("Start the build and tell me when it lands.")],
        ApprovalPolicy::auto(),
    )
    .await;

    // Before any assertion, so a failing one cannot leave the child behind.
    let killed = Jobs::kill_session(session).await;
    wait::reset_session(session);

    let (waiting, spent) = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::WaitRequired {
                tool_call_id,
                waiting,
                spent,
            } if tool_call_id == WAIT_CALL => Some((waiting.clone(), *spent)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the scripted wait never ended the turn: {events:?}"));
    assert_eq!(
        spent.iterations, PARKED_AT,
        "a wait charged the round it handed back"
    );
    assert_eq!(spent.tool_calls, 2);

    let entries = tool_entries(&events);
    let receipt = job::parse_receipt(&entries[0].1)
        .unwrap_or_else(|| panic!("the backgrounded call returned no spawn receipt: {entries:?}"));
    assert_eq!(entries[0].0, JOB_CALL);
    assert_eq!(
        killed, 1,
        "the job the turn started was not this chat session's to end"
    );
    assert!(
        receipt.log_path.starts_with(&cwd),
        "the job log landed outside the call's own directory: {}",
        receipt.log_path
    );
    assert_eq!(waiting.kind, KIND_JOB);
    assert_eq!(waiting.id, receipt.id);

    // The receipt, not an outcome: at the moment it is written the job has not
    // exited, and the outcome is injected on resume instead.
    assert_eq!(entries[1].0, WAIT_CALL);
    assert_eq!(
        entries[1].1,
        wait::receipt(&wait::job_subject(&receipt.id), &waiting.deadline)
    );
    assert!(!entries[1].1.contains("exited with code"), "{entries:?}");
    assert!(!entries[1].1.contains("Timed out after"), "{entries:?}");

    assert_eq!(entries[2].0, QUEUED);
    assert_eq!(
        entries[2].1,
        "Not executed: the turn ended when a wait was opened."
    );
    assert_eq!(
        started(&events),
        vec![JOB_CALL, WAIT_CALL],
        "nothing queued behind the wait ran"
    );
    assert_eq!(requests.len(), 2);

    let (events, requests) = exercise(vec![
        text(&read("read_0").to_string()),
        text(&json!([ask("ask_1"), read(QUEUED)]).to_string()),
    ])
    .await;

    let spent = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::QuestionRequired {
                tool_call_id,
                spent,
                ..
            } if tool_call_id == "ask_1" => Some(*spent),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the scripted question never ended the turn: {events:?}"));
    assert_eq!(
        spent.iterations,
        PARKED_AT + 1,
        "a question handed back the round it spends"
    );

    let entries = tool_entries(&events);
    assert_eq!(entries.last().unwrap().0, QUEUED);
    assert_eq!(
        entries.last().unwrap().1,
        "Not executed: the turn ended when the user was asked a question.",
        "a turn stopped by a wait and one stopped by a question tell the calls behind them apart"
    );
    assert_eq!(requests.len(), 2);
}
