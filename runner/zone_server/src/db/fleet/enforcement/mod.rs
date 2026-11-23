//! Fleet policy at each claim that isn't a task run: a chat's lease, a reminder's turn and an
//! auto project's drive.

mod chat;
mod project;
mod reminder;

use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::fleet::{Labels, Version};

use crate::db::fleet::claimant::Claimant;
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::state::State;
use crate::db::fleet::{labels, policy, revocation, runners};
use crate::db::testing::Database;

/// The process every runner of these tests is registered by, and claims as.
const INSTANCE: Uuid = Uuid::from_u128(0x5eed);

const UNREGISTERED: Claimant = Claimant {
    runner: None,
    instance: INSTANCE,
    enforced: true,
};

fn enforced(runner: Uuid) -> Claimant {
    Claimant {
        runner: Some(runner),
        instance: INSTANCE,
        enforced: true,
    }
}

fn unenforced_on(runner: Uuid) -> Claimant {
    Claimant {
        runner: Some(runner),
        instance: INSTANCE,
        enforced: false,
    }
}

/// A database of the test's own, since a minimum version is server-wide, with one
/// organization, one workspace and one user.
struct Fleet {
    database: Database,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
}

impl Fleet {
    async fn new() -> Self {
        let database = Database::create("fleet_enforcement").await;
        let organization: Uuid = sqlx::query_scalar(
            "INSERT INTO organizations (name, slug) VALUES ('Fleet', gen_random_uuid()::text) \
             RETURNING id",
        )
        .fetch_one(&database.pool)
        .await
        .expect("an organization");
        let workspace: Uuid = sqlx::query_scalar(
            "INSERT INTO workspaces (organization_id, name, slug) \
             VALUES ($1, 'Fleet', gen_random_uuid()::text) RETURNING id",
        )
        .bind(organization)
        .fetch_one(&database.pool)
        .await
        .expect("a workspace");
        let user: Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash) \
             VALUES (gen_random_uuid()::text || '@zone.test', 'test-only') RETURNING id",
        )
        .fetch_one(&database.pool)
        .await
        .expect("a user");
        Self {
            database,
            organization,
            workspace,
            user,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn runner(&self, state: State, version: &str, carried: &str) -> Uuid {
        let runner = runners::upsert(
            self.pool(),
            &format!("runner-{}", Uuid::new_v4().simple()),
            INSTANCE,
            None,
            &Version::parse(version).unwrap(),
            &Labels::parse_list(carried).unwrap(),
        )
        .await
        .expect("a runner")
        .runner
        .id;
        sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
            .bind(runner)
            .bind(state.as_str())
            .execute(self.pool())
            .await
            .expect("the runner's state");
        runner
    }

    /// Registers a newer process under `runner`'s name, on `version` and carrying `carried`, so
    /// the runner's row holds that process's registration.
    async fn take_over(&self, runner: Uuid, version: &str, carried: &str) {
        let name: String = sqlx::query_scalar("SELECT name FROM fleet_runners WHERE id = $1")
            .bind(runner)
            .fetch_one(self.pool())
            .await
            .expect("the runner's name");
        runners::upsert(
            self.pool(),
            &name,
            Uuid::new_v4(),
            None,
            &Version::parse(version).unwrap(),
            &Labels::parse_list(carried).unwrap(),
        )
        .await
        .expect("the other process's registration");
    }

    /// One runner short of the work in each way: cordoned, revoked, below a `2.0.0` minimum,
    /// and missing the `gpu` label the workspace requires. Sets that minimum and label.
    async fn short(&self) -> [(&'static str, Uuid); 4] {
        let cordoned = self.runner(State::Cordoned, "2.0.0", "gpu").await;
        let revoked = self.runner(State::Revoked, "2.0.0", "gpu").await;
        let outdated = self.runner(State::Active, "1.9.0", "gpu").await;
        let unlabelled = self.runner(State::Active, "2.0.0", "").await;
        self.minimum("2.0.0").await;
        self.require(self.workspace, "gpu").await;
        [
            ("cordoned", cordoned),
            ("revoked", revoked),
            ("outdated", outdated),
            ("unlabelled", unlabelled),
        ]
    }

    async fn minimum(&self, minimum: &str) {
        let minimum = Minimum::parse(minimum).unwrap();
        let mut transaction = self.pool().begin().await.unwrap();
        policy::save(&mut transaction, Some(&minimum), self.user)
            .await
            .expect("a minimum version");
        transaction.commit().await.unwrap();
    }

    async fn require(&self, workspace: Uuid, required: &str) {
        let required = Labels::parse_list(required).unwrap();
        let mut transaction = self.pool().begin().await.unwrap();
        labels::save_workspace(&mut transaction, workspace, &required, self.user)
            .await
            .expect("workspace labels");
        transaction.commit().await.unwrap();
    }

    async fn revoke(&self, runner: Uuid) {
        let mut transaction = self.pool().begin().await.unwrap();
        policy::lock(&mut transaction)
            .await
            .expect("the fleet policy lock");
        let revoked = revocation::revoke(&mut transaction, runner, None)
            .await
            .expect("the revoke")
            .expect("the runner exists");
        transaction.commit().await.unwrap();
        revoked.publish();
    }

    async fn chat(&self) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO chats (workspace_id, title, model_name) \
             VALUES ($1, 'Fleet', 'test') RETURNING id",
        )
        .bind(self.workspace)
        .fetch_one(self.pool())
        .await
        .expect("a chat")
    }

    async fn remove(self) {
        self.database.remove().await;
    }
}
