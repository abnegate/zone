//! How a real `zone-server` process stops on SIGTERM: it fails readiness while
//! it keeps serving, finishes what is in flight, stops the chat turns it runs
//! so another replica can take their chats at once, and tells every socket to
//! reconnect elsewhere.

use std::time::Duration;

use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::ConnectOptions;
use sqlx::postgres::PgConnectOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use uuid::Uuid;

use crate::common::Database;
use crate::common::blackhole::Blackhole;
use crate::common::chat_socket::{ChatSocket, Received};
use crate::common::replica::{Account, Environment, Replica};
use crate::common::stub_litellm::{Reply, StubLitellm};

const PREFIX: &str = "high_availability_shutdown";
const READY: &str = "/health/ready";
const HEALTH: &str = "/health";
const DELAY: &str = "ZONE_SHUTDOWN_DELAY_SECONDS";
const GRACE: &str = "ZONE_SHUTDOWN_GRACE_SECONDS";
const JOIN: Duration = Duration::from_secs(10);
const TURN: Duration = Duration::from_secs(60);
const PAUSE: Duration = Duration::from_millis(100);
const NOTICED: Duration = Duration::from_secs(2);
/// How long a lease outlives its last renewal (`LEASE_LIFETIME`).
const LEASE: Duration = Duration::from_secs(30);
/// How long a replica on default settings may take to stop.
const PROMPT: Duration = Duration::from_secs(10);
const TERMINATORS: [&str; 2] = ["cancelled", "message_end"];
/// How often a replica looks for reminders that are due.
const REMINDER_TICK: Duration = Duration::from_secs(10);
/// What the LiteLLM stand-in answers once its queued replies run out.
const ANSWER: &str = "Hello";

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
        let environment = Environment::new(url(&database.pool.connect_options()), litellm.url());
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

fn url(options: &PgConnectOptions) -> String {
    options.to_url_lossy().to_string()
}

async fn status(replica: &Replica, path: &str) -> (StatusCode, Value) {
    tokio::time::timeout(NOTICED, replica.get(path, None))
        .await
        .unwrap_or_else(|_| panic!("{path} answers within {NOTICED:?}"))
}

/// Polls readiness until it reports `expected`.
async fn readiness(replica: &Replica, expected: StatusCode) -> Value {
    let deadline = Instant::now() + NOTICED;
    loop {
        let (code, body) = status(replica, READY).await;
        if code == expected {
            return body;
        }
        assert!(
            Instant::now() < deadline,
            "readiness stayed {code} {body} instead of {expected}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn kinds(frames: &[Received]) -> Vec<&str> {
    frames.iter().map(Received::kind).collect()
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

/// Every frame until the socket closes, failing if it stays open past `within`.
async fn until_closed(socket: &mut ChatSocket, within: Duration) -> Vec<Received> {
    let deadline = Instant::now() + within;
    let mut received = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(
            !left.is_zero(),
            "the socket stayed open: {:?}",
            kinds(&received)
        );
        match socket.next(left).await {
            Some(frame) => received.push(frame),
            None if socket.closed().is_some() => return received,
            None => panic!(
                "the socket ended without a close frame: {:?}",
                kinds(&received)
            ),
        }
    }
}

/// Every frame up to the one that ends the turn the socket is watching.
async fn until_ended(socket: &mut ChatSocket, within: Duration) -> Vec<Received> {
    let deadline = Instant::now() + within;
    let mut received = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let frame = socket
            .next(left)
            .await
            .unwrap_or_else(|| panic!("the turn never ended: {:?}", kinds(&received)));
        let ended = TERMINATORS.contains(&frame.kind());
        received.push(frame);
        if ended {
            return received;
        }
    }
}

/// A registration whose request is on the wire, headers and half its body,
/// so the server is in the middle of reading it.
struct Registration {
    connection: TcpStream,
    rest: Vec<u8>,
}

impl Registration {
    async fn begin(replica: &Replica) -> Self {
        let body = json!({
            "email": crate::common::test_email(),
            "password": crate::common::test_password(),
        })
        .to_string()
        .into_bytes();
        let head = format!(
            "POST /api/auth/register HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            replica.port(),
            body.len()
        );
        let mut connection = TcpStream::connect(("127.0.0.1", replica.port()))
            .await
            .expect("the replica accepts a connection");
        let (first, rest) = body.split_at(body.len() / 2);
        connection
            .write_all(head.as_bytes())
            .await
            .expect("the request head is sent");
        connection
            .write_all(first)
            .await
            .expect("half the body is sent");
        Self {
            connection,
            rest: rest.to_vec(),
        }
    }

    /// Sends the rest of the body and reads the whole response.
    async fn finish(mut self) -> String {
        self.connection
            .write_all(&self.rest)
            .await
            .expect("the rest of the body is sent");
        let mut response = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            self.connection.read_to_end(&mut response),
        )
        .await
        .expect("the response arrives")
        .expect("the response is read");
        String::from_utf8_lossy(&response).into_owned()
    }
}

#[tokio::test]
async fn a_draining_replica_fails_readiness_finishes_what_it_serves_and_exits_within_its_grace() {
    let delay = Duration::from_secs(2);
    let grace = Duration::from_secs(5);
    let deployment = Deployment::new().await;
    let environment = deployment
        .environment
        .clone()
        .with_variable(DELAY, delay.as_secs().to_string())
        .with_variable(GRACE, grace.as_secs().to_string());
    let mut replica = Replica::start(&environment).await;
    assert_eq!(
        readiness(&replica, StatusCode::OK).await,
        json!({"status": "ready"})
    );
    let registration = Registration::begin(&replica).await;

    let terminated = Instant::now();
    replica.terminate();

    assert_eq!(
        readiness(&replica, StatusCode::SERVICE_UNAVAILABLE).await,
        json!({"status": "draining"})
    );
    let (health, body) = status(&replica, HEALTH).await;
    assert_eq!(health, StatusCode::OK, "{body}");
    tokio::time::sleep_until(terminated + delay + Duration::from_secs(1)).await;
    let response = registration.finish().await;
    assert!(
        response.starts_with("HTTP/1.1 201") && response.contains("access_token"),
        "the request in flight when listening stopped still completes: {response}"
    );
    let exited = replica
        .exited(delay + grace + Duration::from_secs(2))
        .await
        .unwrap_or_else(|| {
            panic!(
                "the replica was still running {:?} after SIGTERM",
                terminated.elapsed()
            )
        });
    assert!(exited.success(), "{exited}");
    deployment.retire(vec![replica]).await;
}

#[tokio::test]
async fn a_terminated_replica_stops_its_turn_closes_its_sockets_and_frees_the_chat_at_once() {
    let deployment = Deployment::new().await;
    let mut first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let reply = Reply::counted(300, PAUSE);
    deployment.litellm.queue(reply.clone());
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    owner.say(Uuid::new_v4(), "Hello").await;
    chunked(&mut mirror, 3).await;

    let terminated = Instant::now();
    first.terminate();

    let closing = until_closed(&mut owner, PROMPT).await;
    let close = owner.closed().expect("a close frame");
    assert_eq!(close.code, CloseCode::Restart, "{close:?}");
    assert!(
        kinds(&closing).contains(&"cancelled"),
        "the owner's socket hears the turn stop before it closes: {:?}",
        kinds(&closing)
    );
    let ended = until_ended(&mut mirror, PROMPT).await;
    assert!(errors(&ended).is_empty(), "{:?}", errors(&ended));
    let exited = first.exited(PROMPT).await.expect("the replica exits");
    assert!(exited.success(), "{exited}");

    mirror.say(Uuid::new_v4(), "Are you there?").await;
    let answered = mirror.until("message_end", TURN).await;
    assert!(errors(&answered).is_empty(), "{:?}", errors(&answered));
    assert_eq!(
        answered.last().expect("the reply").frame["content"],
        "Hello"
    );
    assert!(
        terminated.elapsed() < LEASE,
        "the other replica took the chat {:?} after SIGTERM, not after the lease ran out",
        terminated.elapsed()
    );
    let messages = account.messages(&second, chat).await;
    assert!(
        messages.iter().any(|message| message["role"] == "assistant"
            && message["content"]
                .as_str()
                .is_some_and(|content| content.starts_with("word0 word1 word2 "))),
        "the stopped reply keeps what streamed: {messages:?}"
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn readiness_stays_ready_while_the_database_is_unreachable() {
    let deployment = Deployment::new().await;
    let direct = deployment.database.pool.connect_options();
    let proxy = Blackhole::start(direct.get_host().to_string(), direct.get_port()).await;
    let routed = (*direct).clone().host("127.0.0.1").port(proxy.port());
    let replica = Replica::start(
        &deployment
            .environment
            .clone()
            .with_variable("DATABASE_URL", url(&routed)),
    )
    .await;
    assert_eq!(
        readiness(&replica, StatusCode::OK).await,
        json!({"status": "ready"})
    );

    proxy.silence();
    tokio::time::sleep(Duration::from_secs(1)).await;

    for _ in 0..3 {
        let (code, body) = status(&replica, READY).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body, json!({"status": "ready"}));
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let (health, _) = status(&replica, HEALTH).await;
    assert_eq!(health, StatusCode::OK);
    let mut replica = replica;
    replica.kill().await;
    drop(proxy);
    deployment.retire(vec![replica]).await;
}

#[tokio::test]
async fn a_replica_on_default_settings_exits_promptly() {
    let deployment = Deployment::new().await;
    let mut replica = deployment.replica().await;

    let terminated = Instant::now();
    replica.terminate();

    let exited = replica.exited(PROMPT).await.unwrap_or_else(|| {
        panic!(
            "the replica was still running {:?} after SIGTERM",
            terminated.elapsed()
        )
    });
    assert!(exited.success(), "{exited}");
    deployment.retire(vec![replica]).await;
}

/// A one-shot reminder in `chat`, due now, whose firing is a turn asking `prompt`.
async fn remind(database: &Database, account: &Account, chat: Uuid, prompt: &str) -> Uuid {
    let user: Uuid =
        sqlx::query_scalar("SELECT user_id FROM workspace_members WHERE workspace_id = $1")
            .bind(account.workspace)
            .fetch_one(&database.pool)
            .await
            .expect("the workspace's member");
    sqlx::query_scalar(
        "INSERT INTO reminders (workspace_id, chat_id, created_by, content, due_at, prompt) \
         VALUES ($1, $2, $3, $4, NOW(), $4) RETURNING id",
    )
    .bind(account.workspace)
    .bind(chat)
    .bind(user)
    .bind(prompt)
    .fetch_one(&database.pool)
    .await
    .expect("a reminder")
}

/// How many of the reminder's firings are owed, and how many of those are claimed.
async fn owed(database: &Database, reminder: Uuid) -> (i64, i64) {
    sqlx::query_as("SELECT COUNT(*), COUNT(claimed_at) FROM reminder_turns WHERE reminder_id = $1")
        .bind(reminder)
        .fetch_one(&database.pool)
        .await
        .expect("the reminder's turns")
}

/// Whether the reminder still waits to be delivered.
async fn pending(database: &Database, reminder: Uuid) -> bool {
    sqlx::query_scalar("SELECT status = 'pending' FROM reminders WHERE id = $1")
        .bind(reminder)
        .fetch_one(&database.pool)
        .await
        .expect("the reminder")
}

#[tokio::test]
async fn a_terminated_replica_gives_its_reminder_firing_back_and_takes_no_new_one() {
    let delay = REMINDER_TICK + Duration::from_secs(2);
    let grace = Duration::from_secs(1);
    let deployment = Deployment::new().await;
    let draining = deployment
        .environment
        .clone()
        .with_variable(DELAY, delay.as_secs().to_string())
        .with_variable(GRACE, grace.as_secs().to_string());
    let mut first = Replica::start(&draining).await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    deployment.litellm.queue(Reply::counted(600, PAUSE));
    let running = remind(&deployment.database, &account, chat, "Check the build").await;
    let deadline = Instant::now() + REMINDER_TICK * 2;
    while deployment.litellm.streamed() == 0 {
        assert!(
            Instant::now() < deadline,
            "the replica never started the reminder's turn"
        );
        tokio::time::sleep(PAUSE).await;
    }
    assert_eq!(owed(&deployment.database, running).await, (1, 1));

    first.terminate();
    readiness(&first, StatusCode::SERVICE_UNAVAILABLE).await;
    let late = remind(&deployment.database, &account, chat, "Check the deploy").await;
    let exited = first
        .exited(delay + grace + PROMPT * 3)
        .await
        .expect("the replica exits");
    assert!(exited.success(), "{exited}");

    assert_eq!(
        owed(&deployment.database, running).await,
        (1, 0),
        "the firing whose turn the shutdown stopped is owed again, and unclaimed"
    );
    let (late_owed, late_claimed) = owed(&deployment.database, late).await;
    assert_eq!(
        late_claimed, 0,
        "the draining replica claimed a firing that fell due after SIGTERM"
    );
    assert!(
        pending(&deployment.database, late).await || late_owed == 1,
        "the firing that fell due after SIGTERM is still owed"
    );

    let second = deployment.replica().await;
    let deadline = Instant::now() + REMINDER_TICK + TURN;
    loop {
        let settled = owed(&deployment.database, running).await.0 == 0
            && owed(&deployment.database, late).await.0 == 0
            && !pending(&deployment.database, late).await;
        if settled {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the other replica never ran both firings"
        );
        tokio::time::sleep(PAUSE).await;
    }
    let answers = account
        .messages(&second, chat)
        .await
        .into_iter()
        .filter(|message| message["role"] == "assistant" && message["content"] == ANSWER)
        .count();
    assert_eq!(
        answers, 2,
        "each firing was answered in full on the other replica"
    );
    deployment.retire(vec![first, second]).await;
}
