//! The cluster bus against a real Valkey at `REDIS_URL`. Every test derives
//! its own master key, so its channels live in a namespace no parallel test
//! shares, and it disturbs only connections it opened itself.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use metrics::{Gauge, GaugeFn};
use serde::{Deserialize, Serialize};
use sqlx::ConnectOptions;
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;
use zone_server::cluster::{Bus, Envelope, Kind, Subscription, Topic};

use crate::common::Database;

const WAIT: Duration = Duration::from_secs(10);
const SILENCE: Duration = Duration::from_millis(300);
const REQUEST: Duration = Duration::from_secs(2);
const STARTUP: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_millis(100);
const CONNECTED: f64 = 1.0;
const DISCONNECTED: f64 = 0.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Title {
    chat: Uuid,
    title: String,
}

/// A gauge that remembers every value it was set to.
#[derive(Default)]
struct Recorded {
    values: Mutex<Vec<f64>>,
}

impl Recorded {
    fn values(&self) -> Vec<f64> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn last(&self) -> Option<f64> {
        self.values().last().copied()
    }
}

impl GaugeFn for Recorded {
    fn increment(&self, value: f64) {
        self.set(self.last().unwrap_or_default() + value);
    }

    fn decrement(&self, value: f64) {
        self.set(self.last().unwrap_or_default() - value);
    }

    fn set(&self, value: f64) {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value);
    }
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

fn passphrase() -> String {
    format!("cluster-bus-test-{}", Uuid::new_v4().simple())
}

fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn title() -> Title {
    Title {
        chat: Uuid::new_v4(),
        title: "Quarterly plan".to_string(),
    }
}

fn connect(url: &str, passphrase: &str) -> (Bus, Arc<Recorded>) {
    let recorded = Arc::new(Recorded::default());
    let master = zone_server::secret::derive(passphrase).unwrap();
    let bus = Bus::connect(url, &master, Gauge::from_arc(Arc::clone(&recorded)));
    (bus, recorded)
}

async fn connected(bus: &Bus) {
    tokio::time::timeout(WAIT, bus.connection().wait_for(|connected| *connected))
        .await
        .expect("the bus subscribes on Valkey")
        .unwrap();
}

async fn disconnected(bus: &Bus) {
    tokio::time::timeout(WAIT, bus.connection().wait_for(|connected| !*connected))
        .await
        .expect("the bus notices its connection is gone")
        .unwrap();
}

async fn pair() -> (Bus, Bus) {
    let passphrase = passphrase();
    let (first, _) = connect(&redis_url(), &passphrase);
    let (second, _) = connect(&redis_url(), &passphrase);
    connected(&first).await;
    connected(&second).await;
    (first, second)
}

async fn next(subscription: &mut Subscription) -> Envelope {
    tokio::time::timeout(WAIT, subscription.recv())
        .await
        .expect("an envelope arrives")
        .expect("the subscription is open")
}

async fn silent(subscription: &mut Subscription) {
    tokio::time::sleep(SILENCE).await;
    assert!(matches!(subscription.try_recv(), Err(TryRecvError::Empty)));
}

/// Kills the one pub/sub connection `bus` holds, found by the instance id it
/// names itself with, so connections other tests hold are left alone.
async fn kill_subscription(bus: &Bus) {
    let client = redis::Client::open(redis_url()).unwrap();
    let mut connection = client.get_multiplexed_async_connection().await.unwrap();
    let clients: String = redis::cmd("CLIENT")
        .arg("LIST")
        .arg("TYPE")
        .arg("pubsub")
        .query_async(&mut connection)
        .await
        .unwrap();
    let marker = format!("lib-ver={}", bus.instance());
    let id = clients
        .lines()
        .find(|line| line.split(' ').any(|field| field == marker))
        .and_then(|line| line.split(' ').find_map(|field| field.strip_prefix("id=")))
        .unwrap_or_else(|| panic!("no pub/sub connection names {marker} in {clients}"))
        .to_string();
    let killed: i64 = redis::cmd("CLIENT")
        .arg("KILL")
        .arg("TYPE")
        .arg("pubsub")
        .arg("ID")
        .arg(&id)
        .query_async(&mut connection)
        .await
        .unwrap();
    assert_eq!(killed, 1);
}

#[tokio::test]
async fn a_peer_receives_what_another_publishes_but_the_publisher_does_not() {
    let (first, second) = pair().await;
    let mut theirs = second.subscribe(Topic::Titles).await;
    let mut ours = first.subscribe(Topic::Titles).await;
    let sent = title();

    first.publish(Topic::Titles, &sent);

    let envelope = next(&mut theirs).await;
    assert_eq!(envelope.payload::<Title>().unwrap(), sent);
    assert_eq!(envelope.origin(), first.instance());
    silent(&mut ours).await;
}

#[tokio::test]
async fn envelopes_from_one_publisher_arrive_in_order() {
    let (first, second) = pair().await;
    let chat = Uuid::new_v4();
    let mut frames = second.subscribe(Topic::ChatFrames(chat)).await;

    for sequence in 0..200u64 {
        first.publish_raw(
            Topic::ChatFrames(chat),
            &sequence,
            bytes::Bytes::from(format!(r#"{{"type":"chunk","sequence":{sequence}}}"#)),
        );
    }

    for sequence in 0..200u64 {
        let envelope = next(&mut frames).await;
        assert_eq!(envelope.header::<u64>().unwrap(), sequence);
        assert_eq!(
            envelope.bytes().as_ref(),
            format!(r#"{{"type":"chunk","sequence":{sequence}}}"#).as_bytes()
        );
    }
}

#[tokio::test]
async fn a_request_is_answered_by_a_peer_and_is_none_when_nobody_answers() {
    let (asker, answerer) = pair().await;
    let answering_instance = answerer.instance();
    let chat = Uuid::new_v4();
    let mut control = answerer.subscribe(Topic::ChatControl(chat)).await;
    tokio::spawn(async move {
        let request = next(&mut control).await;
        let mut asked: Title = request.payload().unwrap();
        asked.title.push_str(" (acknowledged)");
        answerer.reply(&request, &asked);
    });

    let reply = asker
        .request(Topic::ChatControl(chat), &title(), REQUEST)
        .await
        .expect("the peer answers");
    let unanswered = asker
        .request(Topic::ChatControl(Uuid::new_v4()), &title(), REQUEST)
        .await;

    assert_eq!(reply.kind(), Kind::Reply);
    assert_eq!(reply.origin(), answering_instance);
    assert_eq!(
        reply.payload::<Title>().unwrap().title,
        "Quarterly plan (acknowledged)"
    );
    assert!(unanswered.is_none());
}

#[tokio::test]
async fn a_bus_with_another_master_key_never_receives() {
    let (sender, _) = connect(&redis_url(), &passphrase());
    let (stranger, _) = connect(&redis_url(), &passphrase());
    connected(&sender).await;
    connected(&stranger).await;
    let mut titles = stranger.subscribe(Topic::Titles).await;

    sender.publish(Topic::Titles, &title());

    silent(&mut titles).await;
}

#[tokio::test]
async fn a_killed_subscription_resubscribes_and_the_gauge_follows() {
    let passphrase = passphrase();
    let (sender, _) = connect(&redis_url(), &passphrase);
    let (receiver, gauge) = connect(&redis_url(), &passphrase);
    connected(&sender).await;
    connected(&receiver).await;
    let mut titles = receiver.subscribe(Topic::Titles).await;
    assert_eq!(gauge.last(), Some(CONNECTED));

    kill_subscription(&receiver).await;
    disconnected(&receiver).await;
    connected(&receiver).await;
    let sent = title();
    sender.publish(Topic::Titles, &sent);

    assert_eq!(next(&mut titles).await.payload::<Title>().unwrap(), sent);
    assert_eq!(
        gauge.values(),
        vec![DISCONNECTED, CONNECTED, DISCONNECTED, CONNECTED]
    );
}

#[tokio::test]
async fn with_valkey_unreachable_local_delivery_works_and_the_gauge_reads_zero() {
    let url = format!("redis://127.0.0.1:{}", closed_port());
    let (bus, gauge) = connect(&url, &passphrase());
    let mut home = bus.subscribe(Topic::Instance(bus.instance())).await;
    let sent = title();

    bus.publish(Topic::Titles, &sent);
    bus.publish(Topic::Instance(bus.instance()), &sent);
    let unanswered = bus
        .request(Topic::ChatControl(Uuid::new_v4()), &sent, SILENCE)
        .await;

    assert_eq!(next(&mut home).await.payload::<Title>().unwrap(), sent);
    assert!(unanswered.is_none());
    assert!(!bus.connected());
    assert_eq!(gauge.values(), vec![DISCONNECTED]);
}

#[tokio::test]
async fn the_server_starts_with_valkey_down_and_reports_the_bus_disconnected() {
    let database = Database::new("cluster_bus").await;
    let url = database.pool.connect_options().to_url_lossy().to_string();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let port = closed_port();
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_zone-server"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("HOST", "127.0.0.1")
        .env("PORT", port.to_string())
        .env("DATABASE_URL", &url)
        .env("REDIS_URL", format!("redis://127.0.0.1:{}", closed_port()))
        .env("JWT_SECRET", passphrase())
        .env("ENCRYPTION_KEY", passphrase())
        .env(
            "LITELLM_HOST",
            format!("http://127.0.0.1:{}", closed_port()),
        )
        .env("LITELLM_KEY", "sk-unused")
        .env("ZONE_AGENT_STATE_DIR", state.path())
        .env("RUST_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let started = tokio::time::Instant::now();
    loop {
        if let Some(status) = server.try_wait().unwrap() {
            panic!("the server exited with {status} while Valkey was down");
        }
        let healthy = client
            .get(format!("{base}/health"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if healthy {
            break;
        }
        assert!(
            started.elapsed() < STARTUP,
            "the server did not come up without Valkey"
        );
        tokio::time::sleep(POLL).await;
    }
    let metrics = client
        .get(format!("{base}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    server.kill().await.unwrap();
    database.cleanup().await;
    assert!(
        metrics.contains("zone_cluster_bus_connected 0"),
        "{metrics}"
    );
}
