use std::path::Path;
use std::time::Duration;

use abnegate_agent::AgentState;
use abnegate_agent::FileSessionStore;
use abnegate_agent::SessionStore;
use abnegate_agent::ToolCallResult;
use abnegate_agent::tool::WaitFor;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use zone_core::vpn::Hold;
use zone_core::vpn::OFFLINE;

use super::*;

const MODEL: &str = "test";

const REASON: &str = "pin";

const NOTES: &str = "notes.txt";

const NOTED: &str = "from the project";

const ZONES_TOOLS: [&str; 7] = [
    "apply_patch",
    "list_files",
    "read_file",
    "run_command",
    "search_code",
    "tail_job",
    "write_file",
];

const OLD_SESSION_ID: &str = "6f1c2a4e-8b3d-4c5e-9f70-1a2b3c4d5e6f";

/// A session file as the CLI wrote it before it ran on abnegate-agent: the
/// abbreviated `project_dir` and `duration_ms` keys, a `zone_` call id, and
/// no `summary` or `consumed`.
const OLD_SESSION: &str = r#"{
  "id": "6f1c2a4e-8b3d-4c5e-9f70-1a2b3c4d5e6f",
  "title": "Read the notes",
  "state": {
    "id": "6f1c2a4e-8b3d-4c5e-9f70-1a2b3c4d5e6f",
    "phase": "complete",
    "messages": [
      {"role": "system", "content": "You are a helpful AI assistant.", "name": null, "tool_calls": null, "tool_call_id": null, "images": [], "generated_images": []},
      {"role": "user", "content": "Read the notes", "name": null, "tool_calls": null, "tool_call_id": null, "images": [], "generated_images": []},
      {"role": "assistant", "content": null, "name": null, "tool_calls": [{"id": "zone_0123456789abcdef", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"}}], "tool_call_id": null, "images": [], "generated_images": []},
      {"role": "tool", "content": "from the project", "name": null, "tool_calls": null, "tool_call_id": "zone_0123456789abcdef", "images": [], "generated_images": []},
      {"role": "assistant", "content": "They say: from the project.", "name": null, "tool_calls": null, "tool_call_id": null, "images": [], "generated_images": []}
    ],
    "steps": [
      {
        "id": "0b1c2d3e-4f50-4617-8293-a4b5c6d7e8f9",
        "phase": "thinking",
        "message": null,
        "tool_calls": [{
          "call": {"id": "zone_0123456789abcdef", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"}},
          "result": "from the project",
          "success": true,
          "duration_ms": 12
        }],
        "started_at": "2026-01-02T03:04:05Z",
        "completed_at": "2026-01-02T03:04:06Z"
      },
      {
        "id": "1c2d3e4f-5061-4728-93a4-b5c6d7e8f901",
        "phase": "thinking",
        "message": {"role": "assistant", "content": "They say: from the project."},
        "tool_calls": null,
        "started_at": "2026-01-02T03:04:06Z",
        "completed_at": "2026-01-02T03:04:07Z"
      }
    ],
    "iteration": 2,
    "tokens_used": 120,
    "finished": true,
    "final_response": "They say: from the project.",
    "error": null,
    "started_at": "2026-01-02T03:04:05Z",
    "finished_at": "2026-01-02T03:04:07Z"
  },
  "created_at": "2026-01-02T03:04:05Z",
  "updated_at": "2026-01-02T03:04:07Z",
  "project_dir": "/nonexistent/replaced-by-the-test"
}"#;

fn completion(message: Value, finish_reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "completion",
        "object": "chat.completion",
        "created": 0,
        "model": MODEL,
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}]
    }))
}

fn answer(content: &str, finish_reason: &str) -> ResponseTemplate {
    completion(
        json!({"role": "assistant", "content": content}),
        finish_reason,
    )
}

fn calling(calls: &[(&str, &str, Value)]) -> ResponseTemplate {
    let tool_calls: Vec<Value> = calls
        .iter()
        .map(|(id, name, arguments)| {
            json!({
                "id": id,
                "type": "function",
                "function": {"name": name, "arguments": arguments.to_string()}
            })
        })
        .collect();
    completion(
        json!({"role": "assistant", "content": null, "tool_calls": tool_calls}),
        "tool_calls",
    )
}

fn reading_notes(id: &str) -> ResponseTemplate {
    calling(&[(id, "read_file", json!({"path": NOTES}))])
}

/// An endpoint answering each request with the next of `replies`.
async fn replying(replies: Vec<ResponseTemplate>) -> MockServer {
    let server = MockServer::start().await;
    for reply in replies {
        Mock::given(method("POST"))
            .respond_with(reply)
            .up_to_n_times(1)
            .mount(&server)
            .await;
    }
    server
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .expect("recorded requests")
        .iter()
        .map(|request| request.body_json().expect("a JSON body"))
        .collect()
}

/// The agent `zone run` builds against `server`, without MCP servers.
fn local(server: &MockServer, working_directory: &Path) -> Agent {
    limited(
        server,
        working_directory,
        Config::default().max_iterations as usize,
    )
}

/// [`local`], given at most `maximum_iterations` model rounds.
fn limited(server: &MockServer, working_directory: &Path, maximum_iterations: usize) -> Agent {
    let config = Config {
        llm_base_url: Some(server.uri()),
        model: MODEL.to_string(),
        ..Config::default()
    };
    agent(
        LlmClient::new(llm_config(&config).expect("an endpoint")),
        registry(),
        0,
        maximum_iterations,
        working_directory,
    )
}

fn project() -> TempDir {
    let directory = TempDir::new().unwrap();
    std::fs::write(directory.path().join(NOTES), NOTED).unwrap();
    directory
}

fn results(state: &AgentState) -> Vec<&ToolCallResult> {
    state
        .steps
        .iter()
        .filter_map(|step| step.tool_calls.as_ref())
        .flatten()
        .collect()
}

fn session_file(sessions: &Path, id: impl std::fmt::Display) -> PathBuf {
    sessions.join(format!("{id}.json"))
}

#[tokio::test]
async fn a_cli_run_approves_every_call() {
    let directory = TempDir::new().unwrap();
    let server = replying(vec![
        calling(&[
            (
                "call_write",
                "write_file",
                json!({"path": "note.txt", "content": "written", "reason": REASON}),
            ),
            (
                "call_run",
                "run_command",
                json!({"command": "echo", "args": ["ran"], "reason": REASON}),
            ),
        ]),
        answer("Done.", "stop"),
    ])
    .await;

    let state = local(&server, directory.path())
        .run("Write a note, then echo", &CliCallback::new(false))
        .await
        .expect("the run answers");

    assert_eq!(
        std::fs::read_to_string(directory.path().join("note.txt")).unwrap(),
        "written"
    );
    let results = results(&state);
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results.iter().all(|result| result.success), "{results:?}");
    assert_eq!(results[1].result, "stdout:\nran\n");
    assert_eq!(state.final_response.as_deref(), Some("Done."));
}

#[tokio::test]
async fn a_cli_turn_sends_zones_temperature() {
    let directory = TempDir::new().unwrap();
    let server = replying(vec![answer("Hello.", "stop")]).await;

    local(&server, directory.path())
        .run("Say hello", &CliCallback::new(false))
        .await
        .expect("the run answers");

    let bodies = bodies(&server).await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0]["temperature"],
        json!(f64::from(0.7_f32)),
        "{}",
        bodies[0]
    );
    assert_eq!(bodies[0]["max_tokens"], json!(4096), "{}", bodies[0]);
    assert_eq!(bodies[0]["model"], MODEL);
}

#[tokio::test]
async fn a_cli_call_without_an_id_is_minted_call_() {
    let directory = project();
    let server = replying(vec![reading_notes(""), answer("Read.", "stop")]).await;

    let state = local(&server, directory.path())
        .run("Read the notes", &CliCallback::new(false))
        .await
        .expect("the run answers");

    let results = results(&state);
    let id = &results[0].call.id;
    assert!(id.starts_with("call_"), "{id}");
    assert_eq!(results[0].result, NOTED);
    let answered = &bodies(&server).await[1]["messages"];
    let tool = answered
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("the tool result is sent back");
    assert_eq!(tool["tool_call_id"], json!(id));
    assert_eq!(answered[2]["tool_calls"][0]["id"], json!(id));
}

#[tokio::test]
async fn a_cli_turn_ends_on_a_length_finish() {
    let directory = TempDir::new().unwrap();
    let server = replying(vec![
        answer("Cut off mid", "length"),
        answer("Never asked for.", "stop"),
    ])
    .await;

    let state = local(&server, directory.path())
        .run("Write at length", &CliCallback::new(false))
        .await
        .expect("the run answers");

    assert!(state.finished);
    assert_eq!(state.final_response.as_deref(), Some("Cut off mid"));
    assert_eq!(bodies(&server).await.len(), 1);
}

#[tokio::test]
async fn a_cli_turn_retries_an_empty_answer() {
    let directory = TempDir::new().unwrap();
    let server = replying(vec![
        answer("", "stop"),
        answer("", "stop"),
        answer("", "stop"),
        answer("Too late.", "stop"),
    ])
    .await;

    let error = local(&server, directory.path())
        .run("Say something", &CliCallback::new(false))
        .await
        .expect_err("three empty answers fail the turn");

    assert!(matches!(error, abnegate_agent::RunError::Empty), "{error}");
    assert_eq!(bodies(&server).await.len(), 3);
}

#[tokio::test]
async fn a_provider_failure_reads_as_a_provider_error() {
    let directory = TempDir::new().unwrap();
    let server = replying(vec![
        ResponseTemplate::new(400).set_body_string("bad request"),
    ])
    .await;

    let error = local(&server, directory.path())
        .run("Say hello", &CliCallback::new(false))
        .await
        .expect_err("a refused request fails the turn");

    let message = RunError::from(error).to_string();
    assert!(
        message.starts_with("Agent error: Provider error: "),
        "{message}"
    );
}

#[tokio::test]
async fn a_turn_past_its_iterations_reads_as_maximum_iterations_exceeded() {
    let directory = project();
    let server = replying(vec![reading_notes("call_one"), reading_notes("call_two")]).await;

    let error = limited(&server, directory.path(), 1)
        .run("Read the notes", &CliCallback::new(false))
        .await
        .expect_err("a turn that never answers fails");

    assert_eq!(
        RunError::from(error).to_string(),
        "Agent error: Maximum iterations exceeded"
    );
}

#[tokio::test]
async fn the_cli_offers_zones_tools_unwaited() {
    let directory = TempDir::new().unwrap();
    let server = replying(vec![answer("Hello.", "stop")]).await;

    local(&server, directory.path())
        .run("Say hello", &CliCallback::new(false))
        .await
        .expect("the run answers");

    let body = &bodies(&server).await[0];
    let tools = body["tools"].as_array().expect("tools are offered");
    let mut names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ZONES_TOOLS);
    let run_command = tools
        .iter()
        .find(|tool| tool["function"]["name"] == "run_command")
        .unwrap();
    let schema = run_command["function"]["parameters"].to_string();
    assert!(schema.contains(WaitFor::Withheld.condition()), "{schema}");
}

#[tokio::test]
async fn the_cli_context_sets_the_public_web_gate() {
    let directory = TempDir::new().unwrap();
    let _vpn = Hold::required_off();
    let server = replying(vec![
        calling(&[(
            "call_curl",
            "run_command",
            json!({"command": "curl", "args": ["https://example.com"], "reason": REASON}),
        )]),
        answer("Offline.", "stop"),
    ])
    .await;

    let state = local(&server, directory.path())
        .run("Fetch example.com", &CliCallback::new(false))
        .await
        .expect("the run answers");

    let results = results(&state);
    assert!(!results[0].success, "{results:?}");
    assert!(results[0].result.contains(OFFLINE), "{}", results[0].result);
}

#[tokio::test]
async fn a_session_saved_by_the_old_cli_still_resumes() {
    let directory = project();
    let sessions = TempDir::new().unwrap();
    let mut saved: Value = serde_json::from_str(OLD_SESSION).unwrap();
    saved["project_dir"] = json!(directory.path().display().to_string());
    std::fs::write(
        session_file(sessions.path(), OLD_SESSION_ID),
        saved.to_string(),
    )
    .unwrap();
    let store = FileSessionStore::new(sessions.path().to_path_buf());
    let server = replying(vec![reading_notes("call_again"), answer("Still.", "stop")]).await;

    let mut session = store
        .load(uuid::Uuid::parse_str(OLD_SESSION_ID).unwrap())
        .await
        .expect("the old session loads");
    assert_eq!(working_directory(&session), directory.path());
    assert_eq!(
        results(&session.state)[0].duration,
        Duration::from_millis(12)
    );
    let agent = local(&server, &working_directory(&session));
    proceed(
        &agent,
        &store,
        &mut session,
        "Read them again",
        &CliCallback::new(false),
    )
    .await
    .expect("the old session resumes");

    let resumed = store.load(session.id).await.unwrap();
    assert!(resumed.state.finished);
    assert_eq!(resumed.state.final_response.as_deref(), Some("Still."));
    assert_eq!(resumed.state.messages.len(), 9);
    let results = results(&resumed.state);
    assert_eq!(results.len(), 2);
    assert_eq!(results[1].result, NOTED);
    let sent = &bodies(&server).await[0]["messages"];
    assert_eq!(sent[3]["tool_call_id"], "zone_0123456789abcdef");
    assert_eq!(sent[5]["content"], "Read them again");
}

#[cfg(unix)]
#[tokio::test]
async fn a_saved_session_is_private_and_writes_the_new_keys() {
    use std::os::unix::fs::PermissionsExt;

    let directory = project();
    let sessions = TempDir::new().unwrap();
    let store = FileSessionStore::new(sessions.path().to_path_buf());
    let server = replying(vec![reading_notes("call_read"), answer("Read.", "stop")]).await;
    let prompt = "Read the notes";
    let state = local(&server, directory.path())
        .run(prompt, &CliCallback::new(false))
        .await
        .expect("the run answers");

    let session = save(&store, state, prompt, directory.path())
        .await
        .expect("the session saves");

    let path = session_file(sessions.path(), session.id);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["title"], prompt);
    assert_eq!(
        written["project_directory"],
        json!(directory.path().display().to_string())
    );
    assert!(written.get("project_dir").is_none(), "{written}");
    let result = &written["state"]["steps"][0]["tool_calls"][0];
    assert!(result["duration_milliseconds"].is_u64(), "{result}");
    assert!(result.get("duration_ms").is_none(), "{result}");
}
