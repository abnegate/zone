//! The task relay's bridges between two replicas: two buses on the Valkey at
//! `REDIS_URL`, each relaying for a registry of its own, as two processes
//! would. Every test derives its own master key, so no parallel test hears it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use metrics::Gauge;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::TryRecvError;
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;
use zone_server::agent::question::Answer;
use zone_server::cluster::Bus;
use zone_server::services::task_relay::{Registry, Relay};

const WAIT: Duration = Duration::from_secs(10);
const SILENCE: Duration = Duration::from_millis(500);
const TITLE: &str = "Quarterly plan";

/// One replica's registries, kept apart from every other replica's.
struct Replica {
    waiting: Mutex<HashMap<Uuid, oneshot::Sender<Vec<Answer>>>>,
    titles: broadcast::Sender<(Uuid, String)>,
    actions: broadcast::Sender<(Uuid, Value)>,
}

impl Replica {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            waiting: Mutex::new(HashMap::new()),
            titles: broadcast::channel(16).0,
            actions: broadcast::channel(16).0,
        })
    }

    fn expect(&self, run: Uuid) -> oneshot::Receiver<Vec<Answer>> {
        let (sender, receiver) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(run, sender);
        receiver
    }

    fn waits_on(&self, run: Uuid) -> bool {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&run)
    }
}

impl Registry for Replica {
    fn answer(&self, run: Uuid, answers: Vec<Answer>) -> bool {
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&run)
            .is_some_and(|sender| sender.send(answers).is_ok())
    }

    fn title(&self, chat: Uuid, title: String) {
        let _ = self.titles.send((chat, title));
    }

    fn action(&self, chat: Uuid, message: Value) {
        let _ = self.actions.send((chat, message));
    }
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

async fn bus(passphrase: &str) -> Bus {
    let master = zone_server::secret::derive(passphrase).unwrap();
    let bus = Bus::connect(&redis_url(), &master, Gauge::noop());
    tokio::time::timeout(WAIT, bus.connection().wait_for(|connected| *connected))
        .await
        .expect("the bus subscribes on Valkey")
        .unwrap();
    bus
}

/// Two buses of one cluster, with nothing relaying on either yet.
async fn pair() -> (Bus, Bus) {
    let passphrase = format!("task-relay-test-{}", Uuid::new_v4().simple());
    (bus(&passphrase).await, bus(&passphrase).await)
}

fn answers() -> Vec<Answer> {
    vec![Answer {
        header: "Scope".to_string(),
        labels: vec!["Backfill".to_string()],
        other: None,
    }]
}

async fn next<T: Clone>(receiver: &mut broadcast::Receiver<T>) -> T {
    tokio::time::timeout(WAIT, receiver.recv())
        .await
        .expect("an update arrives")
        .expect("the channel is open")
}

async fn silent<T: Clone>(receiver: &mut broadcast::Receiver<T>) {
    tokio::time::sleep(SILENCE).await;
    assert!(
        matches!(receiver.try_recv(), Err(TryRecvError::Empty)),
        "nothing else arrives"
    );
}

#[tokio::test]
async fn an_answer_posted_on_one_replica_resolves_a_run_waiting_only_on_another() {
    let (first, second) = pair().await;
    let (here, there) = (Replica::new(), Replica::new());
    let posting = Relay::start(first, Arc::clone(&here)).await;
    let _owning = Relay::start(second, Arc::clone(&there)).await;
    let run = Uuid::new_v4();
    let waiting = there.expect(run);

    assert!(
        posting.answer(run, answers()).await,
        "the replica the answer was posted on reports it delivered"
    );

    let delivered = tokio::time::timeout(WAIT, waiting)
        .await
        .expect("the waiting run is resolved")
        .expect("the answers arrive");
    assert_eq!(delivered, answers());
    assert!(!there.waits_on(run), "the claim is spent");
}

#[tokio::test]
async fn an_answer_is_resolved_locally_when_the_run_waits_on_the_same_replica() {
    let (first, second) = pair().await;
    let (here, there) = (Replica::new(), Replica::new());
    let posting = Relay::start(first, Arc::clone(&here)).await;
    let _other = Relay::start(second, Arc::clone(&there)).await;
    let run = Uuid::new_v4();
    let waiting = here.expect(run);

    assert!(posting.answer(run, answers()).await);

    assert_eq!(waiting.await.unwrap(), answers());
}

#[tokio::test]
async fn without_a_bridge_on_the_owner_an_answer_finds_nothing_to_resolve() {
    let (first, _second) = pair().await;
    let (here, there) = (Replica::new(), Replica::new());
    let posting = Relay::start(first, Arc::clone(&here)).await;
    let run = Uuid::new_v4();
    let _waiting = there.expect(run);

    assert!(!posting.answer(run, answers()).await);
    assert!(
        there.waits_on(run),
        "a registry no bridge serves is never reached"
    );
}

#[tokio::test]
async fn an_answer_nobody_waits_for_reports_nothing_delivered() {
    let (first, second) = pair().await;
    let posting = Relay::start(first, Replica::new()).await;
    let _other = Relay::start(second, Replica::new()).await;

    assert!(!posting.answer(Uuid::new_v4(), answers()).await);
}

#[tokio::test]
async fn a_title_reaches_the_other_replica_once_and_is_never_relayed_back() {
    let (first, second) = pair().await;
    let (here, there) = (Replica::new(), Replica::new());
    let mut ours = here.titles.subscribe();
    let mut theirs = there.titles.subscribe();
    let publishing = Relay::start(first, Arc::clone(&here)).await;
    let _relaying = Relay::start(second, Arc::clone(&there)).await;
    let chat = Uuid::new_v4();

    publishing.title(chat, TITLE.to_string());

    assert_eq!(next(&mut theirs).await, (chat, TITLE.to_string()));
    assert_eq!(next(&mut ours).await, (chat, TITLE.to_string()));
    silent(&mut theirs).await;
    silent(&mut ours).await;
}

#[tokio::test]
async fn an_action_card_reaches_the_other_replica_once_and_is_never_relayed_back() {
    let (first, second) = pair().await;
    let (here, there) = (Replica::new(), Replica::new());
    let mut ours = here.actions.subscribe();
    let mut theirs = there.actions.subscribe();
    let publishing = Relay::start(first, Arc::clone(&here)).await;
    let _relaying = Relay::start(second, Arc::clone(&there)).await;
    let chat = Uuid::new_v4();
    let message =
        json!({"id": Uuid::new_v4(), "role": "assistant", "content": "The task finished."});

    publishing.action(chat, message.clone());

    assert_eq!(next(&mut theirs).await, (chat, message.clone()));
    assert_eq!(next(&mut ours).await, (chat, message));
    silent(&mut theirs).await;
    silent(&mut ours).await;
}

#[tokio::test]
async fn a_title_published_without_a_bridge_on_the_other_replica_stays_local() {
    let (first, _second) = pair().await;
    let (here, there) = (Replica::new(), Replica::new());
    let mut theirs = there.titles.subscribe();
    let publishing = Relay::start(first, Arc::clone(&here)).await;

    publishing.title(Uuid::new_v4(), TITLE.to_string());

    silent(&mut theirs).await;
}
