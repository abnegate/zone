//! A chat served by several replicas at once: real `zone-server` processes
//! on one database and one Valkey, each with sockets on the same chat, where
//! only one of them runs the chat's turn.

mod cutoff;

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::{Value, json};
use sqlx::ConnectOptions;
use tokio::time::Instant;
use uuid::Uuid;

use crate::common::Database;
use crate::common::chat_socket::{ChatSocket, Received};
use crate::common::replica::{Account, Environment, Replica};
use crate::common::stub_litellm::{Reply, StubLitellm};

use cutoff::Cutoff;

const PREFIX: &str = "high_availability_chat";
const JOIN: Duration = Duration::from_secs(10);
const TURN: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(2);
const PAUSE: Duration = Duration::from_millis(100);
/// How long a lease outlives its last renewal (`LEASE_LIFETIME`).
const LEASE: Duration = Duration::from_secs(30);
const STOPPED: &str =
    "The server running this reply stopped; any queued messages were dropped. Send again.";
const BUSY: &str = "This chat is busy with a reply on another server. Try again in a moment.";
/// How long a socket may wait for `init` on a replica that is not running the chat.
const INIT: Duration = Duration::from_secs(2);
/// How long a replica waits for the one running a chat to answer a forwarded command
/// (`ANSWER_TIMEOUT`).
const ANSWER: Duration = Duration::from_secs(2);

/// One test's database, LiteLLM stand-in and the environment its replicas share.
struct Deployment {
    database: Database,
    litellm: StubLitellm,
    environment: Environment,
}

impl Deployment {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        let litellm = StubLitellm::start().await;
        let url = database.pool.connect_options().to_url_lossy().to_string();
        let environment = Environment::new(url, litellm.url());
        Self {
            database,
            litellm,
            environment,
        }
    }

    async fn replica(&self) -> Replica {
        Replica::start(&self.environment).await
    }

    /// Stops every replica, waits for their sessions to leave the database,
    /// and drops it.
    async fn retire(self, replicas: Vec<Replica>) {
        for mut replica in replicas {
            replica.kill().await;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let connected: i64 =
                sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname = $1")
                    .bind(&self.database.name)
                    .fetch_one(&self.database.admin)
                    .await
                    .expect("the sessions can be counted");
            if connected == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.database.cleanup().await;
    }
}

fn kinds(frames: &[Received]) -> Vec<&str> {
    frames.iter().map(Received::kind).collect()
}

fn chunks(frames: &[Received]) -> String {
    frames
        .iter()
        .filter(|frame| frame.kind() == "chunk")
        .map(|frame| frame.frame["content"].as_str().unwrap_or_default())
        .collect()
}

fn texts(frames: &[Received]) -> Vec<&str> {
    frames.iter().map(|frame| frame.text.as_str()).collect()
}

fn errors(frames: &[Received]) -> Vec<&Value> {
    frames
        .iter()
        .filter(|frame| frame.kind() == "error")
        .map(|frame| &frame.frame)
        .collect()
}

/// Reads until `count` chunks have arrived.
async fn chunked(socket: &mut ChatSocket, count: usize) -> Vec<Received> {
    let mut received = Vec::new();
    while received
        .iter()
        .filter(|frame: &&Received| frame.kind() == "chunk")
        .count()
        < count
    {
        received.extend(socket.until("chunk", TURN).await);
    }
    received
}

/// Reads until the user message `content` is saved.
async fn saved(socket: &mut ChatSocket, content: &str) -> Vec<Received> {
    let mut received = Vec::new();
    loop {
        let frames = socket.until("message_saved", TURN).await;
        let found = frames
            .last()
            .is_some_and(|frame| frame.frame["content"] == content);
        received.extend(frames);
        if found {
            return received;
        }
    }
}

/// Reads until the message_end of the reply whose content is `content`.
async fn ended_with(socket: &mut ChatSocket, content: &str) -> Vec<Received> {
    let mut received = Vec::new();
    loop {
        let frames = socket.until("message_end", TURN).await;
        let ended = frames
            .last()
            .is_some_and(|frame| frame.frame["content"] == content);
        received.extend(frames);
        if ended {
            return received;
        }
    }
}

/// Reads until a queue frame `matches` accepts, and returns that frame.
async fn queue_where(socket: &mut ChatSocket, matches: impl Fn(&Value) -> bool) -> Value {
    loop {
        let frames = socket.until("queue", TURN).await;
        let queue = &frames.last().expect("a queue frame").frame;
        if matches(queue) {
            return queue.clone();
        }
    }
}

fn queued_ids(queue: &Value) -> Vec<String> {
    queue["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .map(|message| message["id"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Reads until a user message with each of `contents` is saved, then until
/// the reply to the last of them ends.
async fn ran(socket: &mut ChatSocket, contents: &[&str]) -> Vec<Received> {
    let mut pending = contents.to_vec();
    let mut received = Vec::new();
    while !pending.is_empty() {
        let frames = socket.until("message_saved", TURN).await;
        let content = frames
            .last()
            .and_then(|frame| frame.frame["content"].as_str())
            .unwrap_or_default()
            .to_string();
        pending.retain(|wanted| *wanted != content);
        received.extend(frames);
    }
    received.extend(socket.until("message_end", TURN).await);
    received
}

async fn valkey() -> SocketAddr {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let url = reqwest::Url::parse(&url).expect("REDIS_URL is a URL");
    let host = url.host_str().expect("REDIS_URL names a host");
    let port = url.port().unwrap_or(6379);
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .expect("Valkey's host resolves")
        .collect();
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or(addresses.first())
        .copied()
        .expect("Valkey has an address")
}

#[tokio::test]
async fn a_turn_on_one_replica_reaches_a_socket_on_another_byte_for_byte() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(10, PAUSE);
    deployment.litellm.queue(reply.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;

    owner.say(Uuid::new_v4(), "Hello").await;
    let owned = owner.until("message_end", TURN).await;
    let mirrored = mirror.until("message_end", TURN).await;

    assert_eq!(chunks(&owned), reply.text());
    assert_eq!(
        texts(&mirrored),
        texts(&owned),
        "the other replica's socket sees every frame of the turn, unaltered"
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn a_socket_joining_another_replica_mid_turn_resumes_without_a_gap_or_a_duplicate() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(30, PAUSE);
    deployment.litellm.queue(reply.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Hello").await;
    chunked(&mut owner, 5).await;

    let mut joining = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    let resumed = joining.until("message_end", TURN).await;

    assert_eq!(resumed[0].kind(), "message_start", "{:?}", kinds(&resumed));
    assert_eq!(resumed[0].frame["resumed"], true);
    let end = resumed.last().expect("the reply ends");
    assert_eq!(end.frame["content"], reply.text());
    assert_eq!(
        chunks(&resumed),
        reply.text(),
        "the replay and the frames after it add up to the reply exactly once"
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn cancel_sent_to_another_replica_stops_the_turn_where_it_runs() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(200, PAUSE);
    deployment.litellm.queue(reply.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Hello").await;
    chunked(&mut mirror, 3).await;

    let cancelled = Instant::now();
    mirror.send(json!({"type": "cancel"})).await;
    let stopped = owner.until("cancelled", reply.duration() / 2).await;

    assert!(
        cancelled.elapsed() < reply.duration() / 2,
        "the turn stopped {:?} after the cancel",
        cancelled.elapsed()
    );
    assert!(
        !kinds(&stopped).contains(&"message_end"),
        "{:?}",
        kinds(&stopped)
    );
    mirror.until("cancelled", TURN).await;
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn a_send_on_another_replica_during_a_turn_is_queued_where_it_runs_and_runs_next() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(30, PAUSE);
    let next = Reply::new(["Second answer"], Duration::ZERO);
    deployment.litellm.queue(reply.clone());
    deployment.litellm.queue(next.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "First question").await;
    chunked(&mut mirror, 2).await;

    let queued = Uuid::new_v4();
    mirror.say(queued, "Second question").await;

    for socket in [&mut owner, &mut mirror] {
        let frames = socket.until("queue", TURN).await;
        let queue = &frames.last().expect("a queue frame").frame;
        assert_eq!(queue["messages"][0]["id"], queued.to_string(), "{queue}");
        assert_eq!(queue["messages"][0]["content"], "Second question");
        assert!(errors(&frames).is_empty(), "{:?}", errors(&frames));
    }
    for socket in [&mut owner, &mut mirror] {
        let frames = ended_with(socket, &next.text()).await;
        assert!(errors(&frames).is_empty(), "{:?}", errors(&frames));
        assert!(
            frames.iter().any(|frame| frame.kind() == "message_saved"
                && frame.frame["content"] == "Second question"),
            "the queued message ran as its own turn: {:?}",
            kinds(&frames)
        );
    }
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn killing_the_replica_mid_turn_tells_the_other_replicas_sockets_and_frees_the_chat() {
    let deployment = Deployment::new().await;
    let mut first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    deployment.litellm.queue(Reply::counted(1000, PAUSE));
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Hello").await;
    chunked(&mut mirror, 3).await;

    let killed = Instant::now();
    first.kill().await;
    let frames = mirror.until("error", LEASE + Duration::from_secs(10)).await;

    assert!(
        killed.elapsed() <= LEASE + Duration::from_secs(10),
        "the error came {:?} after the kill",
        killed.elapsed()
    );
    assert_eq!(frames.last().expect("the error").frame["message"], STOPPED);
    mirror.say(Uuid::new_v4(), "Are you there?").await;
    let answered = mirror.until("message_end", TURN).await;
    assert_eq!(
        answered.last().expect("the reply").frame["content"],
        "Hello"
    );
    assert!(errors(&answered).is_empty(), "{:?}", errors(&answered));
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn a_watched_turn_that_finished_unseen_closes_from_what_was_saved_without_an_error() {
    let deployment = Deployment::new().await;
    let cutoff = Cutoff::start(valkey().await).await;
    let first = deployment.replica().await;
    let second = Replica::start(&deployment.environment.clone().with_redis(cutoff.url())).await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(60, Duration::from_millis(150));
    deployment.litellm.queue(reply.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Hello").await;
    let mut seen = chunked(&mut mirror, 2).await;

    cutoff.cut();
    let owned = owner.until("message_end", TURN).await;
    cutoff.restore();
    let closed = mirror.until("message_end", TURN).await;
    seen.extend(closed.iter().cloned());

    assert!(
        seen.iter().filter(|frame| frame.kind() == "chunk").count() < reply.chunks.len(),
        "the cut kept the end of the turn from reaching the replica"
    );
    assert!(errors(&closed).is_empty(), "{:?}", errors(&closed));
    let end = closed.last().expect("the closing frame");
    assert_eq!(end.frame["content"], reply.text());
    assert_eq!(
        end.frame["message_id"],
        owned.last().expect("the owner's end").frame["message_id"]
    );
    assert!(
        errors(&mirror.during(QUIET).await).is_empty(),
        "nothing reports the finished turn as failed afterwards"
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn a_send_as_a_turn_ends_still_runs_and_answers_reach_only_the_socket_that_asked() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let third = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let opening = Reply::counted(20, PAUSE);
    let following = Reply::counted(80, PAUSE);
    let last = Reply::new(["Third answer"], Duration::ZERO);
    for reply in [&opening, &following, &last] {
        deployment.litellm.queue(reply.clone());
    }
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut asker = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    let mut beside = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    let mut distant = ChatSocket::joined(&third, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "First question").await;
    chunked(&mut asker, 2).await;

    asker
        .send(json!({"type": "approve_tool", "tool_call_id": "call-unknown", "approved": true}))
        .await;
    let closed = asker.until("tool_approval_closed", TURN).await;
    assert_eq!(
        closed.last().expect("the closing frame").frame["tool_call_id"],
        "call-unknown"
    );
    asker.until("message_end", TURN).await;
    asker.say(Uuid::new_v4(), "Second question").await;

    let opened = owner.until("message_end", TURN).await;
    let alongside = beside.until("message_end", TURN).await;
    for frames in [&opened, &alongside] {
        assert!(
            !kinds(frames).contains(&"tool_approval_closed"),
            "only the asking socket hears about its own decision: {:?}",
            kinds(frames)
        );
    }
    saved(&mut distant, "Second question").await;
    let resent = Uuid::new_v4();
    distant.say(resent, "Third question").await;
    distant.say(resent, "Third question").await;

    for socket in [&mut owner, &mut asker] {
        ended_with(socket, &following.text()).await;
    }
    let mut finished = Vec::new();
    for socket in [&mut owner, &mut asker, &mut distant] {
        let mut frames = ended_with(socket, &last.text()).await;
        frames.extend(socket.during(QUIET).await);
        finished.push(frames);
    }
    for frames in &finished {
        assert!(
            errors(frames).is_empty(),
            "a replica that holds nothing never answers for the chat: {:?}",
            errors(frames)
        );
        let saved = frames
            .iter()
            .filter(|frame| {
                frame.kind() == "message_saved" && frame.frame["content"] == "Third question"
            })
            .count();
        assert_eq!(saved, 1, "the resent message ran once: {:?}", kinds(frames));
    }
    let messages = account.messages(&first, chat).await;
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["content"] == "Third question")
            .count(),
        1
    );
    deployment.retire(vec![first, second, third]).await;
}

#[tokio::test]
async fn a_socket_joining_an_idle_chat_on_another_replica_gets_init_at_once() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Hello").await;
    owner.until("message_end", TURN).await;

    let mut joining = ChatSocket::connect(&second, chat, &account.token).await;
    let asked = Instant::now();
    let init = joining
        .next(INIT)
        .await
        .unwrap_or_else(|| panic!("init arrives within {INIT:?}"));

    assert_eq!(init.kind(), "init");
    assert!(asked.elapsed() <= INIT);
    assert!(
        joining.during(QUIET).await.is_empty(),
        "an idle chat has nothing to replay"
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn a_send_the_owner_answers_late_is_never_reported_busy_and_runs_once() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(80, PAUSE);
    let next = Reply::new(["Second answer"], Duration::ZERO);
    deployment.litellm.queue(reply.clone());
    deployment.litellm.queue(next.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "First question").await;
    chunked(&mut mirror, 2).await;

    let mut stall = deployment
        .database
        .pool
        .begin()
        .await
        .expect("a transaction");
    sqlx::query("LOCK TABLE workspace_members IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *stall)
        .await
        .expect("the owner's access check is stalled");
    mirror.say(Uuid::new_v4(), "Second question").await;
    tokio::time::sleep(ANSWER + Duration::from_millis(400)).await;
    stall.rollback().await.expect("the stall ends");

    let mut frames = ended_with(&mut mirror, &next.text()).await;
    frames.extend(mirror.during(QUIET).await);

    let busy: Vec<&Value> = errors(&frames)
        .into_iter()
        .filter(|error| error["message"] == BUSY)
        .collect();
    assert!(
        busy.is_empty(),
        "a send the owner went on to run was reported busy: {:?}",
        kinds(&frames)
    );
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.kind() == "message_saved"
                && frame.frame["content"] == "Second question")
            .count(),
        1,
        "the late send ran once: {:?}",
        kinds(&frames)
    );
    let messages = account.messages(&first, chat).await;
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["content"] == "Second question")
            .count(),
        1
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn a_queue_held_on_one_replica_shows_on_another_and_runs_from_a_send_there() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    deployment.litellm.queue(Reply::counted(200, PAUSE));
    deployment
        .litellm
        .fall_back_to(Reply::new(["Later answer"], Duration::ZERO));
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Opening question").await;
    chunked(&mut owner, 2).await;
    let held = [Uuid::new_v4(), Uuid::new_v4()];
    owner.say(held[0], "Held one").await;
    owner.say(held[1], "Held two").await;
    queue_where(&mut owner, |queue| queued_ids(queue).len() == 2).await;
    owner.send(json!({"type": "cancel"})).await;
    queue_where(&mut owner, |queue| queue["held"] == true).await;
    let expected: Vec<String> = held.iter().map(Uuid::to_string).collect();

    let mut joining = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    let shown = joining.until("queue", JOIN).await;
    let queue = &shown.last().expect("the held queue").frame;
    assert_eq!(queued_ids(queue), expected, "{queue}");
    assert_eq!(queue["held"], true, "{queue}");

    joining.say(Uuid::new_v4(), "Fresh question").await;
    let contents = ["Held one", "Held two", "Fresh question"];
    let mut frames = ran(&mut joining, &contents).await;
    frames.extend(joining.during(QUIET).await);

    assert!(errors(&frames).is_empty(), "{:?}", errors(&frames));
    let messages = account.messages(&first, chat).await;
    for content in contents {
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["content"] == content)
                .count(),
            1,
            "{content} ran once: {messages:?}"
        );
    }
    deployment.retire(vec![first, second]).await;
}
