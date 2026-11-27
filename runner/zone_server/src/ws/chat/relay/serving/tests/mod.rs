use std::time::Duration;

use metrics::Gauge;
use uuid::Uuid;

use super::*;
use crate::secret;

const PASSPHRASE: &str = "a-serving-passphrase-of-at-least-32-bytes";
const LIFETIME: Duration = Duration::from_secs(30);
const CONNECTED: Duration = Duration::from_secs(10);

/// A bus on the Valkey at `REDIS_URL`, as one replica of a cluster sharing `passphrase`.
async fn replica(passphrase: &str) -> Bus {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let bus = Bus::connect(&url, &secret::derive(passphrase).unwrap(), Gauge::noop());
    tokio::time::timeout(CONNECTED, bus.connection().wait_for(|connected| *connected))
        .await
        .expect("the bus reaches Valkey at REDIS_URL")
        .expect("the bus status is watched");
    bus
}

fn serving(bus: &Bus, holding: bool) -> Serving {
    Serving {
        instance: bus.instance(),
        answerer: Uuid::new_v4(),
        holding,
    }
}

fn taken(claim: Claim) -> String {
    match claim {
        Claim::Taken(payload) => payload,
        other => panic!("the key was not taken: {other:?}"),
    }
}

#[test]
fn the_serving_key_sits_beside_the_chat_topics_in_the_namespace() {
    let namespace = Namespace::derive(&secret::derive(PASSPHRASE).unwrap());
    let chat = Uuid::new_v4();

    assert_eq!(
        Serving::key(&namespace, chat),
        format!("zone:{namespace}:chat:{chat}:serving")
    );
}

#[test]
fn a_serving_key_reads_back_as_written() {
    let serving = Serving {
        instance: Uuid::new_v4(),
        answerer: Uuid::new_v4(),
        holding: true,
    };

    let payload = serde_json::to_string(&serving).unwrap();

    assert_eq!(serde_json::from_str::<Serving>(&payload).unwrap(), serving);
}

#[tokio::test]
async fn a_replica_holding_the_chat_keeps_its_key_from_every_other_replica() {
    let passphrase = format!("serving-claim-{}", Uuid::new_v4().simple());
    let (holder, other) = (replica(&passphrase).await, replica(&passphrase).await);
    let chat = Uuid::new_v4();
    let holding = serving(&holder, true);

    taken(holding.claim(&holder, chat, LIFETIME).await);

    assert_eq!(
        serving(&other, true).claim(&other, chat, LIFETIME).await,
        Claim::Held
    );
    assert_eq!(
        serving(&other, false).claim(&other, chat, LIFETIME).await,
        Claim::Held
    );
    assert_eq!(Serving::elsewhere(&other, chat).await, Some(holding));
    let waiting = Serving {
        holding: false,
        ..holding
    };
    taken(waiting.claim(&holder, chat, LIFETIME).await);
    assert_eq!(
        Serving::elsewhere(&other, chat).await,
        Some(waiting),
        "the replica holding the key rewrites it as it likes"
    );
}

#[tokio::test]
async fn a_replica_waiting_out_its_grace_gives_its_key_up_only_to_one_holding_the_chat() {
    let passphrase = format!("serving-claim-{}", Uuid::new_v4().simple());
    let (waiting, other) = (replica(&passphrase).await, replica(&passphrase).await);
    let chat = Uuid::new_v4();
    let graceful = serving(&waiting, false);
    taken(graceful.claim(&waiting, chat, LIFETIME).await);

    assert_eq!(
        serving(&other, false).claim(&other, chat, LIFETIME).await,
        Claim::Held,
        "two replicas waiting out their grace: the first one answers"
    );
    let holding = serving(&other, true);
    taken(holding.claim(&other, chat, LIFETIME).await);

    assert_eq!(Serving::elsewhere(&waiting, chat).await, Some(holding));
    assert_eq!(graceful.claim(&waiting, chat, LIFETIME).await, Claim::Held);
}

#[tokio::test]
async fn a_lapsed_or_erased_key_is_anyones_and_an_erase_leaves_a_newer_claim() {
    let passphrase = format!("serving-claim-{}", Uuid::new_v4().simple());
    let (first, second) = (replica(&passphrase).await, replica(&passphrase).await);
    let chat = Uuid::new_v4();
    let lapsing = serving(&first, true);

    taken(
        lapsing
            .claim(&first, chat, Duration::from_millis(200))
            .await,
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let payload = taken(serving(&second, true).claim(&second, chat, LIFETIME).await);

    Serving::erase(&first, chat, &serde_json::to_string(&lapsing).unwrap()).await;
    assert!(Serving::elsewhere(&first, chat).await.is_some());
    Serving::erase(&second, chat, &payload).await;
    assert_eq!(Serving::elsewhere(&first, chat).await, None);
    taken(serving(&first, false).claim(&first, chat, LIFETIME).await);
}

#[tokio::test]
async fn an_unreadable_key_is_overwritten_rather_than_obeyed() {
    let passphrase = format!("serving-claim-{}", Uuid::new_v4().simple());
    let bus = replica(&passphrase).await;
    let chat = Uuid::new_v4();
    let mut connection = bus.valkey().expect("a Valkey connection");
    redis::cmd("SET")
        .arg(Serving::key(bus.namespace(), chat))
        .arg("not json")
        .query_async::<()>(&mut connection)
        .await
        .unwrap();

    taken(serving(&bus, false).claim(&bus, chat, LIFETIME).await);
}
