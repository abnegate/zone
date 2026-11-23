//! Replicas as fleet runners: real `zone-server` processes on one database and one Valkey, under
//! fleet policy. Of the live processes sharing a runner name only the newest takes work, so each
//! replica, like each pod of a deployment, needs a name of its own.

use std::time::Duration;

use sqlx::ConnectOptions;
use tokio::time::Instant;
use uuid::Uuid;
use zone_enterprise::fleet::Refusal;
use zone_server::db::{licenses, migrations};
use zone_server::services::fleet::identity::NAME;

use crate::common::Database;
use crate::common::chat_socket::{ChatSocket, Received};
use crate::common::replica::{Account, Environment, Replica};
use crate::common::sso::active;
use crate::common::stub_litellm::{Reply, StubLitellm};

const PREFIX: &str = "high_availability_fleet";
const JOIN: Duration = Duration::from_secs(10);
const TURN: Duration = Duration::from_secs(60);
const PAUSE: Duration = Duration::from_millis(10);
const ANSWERED: &str = "message_end";
const DENIED: &str = "denied";
const FAILED: &str = "error";

struct Deployment {
    database: Database,
    litellm: StubLitellm,
    environment: Environment,
}

impl Deployment {
    /// A deployment whose database holds a license key before any replica starts. A replica
    /// trusts only the production keyring, which can't verify the test's key, and a stored key
    /// that doesn't verify keeps every feature, fleet policy among them, enforced.
    async fn enforced() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool)
            .await
            .expect("the schema is applied");
        licenses::replace(&database.pool, &active())
            .await
            .expect("the license is stored");
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

    /// A replica registering under `name`, whatever the harness would have named it.
    async fn named(&self, name: &str) -> Replica {
        Replica::start(&self.environment.clone().with_variable(NAME, name)).await
    }

    /// Every registered runner's name and the process instance holding it, by name.
    async fn runners(&self) -> Vec<(String, Uuid)> {
        sqlx::query_as("SELECT name, instance_id FROM fleet_runners ORDER BY name")
            .fetch_all(&self.database.pool)
            .await
            .expect("the runners can be read")
    }

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

/// Sends a message on a new chat through `replica`'s own listener, and every frame up to the
/// one ending the turn: its answer, a denial or an error.
async fn turn(replica: &Replica, account: &Account) -> Vec<Received> {
    let chat = account.chat(replica, false).await;
    let mut socket = ChatSocket::joined(replica, chat, &account.token, JOIN).await;
    socket.say(Uuid::new_v4(), "Hello").await;
    let mut frames = Vec::new();
    loop {
        let Some(frame) = socket.next(TURN).await else {
            panic!(
                "the turn on {} never ended: {:?}",
                replica.name(),
                texts(&frames)
            );
        };
        let ended = [ANSWERED, DENIED, FAILED].contains(&frame.kind());
        frames.push(frame);
        if ended {
            return frames;
        }
    }
}

fn texts(frames: &[Received]) -> Vec<&str> {
    frames.iter().map(|frame| frame.text.as_str()).collect()
}

fn chunks(frames: &[Received]) -> String {
    frames
        .iter()
        .filter(|frame| frame.kind() == "chunk")
        .map(|frame| frame.frame["content"].as_str().unwrap_or_default())
        .collect()
}

fn ending(frames: &[Received]) -> &Received {
    frames.last().expect("a turn ends with a frame")
}

fn unregistered(frames: &[Received]) -> bool {
    frames
        .iter()
        .any(|frame| frame.frame["code"] == Refusal::Unregistered.code())
}

#[tokio::test]
async fn under_fleet_policy_every_replica_registers_its_own_runner_and_takes_work() {
    let deployment = Deployment::enforced().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let mut named = vec![first.name().to_string(), second.name().to_string()];
    named.sort();
    let runners = deployment.runners().await;
    let reply = Reply::counted(3, PAUSE);
    deployment.litellm.fall_back_to(reply.clone());
    let account = Account::register(&first).await;
    let turns = [
        (first.name().to_string(), turn(&first, &account).await),
        (second.name().to_string(), turn(&second, &account).await),
    ];
    deployment.retire(vec![first, second]).await;

    let names: Vec<&str> = runners.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names, named,
        "each replica holds a runner row under the name the harness gave it"
    );
    assert_ne!(
        runners[0].1, runners[1].1,
        "each runner row is held by a process of its own"
    );
    for (name, frames) in &turns {
        assert!(
            !unregistered(frames),
            "fleet policy refused {name} as unregistered: {:?}",
            texts(frames)
        );
        assert_eq!(
            ending(frames).kind(),
            ANSWERED,
            "{name} ran its turn: {:?}",
            texts(frames)
        );
        assert_eq!(chunks(frames), reply.text(), "{name} streamed the answer");
    }
}

#[tokio::test]
async fn under_fleet_policy_a_replica_whose_name_a_newer_one_took_takes_no_work() {
    let deployment = Deployment::enforced().await;
    let older = deployment.replica().await;
    let newer = deployment.named(older.name()).await;
    let name = older.name().to_string();
    let runners = deployment.runners().await;
    let reply = Reply::counted(3, PAUSE);
    deployment.litellm.fall_back_to(reply.clone());
    let account = Account::register(&older).await;
    let refused = turn(&older, &account).await;
    let served = turn(&newer, &account).await;
    deployment.retire(vec![older, newer]).await;

    let names: Vec<&str> = runners.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, [name.as_str()], "the replicas share one runner row");
    let denial = &ending(&refused).frame;
    assert_eq!(
        (denial["type"].as_str(), denial["code"].as_str()),
        (Some(DENIED), Some(Refusal::Unregistered.code())),
        "the older replica takes no work: {:?}",
        texts(&refused)
    );
    assert_eq!(denial["message"], Refusal::Unregistered.message());
    assert_eq!(
        ending(&served).kind(),
        ANSWERED,
        "the newer replica holds the name and takes work: {:?}",
        texts(&served)
    );
    assert_eq!(chunks(&served), reply.text());
}
