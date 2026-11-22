//! Chat updates made outside a turn's frames, a new title and an action card,
//! reaching a socket on a replica other than the one that made them: real
//! `zone-server` processes on one database and one Valkey.

use std::time::Duration;

use serde_json::json;
use sqlx::ConnectOptions;
use tokio::time::Instant;
use uuid::Uuid;

use crate::common::Database;
use crate::common::chat_socket::{ChatSocket, Received};
use crate::common::replica::{Account, Environment, Replica};
use crate::common::stub_litellm::StubLitellm;

const PREFIX: &str = "high_availability_updates";
const JOIN: Duration = Duration::from_secs(10);
const UPDATE: Duration = Duration::from_secs(45);
const TITLE: &str = "Release checklist review";
const TASK_RUN: &str = "task_run";
const CANCELLED: &str = "cancelled";

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

    /// A run of a task posted to `chat`, running with nobody holding it, as
    /// one parked between heartbeats looks.
    async fn running(&self, account: &Account, chat: Uuid) -> Uuid {
        let task: Uuid = sqlx::query_scalar(
            "INSERT INTO tasks (workspace_id, title, description, status, chat_id) \
             VALUES ($1, 'Ship the release', 'Cut and tag it', 'in_progress', $2) RETURNING id",
        )
        .bind(account.workspace)
        .bind(chat)
        .fetch_one(&self.database.pool)
        .await
        .expect("the task is stored");
        let run: Uuid = sqlx::query_scalar(
            "INSERT INTO task_runs (task_id, status) VALUES ($1, 'running') RETURNING id",
        )
        .bind(task)
        .fetch_one(&self.database.pool)
        .await
        .expect("the run is stored");
        sqlx::query("UPDATE tasks SET active_run_id = $2 WHERE id = $1")
            .bind(task)
            .bind(run)
            .execute(&self.database.pool)
            .await
            .expect("the run is the task's active run");
        run
    }
}

fn titled(frames: &[Received]) -> Vec<&str> {
    frames
        .iter()
        .filter(|frame| frame.kind() == "title_updated")
        .filter_map(|frame| frame.frame["title"].as_str())
        .collect()
}

/// Reads until a message from a task run is saved.
async fn announced(socket: &mut ChatSocket) -> Received {
    loop {
        let frames = socket.until("message_saved", UPDATE).await;
        if let Some(frame) = frames
            .into_iter()
            .last()
            .filter(|frame| frame.frame["metadata"]["source"] == TASK_RUN)
        {
            return frame;
        }
    }
}

#[tokio::test]
async fn a_title_generated_on_one_replica_reaches_a_socket_on_another() {
    let deployment = Deployment::new().await;
    deployment.litellm.complete_with(TITLE);
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, true).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;
    let mut owner = ChatSocket::joined(&first, chat, &account.token, JOIN).await;

    owner
        .say(Uuid::new_v4(), "Walk me through the release")
        .await;
    let owned = owner.until("title_updated", UPDATE).await;
    let mirrored = mirror.until("title_updated", UPDATE).await;

    assert_eq!(titled(&owned), [TITLE]);
    assert_eq!(
        titled(&mirrored),
        [TITLE],
        "the socket on the other replica hears the new title"
    );
    deployment.retire(vec![first, second]).await;
}

#[tokio::test]
async fn an_action_card_stored_on_one_replica_reaches_a_socket_on_another() {
    let deployment = Deployment::new().await;
    let first = deployment.replica().await;
    let second = deployment.replica().await;
    let account = Account::register(&first).await;
    let chat = account.chat(&first, false).await;
    let run = deployment.running(&account, chat).await;
    let mut mirror = ChatSocket::joined(&second, chat, &account.token, JOIN).await;

    let stopped = first
        .post(
            &format!("/api/tasks/runs/{run}/stop"),
            &json!({}),
            Some(&account.token),
        )
        .await;
    assert_eq!(stopped["run"]["status"], CANCELLED, "{stopped}");

    let card = announced(&mut mirror).await;
    assert_eq!(card.frame["metadata"]["run_id"], run.to_string());
    assert_eq!(card.frame["metadata"]["status"], CANCELLED);
    deployment.retire(vec![first, second]).await;
}
