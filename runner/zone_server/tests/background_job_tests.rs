//! Background jobs end to end: who may read one, what a spawn writes into the
//! checkout it runs in, and who kills the child when the session that started
//! the job ends.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::SinkExt;
use serde_json::json;
use sqlx::PgPool;
use tempfile::TempDir;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_core::tools::Session;
use zone_core::tools::job::{JobCommand, Jobs, TAIL_JOB, UNAVAILABLE};
use zone_server::agent::{ChatTools, WorkspaceScope};
use zone_server::db::tasks;
use zone_server::state::AppState;

/// The line a task-run spawn keeps in its checkout's exclude file.
const EXCLUDED: &str = ".zone/";

/// Longest a test waits for a child to be reaped or a job to settle.
const SETTLE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(20);

/// How long an open of a FIFO is given before a blocked one counts as proof
/// that nothing is reading the other end any more.
const READER_PROBE: Duration = Duration::from_millis(500);

/// Longest a socket frame is waited for.
const FRAME: Duration = Duration::from_secs(20);

/// The model a chat turn resolves to against the stubbed provider.
const CHAT_MODEL: &str = "llama3.2:3b";

/// The model a task run resolves to. Named explicitly because an empty catalog
/// resolves anything unpinned to `auto`, which fails the run before it starts.
const TASK_MODEL: &str = "gpt-4";

/// The only process variable a job needs: without `PATH` a cleared environment
/// cannot resolve the program at all.
fn environment() -> HashMap<String, String> {
    std::env::var("PATH")
        .map(|path| HashMap::from([("PATH".to_string(), path)]))
        .unwrap_or_default()
}

/// Where a chat tool set's jobs would land, resolved the way the tool set
/// resolves it. Nothing in these tests spawns a chat job there, and the
/// assertions at the end of each test are what keep it that way.
fn chat_root() -> PathBuf {
    std::env::var_os("ZONE_CHAT_AGENT_CWD")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn directory() -> TempDir {
    TempDir::new().expect("a temporary working directory")
}

/// A FIFO nothing is writing to yet, so `cat` on it blocks until a test says
/// otherwise. This is what gives a job a lifetime the test controls without a
/// sleep to race against.
fn gate(root: &Path) -> PathBuf {
    let path = root.join("gate");
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("mkfifo is available");
    assert!(made.success(), "could not create a FIFO at {path:?}");
    path
}

fn blocking_on(gate: &Path) -> JobCommand {
    JobCommand::new("cat", vec![gate.to_string_lossy().into_owned()])
}

/// Whether nothing is reading `gate` any more.
///
/// Opening a FIFO for writing blocks until a reader holds the other end, so an
/// open that cannot complete is a reader that has gone. It proves a child is
/// dead without reaching for the process table, and the thread it leaves
/// blocked is detached so no runtime shutdown waits on it.
async fn reader_is_gone(gate: &Path) -> bool {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let path = gate.to_path_buf();
    std::thread::spawn(move || {
        let _ = sender.send(std::fs::OpenOptions::new().write(true).open(path).is_ok());
    });
    tokio::time::timeout(READER_PROBE, receiver).await.is_err()
}

/// Wait until the registry no longer holds `job` for `session`.
async fn released(session: Session, job: &str) {
    tokio::time::timeout(SETTLE, async {
        while Jobs::read(session, job, 0, 1).await.is_ok() {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{job} was still registered to {session:?}"));
}

/// A task and a run of it the worker can still claim for itself.
struct Prepared {
    task: Uuid,
    run: Uuid,
    workspace: Uuid,
    user: Uuid,
}

async fn prepared_run(pool: &PgPool) -> Prepared {
    let (_organization, workspace, user) = common::setup_workspace_member(pool).await;
    let task = tasks::create_task_as(
        pool,
        workspace,
        &[],
        "Backgrounds a job",
        "Start something long and read its log",
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

/// A claimed run, because a leased tool set refuses every call on a run it
/// does not own.
struct Leased {
    run: Uuid,
    owner: Uuid,
    workspace: Uuid,
    user: Uuid,
}

async fn leased_run(pool: &PgPool) -> Leased {
    let prepared = prepared_run(pool).await;
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

/// The tool set a task run's own turn is given.
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

/// A task tool set with no lease, which is what the five functional-update
/// call sites silently acquire.
async fn detached_tools(state: &AppState, cwd: &Path, workspace: Uuid) -> ChatTools {
    ChatTools::for_task(state, cwd.to_path_buf(), workspace, None).await
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

fn tail(job: &str) -> String {
    json!({"id": job}).to_string()
}

/// The frozen refusal a job id from another session comes back with.
fn missing(job: &str) -> String {
    format!("No job {job} in this session.")
}

/// A git checkout of its own, which is what a task run works in.
fn repository() -> TempDir {
    let root = directory();
    let initialized = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(root.path())
        .status()
        .expect("git is available");
    assert!(initialized.success(), "could not initialize a checkout");
    root
}

/// Where git says this checkout's exclude file lives. Asked rather than
/// derived, because the spawn asks the same question and a test that joined
/// `.git/info/exclude` itself would not notice if the two disagreed.
fn exclude_path(root: &Path) -> PathBuf {
    let resolved = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--git-path")
        .arg("info/exclude")
        .current_dir(root)
        .output()
        .expect("git is available");
    assert!(resolved.status.success(), "git could not resolve the path");
    root.join(
        String::from_utf8(resolved.stdout)
            .expect("a path in UTF-8")
            .trim(),
    )
}

async fn excluded_lines(exclude: &Path) -> usize {
    tokio::fs::read_to_string(exclude)
        .await
        .unwrap_or_default()
        .lines()
        .filter(|line| line.trim() == EXCLUDED)
        .count()
}

/// Decision 6's tool-path isolation, from every session that is not the one
/// that started the job.
///
/// Isolation here is advisory and the contract says so: every session runs as
/// the same operating-system user, and a task run's allow-listed `cat` can
/// still read the log by absolute path. What is enforced is the tool path —
/// `tail_job` is keyed by job id and checks the session — and that is what
/// this pins.
#[tokio::test]
async fn a_job_is_readable_through_tail_job_only_from_the_session_that_started_it() {
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
    tokio::time::timeout(SETTLE, async {
        while !Jobs::read(Session::Task(owning.run), &started.id, 0, 1)
            .await
            .is_ok_and(|tail| tail.state.settled())
        {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .expect("the job never finished");

    let reading = leased_tools(&state, root.path(), &owning).await;
    let read = reading.execute(TAIL_JOB, &tail(&started.id)).await;
    assert!(
        read.success,
        "the owning run cannot read its own job: {read:?}"
    );
    let slice = read.output.unwrap_or_default();
    assert!(
        slice.contains("ledger") && slice.contains("[job exited 0; next="),
        "the slice and the state line are what tail_job returns: {slice}"
    );

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
        let refused = tools.execute(TAIL_JOB, &tail(&started.id)).await;
        assert!(!refused.success, "{surface} read a job it did not start");
        assert!(
            refused
                .error
                .as_deref()
                .is_some_and(|error| error.contains(&missing(&started.id))),
            "{surface} was refused in other words than the frozen ones: {refused:?}"
        );
    }

    let detached = detached_tools(&state, root.path(), owning.workspace).await;
    let refused = detached.execute(TAIL_JOB, &tail(&started.id)).await;
    assert!(!refused.success, "a detached context read a job log");
    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains(UNAVAILABLE)),
        "a context with no session to key a job to is told exactly that: {refused:?}"
    );

    assert_eq!(
        Jobs::kill_session(Session::Task(owning.run)).await,
        1,
        "the run's own job is the only one it owns"
    );
    let log = PathBuf::from(started.log_path);
    assert!(
        log.starts_with(root.path()),
        "a job log belongs inside the working directory the session gave it: {log:?}"
    );
    let left = root.keep();
    tokio::fs::remove_dir_all(&left)
        .await
        .expect("the working directory is removable");
    assert!(!log.exists(), "a job log outlived its test");
    assert!(
        !chat_root().join(EXCLUDED).exists(),
        "a job wrote its log into the server's own working directory"
    );
}

/// A chat works in a checkout it does not own — in a linked worktree its
/// `.git` is a pointer into a directory every worktree shares — so one chat's
/// job must not write into all of them. A task run's checkout is its own, and
/// keeping `.zone/` out of its diff is worth one idempotent line.
#[tokio::test]
async fn a_chat_spawn_leaves_the_exclude_file_alone_and_a_task_spawn_writes_it_once() {
    let root = repository();
    let exclude = exclude_path(root.path());
    let before = tokio::fs::read_to_string(&exclude)
        .await
        .unwrap_or_default();
    assert_eq!(
        excluded_lines(&exclude).await,
        0,
        "a fresh checkout excludes nothing of ours yet"
    );

    let chat = Uuid::new_v4();
    let by_chat = Jobs::spawn(
        Session::Chat(chat),
        &JobCommand::new("echo", vec!["chat".to_string()]),
        root.path(),
        &environment(),
    )
    .await
    .expect("a chat may start a job");
    assert_eq!(
        tokio::fs::read_to_string(&exclude)
            .await
            .unwrap_or_default(),
        before,
        "a chat job wrote into an exclude file its session does not own"
    );

    let run = Uuid::new_v4();
    let first = Jobs::spawn(
        Session::Task(run),
        &JobCommand::new("echo", vec!["task".to_string()]),
        root.path(),
        &environment(),
    )
    .await
    .expect("a run may start a job");
    assert_eq!(
        excluded_lines(&exclude).await,
        1,
        "a task run's checkout keeps its own job logs out of its diff"
    );
    let second = Jobs::spawn(
        Session::Task(run),
        &JobCommand::new("echo", vec!["task again".to_string()]),
        root.path(),
        &environment(),
    )
    .await
    .expect("a second job in the same checkout");
    assert_eq!(
        excluded_lines(&exclude).await,
        1,
        "the write is append-idempotent, or every job adds a line"
    );

    Jobs::kill_session(Session::Chat(chat)).await;
    Jobs::kill_session(Session::Task(run)).await;
    let logs = [by_chat.log_path, first.log_path, second.log_path].map(PathBuf::from);
    for log in &logs {
        assert!(
            log.starts_with(root.path()),
            "a job log escaped the checkout it was started in: {log:?}"
        );
    }
    let left = root.keep();
    tokio::fs::remove_dir_all(&left)
        .await
        .expect("the checkout is removable");
    for log in &logs {
        assert!(!log.exists(), "a job log outlived its test: {log:?}");
    }
    assert!(
        !chat_root().join(EXCLUDED).exists(),
        "a job wrote its log into the server's own working directory"
    );
}

/// A background job outlives the tool call that started it and nothing drops a
/// detached child, so the run's last act is to kill what it left running. A
/// task run that ended while its build kept burning the host would be a leak
/// with no owner left to notice it.
#[tokio::test]
async fn a_job_a_run_started_is_dead_once_the_run_reaches_a_terminal_status() {
    common::init_tracing();
    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_string(completion("Nothing left to do.")),
        )
        .mount(&provider)
        .await;
    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();

    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config, pool.clone());
    let prepared = prepared_run(&pool).await;
    let root = directory();
    let gate = gate(root.path());

    let started = Jobs::spawn(
        Session::Task(prepared.run),
        &blocking_on(&gate),
        root.path(),
        &environment(),
    )
    .await
    .expect("a job keyed to the run");
    assert!(
        !reader_is_gone(&gate).await,
        "the job is not reading its gate, so nothing later proves it was killed"
    );

    zone_server::workers::task::execute_task_run(&state, prepared.run, prepared.task).await;

    let status: String = sqlx::query_scalar("SELECT status FROM task_runs WHERE id = $1")
        .bind(prepared.run)
        .fetch_one(&pool)
        .await
        .expect("the run is still readable");
    assert_eq!(
        status, "completed",
        "the run never reached a terminal status"
    );

    released(Session::Task(prepared.run), &started.id).await;
    assert!(
        reader_is_gone(&gate).await,
        "the child the run left running is still reading its gate"
    );
    let log = PathBuf::from(started.log_path);
    let left = root.keep();
    tokio::fs::remove_dir_all(&left)
        .await
        .expect("the working directory is removable");
    assert!(!log.exists(), "a job log outlived its run");
    assert!(
        !chat_root().join(EXCLUDED).exists(),
        "a job wrote its log into the server's own working directory"
    );
}

/// The same debt on the other surface, settled at the one exit every chat turn
/// takes — the disconnected one included.
///
/// The job is registered to the chat directly rather than spawned by the turn:
/// a chat's own background call needs an approval round trip and would put its
/// log in the server's working directory, and neither is what this is about.
/// What the turn has to do is kill whatever its session still owns.
#[tokio::test]
async fn a_job_a_chat_turn_started_is_dead_once_the_turn_ends() {
    common::init_tracing();
    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({"stream": true})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(completion("Started it.")),
        )
        .mount(&provider)
        .await;
    let mut config = common::test_config();
    config.litellm_host = provider.uri();
    config.ollama_host = provider.uri();

    let pool = common::create_test_pool().await;
    let client = common::TestClient::new(common::create_test_router(common::create_test_state(
        config.clone(),
        pool.clone(),
    )));
    let address = common::serve(common::create_test_state(config, pool)).await;
    let (token, chat, _workspace) = common::seed_chat(&client, CHAT_MODEL).await;
    let chat_id = Uuid::parse_str(&chat).expect("a chat id");

    let root = directory();
    let gate = gate(root.path());
    let started = Jobs::spawn(
        Session::Chat(chat_id),
        &blocking_on(&gate),
        root.path(),
        &environment(),
    )
    .await
    .expect("a job keyed to the chat");
    assert!(
        !reader_is_gone(&gate).await,
        "the job is not reading its gate, so nothing later proves it was killed"
    );

    let (mut socket, _) = connect_async(format!("ws://{address}/ws/chats/{chat}"))
        .await
        .expect("the chat socket accepts a connection");
    socket
        .send(WebSocketMessage::Text(
            json!({"type":"auth","token":token}).to_string().into(),
        ))
        .await
        .expect("the socket takes an auth frame");
    assert_eq!(
        common::next_frame(&mut socket, FRAME).await.expect("init")["type"],
        "init"
    );
    socket
        .send(WebSocketMessage::Text(
            json!({"type":"send","content":"Start the build."})
                .to_string()
                .into(),
        ))
        .await
        .expect("the socket takes a send frame");
    let mut tags = Vec::new();
    while let Some(frame) = common::next_frame(&mut socket, FRAME).await {
        let tag = frame["type"].as_str().unwrap_or_default().to_string();
        let end = tag == "message_end";
        tags.push(tag);
        if end {
            break;
        }
    }
    assert!(
        tags.iter().any(|tag| tag == "message_end"),
        "the turn never ended: {tags:?}"
    );

    released(Session::Chat(chat_id), &started.id).await;
    assert!(
        reader_is_gone(&gate).await,
        "the child the turn left running is still reading its gate"
    );
    let log = PathBuf::from(started.log_path);
    let left = root.keep();
    tokio::fs::remove_dir_all(&left)
        .await
        .expect("the working directory is removable");
    assert!(!log.exists(), "a job log outlived its turn");
    assert!(
        !chat_root().join(EXCLUDED).exists(),
        "a job wrote its log into the server's own working directory"
    );
}

/// One streamed completion carrying `content` and nothing else.
fn completion(content: &str) -> String {
    let chunk = json!({
        "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
        "choices": [{"index": 0, "delta": {"content": content}, "finish_reason": null}]
    });
    let end = json!({
        "id": "completion", "object": "chat.completion.chunk", "created": 0, "model": "test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    });
    format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n")
}
