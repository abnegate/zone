//! `wait_for` end to end: what a park costs a run, what it gives back, who may
//! open one, and what a chat turn does either side of the wait it opened.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use futures_util::SinkExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_core::llm::{LlmClient, LlmConfig, Message};
use zone_core::tools::Session;
use zone_core::tools::job::{self, JobCommand, Jobs};
use zone_server::agent::{
    AgentEvent, AgentRun, ApprovalPolicy, ChatTools, LoopBudget, Spend, WorkspaceScope, run, wait,
};
use zone_server::db::tasks;
use zone_server::state::AppState;

/// Where a settled wait's injected call id ends, so a replayed transcript can
/// be searched for the pair without re-deriving the whole id.
const SETTLED: &str = "#settled";

/// The phase and message a park writes into the run's own log.
const PHASE_WAITING: &str = "waiting";
const WAITING_ON_OUTCOME: &str = "Task run is waiting on something outside its loop";

/// The stall watchdog's own level, and the only thing in these runs that could
/// write one — which is what makes its absence an assertion.
const LEVEL_WARNING: &str = "warning";

/// Execution slots the worker admits at once.
///
/// Mirrors the worker's private `MAX_CONCURRENT_TASKS`. The test that reads it
/// checks the ceiling as well as the release, so a changed ceiling fails there
/// rather than quietly weakening the assertion.
const ADMISSION_SLOTS: usize = 5;

/// The admission semaphore is process-wide, so the one test that counts its
/// slots cannot run beside anything else that takes one.
static ADMISSION: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

const TASK_MODEL: &str = "gpt-4";
const CHAT_MODEL: &str = "llama3.2:3b";

/// Rounds a chat turn is given, so the ceiling a resumed run is built with can
/// be counted rather than inferred.
const CHAT_ROUNDS: usize = 3;

/// Longest a poll waits for a run to park, resume or finish.
const SETTLE: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(25);
const FRAME: Duration = Duration::from_secs(30);

/// Longest the gate-opening thread is given before the job counts as not
/// reading its end of the FIFO.
const GATE: Duration = Duration::from_secs(10);

/// How long a run that queued for a slot holds the one it got.
const HELD: Duration = Duration::from_secs(30);

/// A wait window that outlives the stall watchdog's threshold, so a run parked
/// for all of it would have been announced twice over.
fn past_the_watchdog() -> Duration {
    Duration::from_secs(zone_core::tools::MAX_SLEEP_SECS) + Duration::from_secs(2)
}

fn environment() -> HashMap<String, String> {
    std::env::var("PATH")
        .map(|path| HashMap::from([("PATH".to_string(), path)]))
        .unwrap_or_default()
}

fn directory() -> TempDir {
    TempDir::new().expect("a temporary working directory")
}

/// A FIFO nothing is writing to, so `cat` on it runs until a test says
/// otherwise. A job whose lifetime the test owns outright is what removes
/// every sleep this file would have to race against.
fn gate(root: &Path) -> PathBuf {
    let path = root.join("gate");
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("mkfifo is available");
    assert!(made.success(), "could not create a FIFO at {path:?}");
    path
}

/// Write one line into the gate and close it.
///
/// Writing while the run is parked is what proves the child outlived the turn
/// that started it; closing is the end of input that exits `cat` zero.
async fn release(gate: &Path) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let path = gate.to_path_buf();
    std::thread::spawn(move || {
        let written = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, b"alive\n"));
        let _ = sender.send(written.is_ok());
    });
    assert!(
        tokio::time::timeout(GATE, receiver)
            .await
            .expect("the job is reading its gate")
            .unwrap_or(false),
        "could not write the line that ends the job"
    );
}

/// One streamed completion built from `deltas`.
fn stream(deltas: Vec<Value>) -> ResponseTemplate {
    let mut body = String::new();
    for delta in deltas {
        let chunk = json!({
            "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
            "choices": [{"index": 0, "delta": delta, "finish_reason": null}]
        });
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    let end = json!({
        "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    });
    body.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
    ResponseTemplate::new(200)
        .insert_header("Content-Type", "text/event-stream")
        .set_body_string(body)
}

fn text(content: &str) -> Vec<Value> {
    vec![json!({"content": content})]
}

fn calling(id: &str, name: &str, arguments: Value) -> Vec<Value> {
    vec![json!({"tool_calls": [{
        "index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": arguments.to_string()}
    }]})]
}

/// A read the loop counts as progress, because its signature is new every time
/// and a repeated unchanged read would finalize the turn early.
fn a_read(round: usize) -> Vec<Value> {
    calling(
        &format!("read-{round}"),
        "read_file",
        json!({"path": format!("/nonexistent/round-{round}")}),
    )
}

/// Whether this body is a round of the agent loop rather than an aside.
///
/// Only the loop streams. A summariser or a title pass posts to the same path
/// and must not be counted as a round or advance a script.
fn is_round(body: &Value) -> bool {
    body["stream"] == json!(true)
}

/// A provider that answers each round of the agent loop from `script`, by round
/// number, and answers everything else without advancing it.
async fn scripted<Script>(script: Script) -> MockServer
where
    Script: Fn(usize, &Request) -> ResponseTemplate + Send + Sync + 'static,
{
    let provider = MockServer::start().await;
    let rounds = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            if !is_round(&body) {
                return ResponseTemplate::new(200).set_body_json(json!({
                    "id": "aside", "object": "chat.completion", "created": 0, "model": "test",
                    "choices": [{
                        "index": 0, "finish_reason": "stop",
                        "message": {"role": "assistant", "content": "Aside."}
                    }]
                }));
            }
            script(rounds.fetch_add(1, Ordering::SeqCst), request)
        })
        .mount(&provider)
        .await;
    provider
}

/// The job id a spawn receipt in this request's replay carried, read the way
/// the chat layer reads one.
fn receipt_job(request: &Request) -> Option<String> {
    let body: Value = serde_json::from_slice(&request.body).ok()?;
    body["messages"]
        .as_array()?
        .iter()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str())
        .find_map(job::parse_started)
}

/// The first job id any receipt carried.
///
/// A second attempt rebuilds its context from the task prompt alone, so a
/// script that has to name the same job again cannot read it back out of the
/// replay and remembers it instead.
#[derive(Clone, Default)]
struct Remembered(Arc<Mutex<Option<String>>>);

impl Remembered {
    fn read(&self, request: &Request) -> String {
        let mut held = self.0.lock().expect("the remembered job id is readable");
        if let Some(id) = receipt_job(request) {
            *held = Some(id);
        }
        held.clone().expect("a spawn receipt has been seen by now")
    }
}

/// A task, a run of it nobody has claimed, and the member that triggered it.
struct Prepared {
    task: Uuid,
    run: Uuid,
    workspace: Uuid,
    user: Uuid,
}

async fn prepared_run(pool: &PgPool, title: &str) -> Prepared {
    let (_organization, workspace, user) = common::setup_workspace_member(pool).await;
    prepared_run_in(pool, workspace, user, title).await
}

async fn prepared_run_in(pool: &PgPool, workspace: Uuid, user: Uuid, title: &str) -> Prepared {
    let task = tasks::create_task_as(
        pool,
        workspace,
        &[],
        title,
        "Start the work in the background and wait for it",
        None,
        None,
        true,
        None,
        Some(user),
    )
    .await
    .expect("a task to run");
    sqlx::query("UPDATE tasks SET model_name = $2 WHERE id = $1")
        .bind(task.id)
        .bind(TASK_MODEL)
        .execute(pool)
        .await
        .expect("the task pins a model");
    let run = tasks::create_task_run_as(pool, task.id, Some(user))
        .await
        .expect("a run of it");
    Prepared {
        task: task.id,
        run: run.id,
        workspace,
        user,
    }
}

/// A task run the worker is executing, and what a test asks it about.
struct Driven {
    pool: PgPool,
    run: Uuid,
    provider: MockServer,
    worker: tokio::task::JoinHandle<()>,
}

impl Driven {
    async fn start(state: &AppState, prepared: &Prepared, provider: MockServer) -> Self {
        let pool = state.db().clone();
        let owned = state.clone();
        let (run, task) = (prepared.run, prepared.task);
        let worker = tokio::spawn(async move {
            zone_server::workers::task::execute_task_run(&owned, run, task).await;
        });
        Self {
            pool,
            run,
            provider,
            worker,
        }
    }

    async fn status(&self) -> String {
        sqlx::query_scalar("SELECT status FROM task_runs WHERE id = $1")
            .bind(self.run)
            .fetch_one(&self.pool)
            .await
            .expect("the run is still readable")
    }

    /// Status, what the run waits on, whether anything is answerable, and the
    /// phase, from one read: a parked row can shed all four between queries.
    #[allow(clippy::type_complexity)]
    async fn row(&self) -> (String, Option<Value>, Option<Value>, Option<String>) {
        sqlx::query_as(
            "SELECT status, pending_wait, pending_question, current_phase FROM task_runs WHERE id = $1",
        )
        .bind(self.run)
        .fetch_one(&self.pool)
        .await
        .expect("the run is still readable")
    }

    /// Wait until the run reads as parked, and answer with the parked row.
    #[allow(clippy::type_complexity)]
    async fn parks(&self) -> (String, Option<Value>, Option<Value>, Option<String>) {
        let parked = tokio::time::timeout(SETTLE, async {
            loop {
                let row = self.row().await;
                if row.0 == PHASE_WAITING {
                    return row;
                }
                tokio::time::sleep(POLL).await;
            }
        })
        .await;
        match parked {
            Ok(row) => row,
            Err(_) => panic!("the run never parked; it is {}", self.status().await),
        }
    }

    async fn settles_on(&self, status: &str) {
        let settled = tokio::time::timeout(SETTLE, async {
            while self.status().await != status {
                tokio::time::sleep(POLL).await;
            }
        })
        .await;
        if settled.is_err() {
            panic!(
                "the run never reached {status}; it is {}",
                self.status().await
            );
        }
    }

    /// Every round of the agent loop the provider was sent, in order.
    async fn rounds(&self) -> Vec<Value> {
        self.provider
            .received_requests()
            .await
            .expect("the provider recorded its requests")
            .into_iter()
            .filter(|request| request.url.path() == "/chat/completions")
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(is_round)
            .collect()
    }

    /// The run's own account of every park and resume, in order.
    async fn waiting_lines(&self) -> Vec<(String, Option<Value>)> {
        sqlx::query_as(
            "SELECT message, metadata FROM task_run_logs WHERE task_run_id = $1 AND phase = $2 AND agent_type = 'agent' ORDER BY created_at, id",
        )
        .bind(self.run)
        .bind(PHASE_WAITING)
        .fetch_all(&self.pool)
        .await
        .expect("the run's log is readable")
    }

    async fn warnings(&self) -> Vec<(String, String)> {
        sqlx::query_as("SELECT phase, message FROM task_run_logs WHERE task_run_id = $1 AND log_level = $2 ORDER BY created_at")
            .bind(self.run)
            .bind(LEVEL_WARNING)
            .fetch_all(&self.pool)
            .await
            .expect("the run's log is readable")
    }

    async fn artifacts(&self) -> Value {
        sqlx::query_scalar("SELECT artifacts FROM task_runs WHERE id = $1")
            .bind(self.run)
            .fetch_one(&self.pool)
            .await
            .expect("the run is still readable")
    }

    fn finish(self) {
        self.worker.abort();
    }
}

/// The envelope and the result a settled wait is injected as, from one round.
fn settled_pair(body: &Value) -> Option<(Value, Value)> {
    let messages = body["messages"].as_array()?;
    let position = messages.iter().position(|message| {
        message["tool_calls"]
            .as_array()
            .is_some_and(|calls| calls.iter().any(is_settled_call))
    })?;
    Some((
        messages.get(position)?.clone(),
        messages.get(position + 1)?.clone(),
    ))
}

fn is_settled_call(call: &Value) -> bool {
    call["id"].as_str().is_some_and(|id| id.ends_with(SETTLED))
        && call["function"]["name"] == wait::WAIT_FOR
}

/// A claimed run, because a leased tool set refuses every call on a run it
/// does not own.
struct Leased {
    run: Uuid,
    owner: Uuid,
    workspace: Uuid,
    user: Uuid,
}

async fn leased_run(pool: &PgPool) -> Leased {
    let prepared = prepared_run(pool, "Waits on something").await;
    let owner = Uuid::new_v4();
    assert!(
        tasks::claim_task_run(pool, prepared.run, owner)
            .await
            .expect("the claim is writable"),
        "a fresh run is claimable"
    );
    Leased {
        run: prepared.run,
        owner,
        workspace: prepared.workspace,
        user: prepared.user,
    }
}

async fn leased_tools(state: &AppState, cwd: &Path, leased: &Leased) -> ChatTools {
    ChatTools::for_task(
        state,
        cwd.to_path_buf(),
        leased.workspace,
        Some(leased.user),
    )
    .await
    .with_task_lease(state.db().clone(), leased.run, leased.owner)
}

async fn chat_tools(state: &AppState, workspace: Uuid, user: Uuid, chat: Uuid) -> ChatTools {
    ChatTools::build(WorkspaceScope {
        state: state.clone(),
        workspace_id: workspace,
        chat_id: Some(chat),
        user_id: user,
    })
    .await
}

fn waiting_on_job(job: &str) -> String {
    json!({"kind": wait::KIND_JOB, "id": job}).to_string()
}

fn waiting_on_run(run: Uuid) -> String {
    json!({"kind": wait::KIND_TASK_RUN, "id": run}).to_string()
}

/// Remove a working directory a job logged into, and prove it went.
async fn discard(root: TempDir) {
    let left = root.keep();
    tokio::fs::remove_dir_all(&left)
        .await
        .expect("the working directory is removable");
    assert!(
        !left.exists(),
        "a job's working directory outlived its test"
    );
}

/// The one assertion the primitive rests on, through a run the worker drove
/// itself.
///
/// A backgrounded command outlives the turn that started it, the wait parks
/// the run with nothing to answer and no admission slot, the job exits, and
/// the outcome arrives in the model's context as the pair the store would
/// accept. The call the wait spent is gone; the round it opened in is not,
/// which is the whole difference between a wait and a question.
#[tokio::test]
async fn a_backgrounded_command_survives_its_turn_and_its_wait_resumes_and_finishes_the_run() {
    common::init_tracing();
    let _admission = ADMISSION.read().await;
    let root = directory();
    let gate = gate(root.path());
    let waited = gate.to_string_lossy().into_owned();
    let remembered = Remembered::default();
    let provider = scripted(move |round, request| match round {
        0 => stream(calling(
            "spawn-build",
            "run_command",
            json!({
                "command": "cat", "args": [waited], "background": true,
                "reason": "the build takes longer than a turn"
            }),
        )),
        1 => stream(calling(
            "wait-build",
            wait::WAIT_FOR,
            json!({
                "kind": wait::KIND_JOB, "id": remembered.read(request),
                "timeout_secs": 120
            }),
        )),
        2 => stream(calling(
            "tail-build",
            job::TAIL_JOB,
            json!({"id": remembered.read(request)}),
        )),
        _ => stream(text("The build finished cleanly.")),
    })
    .await;

    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool.clone());
    let prepared = prepared_run(&pool, "Backgrounds a build and waits for it").await;
    let driven = Driven::start(&state, &prepared, provider).await;

    let (status, pending_wait, pending_question, phase) = driven.parks().await;
    assert_eq!(status, PHASE_WAITING, "the wait did not park the run");
    let parked: wait::Waiting =
        serde_json::from_value(pending_wait.expect("a parked run stores what it waits on"))
            .expect("the stored wait is a wait");
    assert_eq!(parked.kind, wait::KIND_JOB);
    assert!(
        parked.id.starts_with("job_"),
        "the run waits on the job it started: {}",
        parked.id
    );
    assert_eq!(
        pending_question, None,
        "a run waiting on a job has nothing to answer, which is what answer_run refuses with a \
         conflict"
    );
    assert_eq!(
        phase.as_deref(),
        Some(PHASE_WAITING),
        "the phase shown beside the badge must say the run is waiting"
    );

    // The job is still reading its gate a whole turn after the call that
    // started it returned, which is what surviving the turn means. Closing the
    // gate is what ends it, so the outcome is an exit and not a kill.
    release(&gate).await;

    driven.settles_on("completed").await;
    let rounds = driven.rounds().await;
    assert_eq!(
        rounds.len(),
        4,
        "the run spawned, waited, read the log and answered, in that many rounds"
    );

    let (envelope, result) = settled_pair(&rounds[2])
        .expect("the resumed round carries the outcome as an envelope and its result");
    let call = envelope["tool_calls"][0]["id"]
        .as_str()
        .expect("the injected call has an id")
        .to_string();
    assert_eq!(call, format!("wait-build{SETTLED}"));
    assert_eq!(result["role"], "tool");
    assert_eq!(result["tool_call_id"], call);
    let outcome = result["content"].as_str().expect("an outcome").to_string();
    assert!(
        outcome.starts_with(&format!("{} exited with code 0 after ", parked.id)),
        "the model was told the job ended some other way: {outcome}"
    );
    assert!(
        replayed(&rounds[2]).any(|content| content.contains(&parked.id)
            && content.contains(&format!("read it with {}", job::TAIL_JOB))),
        "the spawn receipt the model read the job id from is no longer in the replay"
    );
    assert!(
        replayed(&rounds[3])
            .any(|content| content.contains("alive") && content.contains("[job exited 0; next=")),
        "the log the job wrote while the run was parked never came back through tail_job"
    );

    assert_eq!(
        driven.artifacts().await["tool_calls"],
        3,
        "the spawn, the wait and the log read are three calls the attempt made"
    );
    let lines = driven.waiting_lines().await;
    assert_eq!(lines.len(), 2, "a park and its ending: {lines:?}");
    assert_eq!(lines[0].0, WAITING_ON_OUTCOME);
    let payload = lines[0].1.as_ref().expect("the park says what it waits on");
    assert_eq!(payload["tool_call_id"], "wait-build");
    assert_eq!(payload["waiting"]["id"], parked.id);
    assert!(
        payload.get("questions").is_none(),
        "a wait is not a question: {payload}"
    );
    assert_eq!(
        lines[1].0, outcome,
        "the outcome is its own headline, because what it says is the point of the line"
    );
    assert_eq!(
        lines[1].1.as_ref().expect("a resume payload")["resume"],
        outcome
    );
    assert!(
        driven.warnings().await.is_empty(),
        "a run that was waiting, not stalling, was announced as stalled"
    );

    let (status, pending_wait, _, phase) = driven.row().await;
    assert_eq!(status, "completed");
    assert_eq!(pending_wait, None, "a finished run waits on nothing");
    assert_ne!(
        phase.as_deref(),
        Some(PHASE_WAITING),
        "a finished run must not go on reading as waiting"
    );

    driven.finish();
    assert!(
        !root.path().join(job::JOB_LOG_DIRECTORY).exists(),
        "the job logged beside its gate instead of inside the checkout it ran in"
    );
    discard(root).await;
}

/// Every tool result in one round's replay.
fn replayed(body: &Value) -> impl Iterator<Item = &str> {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str())
}

/// The expression the refund is, read off the event the loop yields.
///
/// A wait hands back the round it opened in and keeps the call it spent, so a
/// resumed turn continues on the same ceiling rather than a smaller one.
/// Driven through a leased tool set and a real job, because a tool set built
/// without a lease stages under a detached session and refuses both.
#[tokio::test]
async fn a_wait_hands_the_resumed_turn_back_the_round_it_opened_in() {
    common::init_tracing();
    let root = directory();
    let gate = gate(root.path());
    let waited = gate.to_string_lossy().into_owned();
    let remembered = Remembered::default();
    let provider = scripted(move |round, request| match round {
        0 => stream(calling(
            "spawn-refund",
            "run_command",
            json!({
                "command": "cat", "args": [waited], "background": true,
                "reason": "the build takes longer than a turn"
            }),
        )),
        _ => stream(calling(
            "wait-refund",
            wait::WAIT_FOR,
            json!({"kind": wait::KIND_JOB, "id": remembered.read(request)}),
        )),
    })
    .await;

    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool.clone());
    let leased = leased_run(&pool).await;
    let tools = leased_tools(&state, root.path(), &leased).await;
    let budget = LoopBudget::task();

    let events: Vec<AgentEvent> = tokio::time::timeout(
        SETTLE,
        run(AgentRun {
            llm: LlmClient::new(LlmConfig {
                base_url: provider.uri(),
                ..LlmConfig::default()
            }),
            model: "test".to_string(),
            tools,
            messages: vec![Message::user("Start the build and wait for it.")],
            budget,
            approval: ApprovalPolicy::auto(),
        })
        .collect(),
    )
    .await
    .expect("a bounded turn");

    let spent = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::WaitRequired { spent, .. } => Some(*spent),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the turn never suspended on a wait: {events:?}"));
    assert_eq!(
        spent,
        Spend {
            iterations: 1,
            tool_calls: 2
        },
        "the wait opened in the second round and is the second call the turn made"
    );
    let resumed = budget.less(spent);
    assert_eq!(
        resumed.max_iterations,
        budget.max_iterations - 1,
        "the rounds already spent stay spent, and the wait's own is not one of them"
    );
    assert_eq!(
        resumed.max_tool_calls,
        budget.max_tool_calls - 2,
        "a refunded round still costs the call that opened the wait"
    );

    assert_eq!(
        Jobs::kill_session(Session::Task(leased.run)).await,
        1,
        "the job the turn left running is the run's to kill"
    );
    wait::reset_session(Session::Task(leased.run));
    discard(root).await;
}

/// The stall line exists for a wedged turn. A run parked on a wait has left
/// the loop the watchdog watches, and announcing it every minute would bury
/// the log it writes into and tell whoever reads it the opposite of the truth.
///
/// Held past the watchdog's own threshold in real time, because the only
/// honest way to prove nothing announced it is to give the announcement its
/// chance.
#[tokio::test]
async fn the_stall_watchdog_says_nothing_about_a_run_parked_past_its_threshold() {
    common::init_tracing();
    let _admission = ADMISSION.read().await;
    let root = directory();
    let marker = root.path().join("marker");
    tokio::fs::write(&marker, b"following\n")
        .await
        .expect("a file to follow");
    let followed = marker.to_string_lossy().into_owned();
    let window = past_the_watchdog();
    let remembered = Remembered::default();
    let provider = scripted(move |round, request| match round {
        0 => stream(calling(
            "spawn-watchdog",
            "run_command",
            json!({
                "command": "tail", "args": ["-f", followed], "background": true,
                "reason": "follow the build log"
            }),
        )),
        1 => stream(calling(
            "wait-watchdog",
            wait::WAIT_FOR,
            json!({
                "kind": wait::KIND_JOB, "id": remembered.read(request),
                "timeout_secs": window.as_secs()
            }),
        )),
        _ => stream(text("The wait ran out, so nothing is confirmed.")),
    })
    .await;

    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool.clone());
    let prepared = prepared_run(&pool, "Waits longer than the watchdog").await;
    let driven = Driven::start(&state, &prepared, provider).await;

    let (_, pending_wait, _, _) = driven.parks().await;
    let parked: wait::Waiting =
        serde_json::from_value(pending_wait.expect("a parked run stores its wait"))
            .expect("the stored wait is a wait");

    // The job follows a file nothing appends to, so the only thing that can end
    // this park is its own deadline — which is the point: the park has to
    // outlast the watchdog's threshold with the run reading as healthy.
    tokio::time::timeout(SETTLE + window, async {
        while driven.status().await == PHASE_WAITING {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .expect("the wait never ran out");

    let lines = driven.waiting_lines().await;
    assert_eq!(
        lines.len(),
        2,
        "the park and its ending are the only lines a wait writes: {lines:?}"
    );
    assert_eq!(lines[0].0, WAITING_ON_OUTCOME);
    assert!(
        lines[1].0.starts_with("Timed out after ")
            && lines[1].0.contains(&wait::job_subject(&parked.id)),
        "silence is not success: the model is told which wait did not finish: {}",
        lines[1].0
    );

    driven.settles_on("completed").await;
    let warnings = driven.warnings().await;
    assert!(
        warnings.is_empty(),
        "a run that was waiting, not stalling, was announced as stalled: {warnings:?}"
    );
    driven.finish();
    discard(root).await;
}

/// A wait is keyed to the session that started the job, the same way a read of
/// its log is. A context with no session to key one to has nothing to wait on
/// at all.
#[tokio::test]
async fn a_wait_on_a_job_is_refused_from_every_session_but_the_one_that_started_it() {
    common::init_tracing();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool.clone());
    let owning = leased_run(&pool).await;
    let other = leased_run(&pool).await;
    let root = directory();

    let started = Jobs::spawn(
        Session::Task(owning.run),
        &JobCommand::new("echo", vec!["ledger".to_string()]),
        root.path(),
        &environment(),
    )
    .await
    .expect("a job the run owns");
    let missing = format!("No job {} in this session.", started.id);

    for (surface, tools) in [
        (
            "another task run",
            leased_tools(&state, root.path(), &other).await,
        ),
        (
            "a chat",
            chat_tools(&state, owning.workspace, owning.user, Uuid::new_v4()).await,
        ),
    ] {
        let refused = tools
            .execute(wait::WAIT_FOR, &waiting_on_job(&started.id))
            .await;
        assert!(
            !refused.success,
            "{surface} waited on a job it did not start"
        );
        assert!(
            refused
                .error
                .as_deref()
                .is_some_and(|error| error.contains(&missing)),
            "{surface} was refused in other words than the frozen ones: {refused:?}"
        );
    }

    // A detached context has no session to key a job to, and every entry point
    // says so in the same words: `Jobs::start`, `Jobs::read` and `Jobs::settled`
    // all refuse it outright rather than letting it fall through to a lookup
    // that could only ever miss. Nothing leaks either way, since no job is
    // keyed to a detached session; what this pins is that a wait and a read
    // refuse it identically, so neither reads as a job that merely went away.
    let detached =
        ChatTools::for_task(&state, root.path().to_path_buf(), owning.workspace, None).await;
    let refused = detached
        .execute(wait::WAIT_FOR, &waiting_on_job(&started.id))
        .await;
    assert!(
        !refused.success,
        "a detached context opened a wait on a job"
    );
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains(job::UNAVAILABLE)),
        "a detached wait is told it has no session at all, not that the job is missing: {refused:?}"
    );
    let read = detached
        .execute(job::TAIL_JOB, &json!({"id": started.id}).to_string())
        .await;
    assert!(
        read.error
            .as_deref()
            .is_some_and(|error| error.contains(job::UNAVAILABLE)),
        "a detached read is told the same thing in the same words: {read:?}"
    );

    // The owning run can still open one, so nothing above was refused for some
    // reason other than the session it came from.
    let opened = leased_tools(&state, root.path(), &owning)
        .await
        .execute(wait::WAIT_FOR, &waiting_on_job(&started.id))
        .await;
    assert!(opened.success, "the owning run could not wait: {opened:?}");
    assert!(
        opened.output.as_deref().is_some_and(
            |receipt| receipt.starts_with(&format!("Waiting for {} until ", started.id))
        ),
        "the receipt is what execute returns: {opened:?}"
    );

    for run in [owning.run, other.run] {
        Jobs::kill_session(Session::Task(run)).await;
        wait::reset_session(Session::Task(run));
    }
    discard(root).await;
}

/// Ten waits is the allowance, and the eleventh is an error rather than a
/// park: an error never ends the turn, so the model is told to act on what it
/// has instead of parking again. The allowance belongs to the attempt, so the
/// attempt after a transient failure opens its own.
#[tokio::test]
async fn the_eleventh_wait_in_an_attempt_is_refused_and_a_second_attempt_starts_at_zero() {
    common::init_tracing();
    let _admission = ADMISSION.read().await;
    let allowance = wait::MAX_WAITS_PER_ATTEMPT;
    let failing = allowance + 2;
    let remembered = Remembered::default();
    let provider = scripted(move |round, request| {
        let waiting = |call: &str| {
            stream(calling(
                call,
                wait::WAIT_FOR,
                json!({"kind": wait::KIND_JOB, "id": remembered.read(request)}),
            ))
        };
        match round {
            0 => stream(calling(
                "spawn-allowance",
                "run_command",
                json!({
                    "command": "echo", "args": ["started"], "background": true,
                    "reason": "something to wait on more than once"
                }),
            )),
            // The allowance, spent on a job that has already exited so every
            // park settles the instant it is awaited.
            round if round <= allowance => waiting(&format!("wait-allowance-{round}")),
            // One past it, which must come back as a tool error.
            round if round == allowance + 1 => waiting("wait-allowance-over"),
            // A transient provider failure ends the attempt and buys another.
            round if round == failing => {
                ResponseTemplate::new(503).set_body_string("upstream is busy")
            }
            // The first wait of the second attempt, which parks only if the
            // allowance was reset with the attempt.
            round if round == failing + 1 => waiting("wait-allowance-again"),
            _ => stream(text("Everything I waited for has finished.")),
        }
    })
    .await;

    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool.clone());
    let prepared = prepared_run(&pool, "Waits until it runs out of waits").await;
    let driven = Driven::start(&state, &prepared, provider).await;
    driven.settles_on("completed").await;

    let lines = driven.waiting_lines().await;
    let parks = lines
        .iter()
        .filter(|(message, _)| message == WAITING_ON_OUTCOME)
        .count();
    assert_eq!(
        parks,
        allowance + 1,
        "the allowance is the attempt's: {allowance} parks in the first and one in the second"
    );

    let limit = wait::too_many_waits();
    let refusals = driven
        .rounds()
        .await
        .iter()
        .filter(|body| replayed(body).any(|content| content.contains(&limit)))
        .count();
    assert!(
        refusals > 0,
        "the wait past the allowance was not refused in the frozen words"
    );

    // A refused wait never ends the turn, so nothing parked on `wait-over`.
    let parked: Vec<String> = lines
        .iter()
        .filter_map(|(_, metadata)| metadata.as_ref())
        .filter_map(|metadata| metadata["tool_call_id"].as_str().map(str::to_string))
        .collect();
    assert!(
        !parked.iter().any(|call| call == "wait-allowance-over"),
        "a refused wait parked the run anyway: {parked:?}"
    );
    assert!(
        parked.iter().any(|call| call == "wait-allowance-again"),
        "the second attempt could not open a wait of its own: {parked:?}"
    );

    driven.finish();
}

/// A run waiting on a build is not executing. Held through the wait, five runs
/// parked on half-hour waits take a deployment's whole task throughput to
/// zero, and the timeout is the model's to size.
#[tokio::test]
async fn a_run_parked_on_a_wait_gives_its_admission_slot_back() {
    common::init_tracing();
    let _admission = ADMISSION.write().await;
    let root = directory();
    let marker = root.path().join("marker");
    tokio::fs::write(&marker, b"following\n")
        .await
        .expect("a file to follow");
    let followed = marker.to_string_lossy().into_owned();
    let remembered = Remembered::default();
    let parking = scripted(move |round, request| match round {
        0 => stream(calling(
            "spawn-slot",
            "run_command",
            json!({
                "command": "tail", "args": ["-f", followed], "background": true,
                "reason": "follow the build log"
            }),
        )),
        1 => stream(calling(
            "wait-slot",
            wait::WAIT_FOR,
            json!({
                "kind": wait::KIND_JOB, "id": remembered.read(request),
                "timeout_secs": wait::MAX_WAIT_SECS
            }),
        )),
        _ => stream(text("The wait ran out, so nothing is confirmed.")),
    })
    .await;

    // The runs that queue behind the parked one never get an answer, so each
    // holds the slot it took for as long as this test needs it held.
    let holding = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_delay(HELD))
        .mount(&holding)
        .await;

    let pool = common::create_test_pool().await;
    let mut config = common::test_config();
    config.litellm_host = parking.uri();
    config.ollama_host = parking.uri();
    let parked_state = common::create_test_state(config, pool.clone());
    let prepared = prepared_run(&pool, "Parks and gives its slot up").await;
    let driven = Driven::start(&parked_state, &prepared, parking).await;
    driven.parks().await;

    let mut config = common::test_config();
    config.litellm_host = holding.uri();
    config.ollama_host = holding.uri();
    let holding_state = common::create_test_state(config, pool.clone());
    let mut queued = Vec::new();
    let mut workers = Vec::new();
    for slot in 0..=ADMISSION_SLOTS {
        let waiting = prepared_run(&pool, &format!("Queues for a slot {slot}")).await;
        let state = holding_state.clone();
        let (run, task) = (waiting.run, waiting.task);
        queued.push(run);
        workers.push(tokio::spawn(async move {
            zone_server::workers::task::execute_task_run(&state, run, task).await;
        }));
    }

    let filled = tokio::time::timeout(SETTLE, async {
        while admitted(&pool, &queued).await < ADMISSION_SLOTS {
            tokio::time::sleep(POLL).await;
        }
    })
    .await;
    if filled.is_err() {
        panic!(
            "only {} of {ADMISSION_SLOTS} runs were admitted beside a parked one",
            admitted(&pool, &queued).await
        );
    }
    // And no more than that, which is what proves the count above is the whole
    // ceiling rather than an accident of how quickly the runs started.
    tokio::time::sleep(POLL * 20).await;
    assert_eq!(
        admitted(&pool, &queued).await,
        ADMISSION_SLOTS,
        "the parked run held a slot, or the ceiling is no longer {ADMISSION_SLOTS}"
    );

    for worker in workers {
        worker.abort();
    }
    driven.finish();
    Jobs::kill_session(Session::Task(prepared.run)).await;
    wait::reset_session(Session::Task(prepared.run));
    discard(root).await;
}

/// A run only reaches its model once it holds a slot, so a phase is the mark
/// of a run that was admitted.
async fn admitted(pool: &PgPool, queued: &[Uuid]) -> usize {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_runs WHERE id = ANY($1) AND current_phase IS NOT NULL",
    )
    .bind(queued)
    .fetch_one(pool)
    .await
    .expect("the queued runs are readable");
    count as usize
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A chat served over a real socket, with a scripted provider behind it.
struct Chatting {
    pool: PgPool,
    address: String,
    token: String,
    chat: String,
    chat_id: Uuid,
    workspace: Uuid,
    user: Uuid,
    provider: MockServer,
}

/// A chat whose turn is given `rounds` reason/act rounds.
///
/// The ceiling is lowered from its default so the one a resumed run is built
/// with can be counted out of the provider's own transcript, which is the only
/// place a turn's remaining budget is observable from outside.
async fn chatting(provider: MockServer, rounds: usize) -> Chatting {
    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();
    config.chat.rounds = rounds;
    let pool = common::create_test_pool().await;
    let client = common::TestClient::new(common::create_test_router(common::create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let address = common::serve(common::create_test_state(config, pool.clone())).await;
    let (token, chat, workspace) = common::seed_chat(&client, CHAT_MODEL).await;
    let chat_id = Uuid::parse_str(&chat).expect("a chat id");
    let workspace = Uuid::parse_str(&workspace).expect("a workspace id");
    let user =
        sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id = $1 LIMIT 1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .expect("whoever made the workspace is a member of it");
    Chatting {
        pool,
        address,
        token,
        chat,
        chat_id,
        workspace,
        user,
        provider,
    }
}

impl Chatting {
    async fn connect(&self) -> Socket {
        let (mut socket, _) =
            connect_async(format!("ws://{}/ws/chats/{}", self.address, self.chat))
                .await
                .expect("the chat socket accepts a connection");
        socket
            .send(WebSocketMessage::Text(
                json!({"type": "auth", "token": self.token})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("the socket takes an auth frame");
        let init = common::next_frame(&mut socket, FRAME)
            .await
            .expect("an init frame");
        assert_eq!(
            init["type"], "init",
            "the socket did not initialize: {init}"
        );
        socket
    }

    /// A task run of this chat's own workspace, at `status`.
    async fn run_at(&self, status: &str) -> Uuid {
        let prepared = prepared_run_in(
            &self.pool,
            self.workspace,
            self.user,
            "Something to wait on",
        )
        .await;
        if status != "running" {
            tasks::complete_task_run(&self.pool, prepared.run, status, None, None)
                .await
                .expect("the run is finishable")
                .unwrap_or_else(|| panic!("nothing was there to finish"));
        }
        prepared.run
    }

    async fn rounds(&self) -> Vec<Value> {
        self.provider
            .received_requests()
            .await
            .expect("the provider recorded its requests")
            .into_iter()
            .filter(|request| request.url.path() == "/chat/completions")
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(is_round)
            .collect()
    }

    async fn messages(&self, role: &str) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT content FROM messages WHERE chat_id = $1 AND role = $2 ORDER BY created_at",
        )
        .bind(self.chat_id)
        .bind(role)
        .fetch_all(&self.pool)
        .await
        .expect("the chat's messages are readable")
    }

    /// Every settled-wait call this chat has stored, with the entry that
    /// answered it. A pair with no result would be a half the store rejects.
    async fn stored_pairs(&self) -> Vec<(String, Option<String>)> {
        sqlx::query_as(
            "SELECT id, result_id FROM chat_calls WHERE chat_id = $1 AND id LIKE $2 ORDER BY id",
        )
        .bind(self.chat_id)
        .bind(format!("%{SETTLED}"))
        .fetch_all(&self.pool)
        .await
        .expect("the chat's calls are readable")
    }

    async fn entry(&self, id: &str) -> Value {
        sqlx::query_scalar("SELECT message FROM chat_entries WHERE chat_id = $1 AND id = $2")
            .bind(self.chat_id)
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("the stored entry is readable")
    }
}

async fn say(socket: &mut Socket, content: &str) {
    socket
        .send(WebSocketMessage::Text(
            json!({"type": "send", "content": content})
                .to_string()
                .into(),
        ))
        .await
        .expect("the socket takes a send frame");
}

/// Every frame of one turn, up to and including its end.
async fn turn(socket: &mut Socket) -> Vec<Value> {
    let mut frames = Vec::new();
    while let Some(frame) = common::next_frame(socket, FRAME).await {
        let end = frame["type"] == "message_end";
        frames.push(frame);
        if end {
            return frames;
        }
    }
    panic!("the turn never ended: {frames:?}");
}

fn tagged<'a>(frames: &'a [Value], tag: &str) -> Vec<&'a Value> {
    frames.iter().filter(|frame| frame["type"] == tag).collect()
}

/// Decision 3(a): the wait suspends the turn, not the message.
///
/// One `message_start` covers both runs, and the run that resumes is built
/// with what the first one actually spent — which on a wait is the call and
/// not the round. The ceiling is small enough here that the resumed run's
/// rounds can be counted, and a round charged to the wait would cost it one.
#[tokio::test]
async fn a_settled_wait_runs_a_second_agent_run_in_the_same_turn_and_message() {
    common::init_tracing();
    let waited = Arc::new(Mutex::new(Uuid::nil()));
    let named = waited.clone();
    let provider = scripted(move |round, _| match round {
        0 => stream(calling(
            "wait-same-turn",
            wait::WAIT_FOR,
            json!({
                "kind": wait::KIND_TASK_RUN,
                "id": *named.lock().expect("the run id is readable"),
                "timeout_secs": 60
            }),
        )),
        round if round <= CHAT_ROUNDS => stream(a_read(round)),
        _ => stream(text("The run I waited for had already finished.")),
    })
    .await;

    let chatting = chatting(provider, CHAT_ROUNDS).await;
    let run = chatting.run_at("completed").await;
    *waited.lock().expect("the run id is writable") = run;

    let mut socket = chatting.connect().await;
    say(&mut socket, "Wait for that run and then look around.").await;
    let frames = turn(&mut socket).await;

    let started = tagged(&frames, "message_start");
    assert_eq!(
        started.len(),
        1,
        "the second run continued the same assistant message, or it started another"
    );
    let message = started[0]["message_id"].clone();
    let opened = tagged(&frames, "wait_started");
    assert_eq!(opened.len(), 1, "one wait was opened: {frames:?}");
    assert_eq!(opened[0]["message_id"], message);
    assert_eq!(opened[0]["tool_call_id"], "wait-same-turn");
    assert_eq!(opened[0]["waiting"]["kind"], wait::KIND_TASK_RUN);
    assert_eq!(opened[0]["waiting"]["id"], run.to_string());

    let settled = tagged(&frames, "wait_settled");
    assert_eq!(settled.len(), 1, "one wait settled: {frames:?}");
    assert_eq!(settled[0]["message_id"], message);
    assert_eq!(settled[0]["settled"]["tool_call_id"], "wait-same-turn");
    assert_eq!(
        settled[0]["settled"]["verdict"], "settled",
        "a run that had already finished is not a timeout"
    );
    let outcome = settled[0]["settled"]["outcome"]
        .as_str()
        .expect("an outcome")
        .to_string();
    assert!(
        outcome.starts_with(&format!("Task run {run} completed after ")),
        "the outcome did not name what settled: {outcome}"
    );

    let rounds = chatting.rounds().await;
    assert_eq!(
        rounds.len(),
        1 + CHAT_ROUNDS + 1,
        "the wait's own round came back, so the resumed run had the whole ceiling: {} rounds",
        rounds.len()
    );
    assert!(
        settled_pair(&rounds[1]).is_some(),
        "the resumed run was not given the outcome it had just published"
    );
}

/// Decision 11: the outcome has a durable home, so the model still has it a
/// turn later.
///
/// A later turn rebuilds its context from canonical entries alone. Without the
/// stored pair it would see a `wait_for` call whose only result is the receipt
/// — it asked, and as far as the transcript goes nothing ever came back.
#[tokio::test]
async fn a_turn_started_after_a_completed_wait_carries_the_outcome() {
    common::init_tracing();
    let waited = Arc::new(Mutex::new(Uuid::nil()));
    let named = waited.clone();
    let provider = scripted(move |round, _| match round {
        0 => stream(calling(
            "wait-later-turn",
            wait::WAIT_FOR,
            json!({
                "kind": wait::KIND_TASK_RUN,
                "id": *named.lock().expect("the run id is readable"),
                "timeout_secs": 60
            }),
        )),
        1 => stream(text("It had already finished.")),
        _ => stream(text("Nothing further.")),
    })
    .await;

    let chatting = chatting(provider, CHAT_ROUNDS).await;
    let run = chatting.run_at("completed").await;
    *waited.lock().expect("the run id is writable") = run;

    let mut socket = chatting.connect().await;
    say(&mut socket, "Wait for that run.").await;
    let first = turn(&mut socket).await;
    let outcome = tagged(&first, "wait_settled")
        .first()
        .and_then(|frame| frame["settled"]["outcome"].as_str())
        .expect("the wait settled")
        .to_string();

    say(&mut socket, "What did it say?").await;
    let _ = turn(&mut socket).await;

    let rounds = chatting.rounds().await;
    assert_eq!(
        rounds.len(),
        3,
        "two rounds of one turn and one of the next"
    );
    let (envelope, result) = settled_pair(&rounds[2])
        .expect("a turn started after a completed wait must carry the outcome");
    let call = envelope["tool_calls"][0]["id"]
        .as_str()
        .expect("the stored call has an id")
        .to_string();
    assert_eq!(call, format!("wait-later-turn{SETTLED}"));
    assert_eq!(
        envelope["tool_calls"][0]["function"]["name"],
        wait::WAIT_FOR,
        "the envelope names the tool whose outcome it carries"
    );
    assert_eq!(
        result["tool_call_id"], call,
        "the result must follow the envelope it answers, or compaction rejects the pair"
    );
    assert_eq!(
        result["content"].as_str(),
        Some(outcome.as_str()),
        "the outcome reached the later turn in other words than it settled in"
    );

    let stored = chatting.stored_pairs().await;
    assert_eq!(stored.len(), 1, "one settled wait was stored: {stored:?}");
    assert_eq!(stored[0].0, call);
    let answering = stored[0]
        .1
        .as_deref()
        .expect("a stored envelope with no result is a half the store rejects");
    assert_eq!(
        chatting.entry(answering).await["content"].as_str(),
        Some(outcome.as_str())
    );
}

/// Decision 4: a turn a disconnected socket left running still finishes.
///
/// Nothing owns the spawned turn, only an explicit cancel reaches it, and the
/// lease is renewed from inside it — so the wait settles, the second run runs,
/// and both halves land in the database with nobody listening. Anything less
/// would lose a reply the user paid for by closing a laptop lid.
#[tokio::test]
async fn a_wait_settles_and_its_second_run_persists_with_no_socket_attached() {
    common::init_tracing();
    const ANSWERED: &str = "The run finished while you were away.";
    let waited = Arc::new(Mutex::new(Uuid::nil()));
    let named = waited.clone();
    let provider = scripted(move |round, _| match round {
        0 => stream(calling(
            "wait-detached",
            wait::WAIT_FOR,
            json!({
                "kind": wait::KIND_TASK_RUN,
                "id": *named.lock().expect("the run id is readable"),
                "timeout_secs": 120
            }),
        )),
        _ => stream(text(ANSWERED)),
    })
    .await;

    let chatting = chatting(provider, CHAT_ROUNDS).await;
    let run = chatting.run_at("running").await;
    *waited.lock().expect("the run id is writable") = run;

    let mut socket = chatting.connect().await;
    say(&mut socket, "Wait for that run.").await;
    let mut opened = None;
    while let Some(frame) = common::next_frame(&mut socket, FRAME).await {
        if frame["type"] == "wait_started" {
            opened = Some(frame);
            break;
        }
    }
    let opened = opened.expect("the turn opened a wait");
    assert_eq!(opened["tool_call_id"], "wait-detached");

    // Not a cancel: only an explicit Cancel frame stops a turn, and a dropped
    // socket is exactly the case the turn has to survive.
    drop(socket);

    tasks::complete_task_run(&chatting.pool, run, "completed", None, None)
        .await
        .expect("the run is finishable")
        .expect("the run was still running");

    let persisted = tokio::time::timeout(SETTLE, async {
        loop {
            let replies = chatting.messages("assistant").await;
            if replies.iter().any(|reply| reply.contains(ANSWERED)) {
                return replies;
            }
            tokio::time::sleep(POLL).await;
        }
    })
    .await;
    let replies = persisted.unwrap_or_else(|_| {
        panic!("the detached turn never persisted its second run's reply");
    });
    assert_eq!(
        replies.len(),
        1,
        "the second run continued one assistant message: {replies:?}"
    );

    let mut socket = chatting.connect().await;
    let stored = chatting.stored_pairs().await;
    assert_eq!(
        stored.len(),
        1,
        "the outcome a detached turn settled was never stored: {stored:?}"
    );
    assert_eq!(stored[0].0, format!("wait-detached{SETTLED}"));
    let answering = stored[0]
        .1
        .as_deref()
        .expect("a stored envelope with no result is a half the store rejects");
    let outcome = chatting.entry(answering).await["content"]
        .as_str()
        .expect("an outcome")
        .to_string();
    assert!(
        outcome.starts_with(&format!("Task run {run} completed after ")),
        "the stored outcome did not name what settled: {outcome}"
    );
    let _ = socket.close(None).await;
}

/// Decision 8, and the workspace assertion that sits on top of it.
///
/// A run is readable to every member of the workspace that owns it, which is
/// not necessarily the caller's own — so membership is not enough, and a run
/// outside the chat's workspace is refused to someone who can read it
/// perfectly well from the other side.
#[tokio::test]
async fn waiting_on_a_task_run_is_refused_on_the_task_surface_and_scoped_to_one_workspace() {
    common::init_tracing();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(common::test_config(), pool.clone());
    let leased = leased_run(&pool).await;
    let root = directory();

    let refused = leased_tools(&state, root.path(), &leased)
        .await
        .execute(wait::WAIT_FOR, &waiting_on_run(leased.run))
        .await;
    assert!(!refused.success, "a task run waited on another task run");
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains(&wait::task_run_wait_unavailable())),
        "the task surface was refused in other words than the frozen ones: {refused:?}"
    );

    let asking = Uuid::new_v4();
    let chat = chat_tools(&state, leased.workspace, leased.user, asking).await;
    let own = prepared_run_in(
        &pool,
        leased.workspace,
        leased.user,
        "In the same workspace",
    )
    .await;
    let opened = chat.execute(wait::WAIT_FOR, &waiting_on_run(own.run)).await;
    assert!(
        opened.success,
        "a chat could not wait on a run of its own workspace: {opened:?}"
    );
    assert!(
        opened.output.as_deref().is_some_and(
            |receipt| receipt.starts_with(&format!("Waiting for task run {} until ", own.run))
        ),
        "the receipt is what execute returns: {opened:?}"
    );

    // The same person, a member of both workspaces, and a run that belongs to
    // the other one.
    let (_organization, elsewhere, stranger) = common::setup_workspace_member(&pool).await;
    zone_server::db::workspace_members::add_member(
        &pool,
        elsewhere,
        leased.user,
        zone_server::db::workspace_members::WorkspaceRole::Member,
        None,
    )
    .await
    .expect("the caller joins the other workspace too");
    let foreign = prepared_run_in(&pool, elsewhere, stranger, "In another workspace").await;
    let refused = chat
        .execute(wait::WAIT_FOR, &waiting_on_run(foreign.run))
        .await;
    assert!(
        !refused.success,
        "a chat waited on a run outside its own workspace: {refused:?}"
    );
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Task run not found in this workspace.")),
        "a foreign run was refused in other words than the frozen ones: {refused:?}"
    );

    wait::reset_session(Session::Task(leased.run));
    wait::reset_session(Session::Chat(asking));
    discard(root).await;
}
