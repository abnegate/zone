//! This server's part in its runner fleet: picking up task runs that the runner that admitted
//! them may not take.

use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::db::fleet::candidate::Candidate;
use crate::db::fleet::eligibility;
use crate::services::fleet;
use crate::state::AppState;
use crate::workers::task;

pub const PICKUP_INTERVAL: Duration = Duration::from_secs(5);

pub fn spawn(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PICKUP_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            for candidate in pickup(&state, task::available()).await {
                let state = state.clone();
                tokio::spawn(async move {
                    task::execute_task_run(&state, candidate.run, candidate.task).await;
                });
            }
        }
    })
}

/// The runs this runner should pick up, at most `capacity` of them. Without fleet policy there
/// are none, since the runner that admits a run always claims it.
pub(crate) async fn pickup(state: &AppState, capacity: usize) -> Vec<Candidate> {
    if capacity == 0 {
        return Vec::new();
    }
    let claimant = fleet::claimant(state).await;
    let (true, Some(runner)) = (claimant.enforced, claimant.runner) else {
        return Vec::new();
    };
    let limit = i64::try_from(capacity).unwrap_or(i64::MAX);
    match eligibility::candidates(state.db(), runner, limit).await {
        Ok(candidates) => candidates,
        Err(error) => {
            tracing::warn!(%error, "Could not look for task runs to pick up");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use uuid::Uuid;
    use zone_enterprise::Plan;
    use zone_enterprise::fleet::{Labels, Refusal};

    use super::*;
    use crate::db::audit::{actions, resources};
    use crate::db::fleet::minimum::Minimum;
    use crate::db::fleet::{labels, policy, runners};
    use crate::db::tasks::{self, TaskRunRow};
    use crate::services::fleet::identity::version;
    use crate::services::fleet::registration;
    use crate::services::fleet::tests::Server;

    #[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
    struct Refused {
        organization_id: Option<Uuid>,
        resource_type: String,
        resource_id: Option<Uuid>,
        new_values: Option<Value>,
    }

    impl Server {
        async fn user(&self) -> Uuid {
            sqlx::query_scalar(
                "INSERT INTO users (email, password_hash) \
                 VALUES (gen_random_uuid()::text || '@zone.test', 'test-only') RETURNING id",
            )
            .fetch_one(&self.database.pool)
            .await
            .expect("a user")
        }

        async fn workspace(&self) -> (Uuid, Uuid) {
            let organization: Uuid = sqlx::query_scalar(
                "INSERT INTO organizations (name, slug) \
                 VALUES ('Pickup', gen_random_uuid()::text) RETURNING id",
            )
            .fetch_one(&self.database.pool)
            .await
            .expect("an organization");
            let workspace = sqlx::query_scalar(
                "INSERT INTO workspaces (organization_id, name, slug) \
                 VALUES ($1, 'Pickup', gen_random_uuid()::text) RETURNING id",
            )
            .bind(organization)
            .fetch_one(&self.database.pool)
            .await
            .expect("a workspace");
            (organization, workspace)
        }

        async fn require(&self, organization: Uuid, required: &str) {
            let actor = self.user().await;
            let mut transaction = self.database.pool.begin().await.unwrap();
            labels::save_organization(
                &mut transaction,
                organization,
                &Labels::parse_list(required).unwrap(),
                actor,
            )
            .await
            .expect("organization labels");
            transaction.commit().await.unwrap();
        }

        async fn minimum(&self, minimum: &str) {
            let actor = self.user().await;
            let minimum = Minimum::parse(minimum).unwrap();
            let mut transaction = self.database.pool.begin().await.unwrap();
            policy::save(&mut transaction, Some(&minimum), actor)
                .await
                .expect("a minimum");
            transaction.commit().await.unwrap();
        }

        async fn admit(&self, workspace: Uuid, settled: f64) -> TaskRunRow {
            let task: Uuid = sqlx::query_scalar(
                "INSERT INTO tasks (workspace_id, title, description) \
                 VALUES ($1, 'Pickup', 'Pickup') RETURNING id",
            )
            .bind(workspace)
            .fetch_one(&self.database.pool)
            .await
            .expect("a task");
            let run = tasks::create_task_run(&self.database.pool, task)
                .await
                .expect("an admitted run");
            sqlx::query(
                "UPDATE task_runs SET heartbeat_at = now() - make_interval(secs => $2) \
                 WHERE id = $1",
            )
            .bind(run.id)
            .bind(settled)
            .execute(&self.database.pool)
            .await
            .expect("a settled run");
            run
        }

        async fn run(&self, run: Uuid) -> TaskRunRow {
            tasks::get_task_run(&self.database.pool, run)
                .await
                .unwrap()
                .expect("the run")
        }

        async fn owner(&self, run: Uuid) -> Option<Uuid> {
            sqlx::query_scalar("SELECT owner FROM task_runs WHERE id = $1")
                .bind(run)
                .fetch_one(&self.database.pool)
                .await
                .unwrap()
        }

        async fn refusals(&self) -> Vec<Refused> {
            sqlx::query_as(
                "SELECT organization_id, resource_type, resource_id, new_values \
                 FROM audit_logs WHERE action = $1",
            )
            .bind(actions::FLEET_RUN_REFUSED)
            .fetch_all(&self.database.pool)
            .await
            .unwrap()
        }
    }

    #[tokio::test]
    async fn pickup_stays_idle_without_an_enterprise_license() {
        for plan in [None, Some(Plan::Team)] {
            let server = Server::new(plan).await;
            registration::register(&server.state).await.unwrap();
            let (_, workspace) = server.workspace().await;
            server.admit(workspace, 5.0).await;

            assert!(pickup(&server.state, 5).await.is_empty(), "{plan:?}");
            server.remove().await;
        }
    }

    #[tokio::test]
    async fn pickup_takes_what_this_runner_may_up_to_its_capacity() {
        let server = Server::new(Some(Plan::Enterprise)).await;
        let (_, workspace) = server.workspace().await;
        let (elsewhere, unplaceable) = server.workspace().await;
        server.require(elsewhere, "ssd").await;
        let oldest = server.admit(workspace, 10.0).await;
        let settled = server.admit(workspace, 5.0).await;
        server.admit(workspace, 0.0).await;
        server.admit(unplaceable, 5.0).await;
        let candidate = |run: &TaskRunRow| Candidate {
            run: run.id,
            task: run.task_id,
        };

        assert!(pickup(&server.state, 0).await.is_empty());
        assert_eq!(pickup(&server.state, 1).await, [candidate(&oldest)]);
        assert_eq!(
            pickup(&server.state, 5).await,
            [candidate(&oldest), candidate(&settled)]
        );
        server.remove().await;
    }

    #[tokio::test]
    async fn a_run_no_runner_can_take_fails_naming_what_it_needs_in_its_organization() {
        let server = Server::new(Some(Plan::Enterprise)).await;
        let (organization, workspace) = server.workspace().await;
        server.require(organization, "ssd").await;
        server.minimum("9.0.0").await;
        let run = server.admit(workspace, 0.0).await;

        task::execute_task_run(&server.state, run.id, run.task_id).await;

        let failed = server.run(run.id).await;
        let refusal = Refusal::NoRunner {
            minimum: Some(Minimum::parse("9.0.0").unwrap().version().clone()),
            labels: Labels::parse_list("ssd").unwrap(),
        };
        assert_eq!(failed.status, tasks::FAILED);
        assert_eq!(failed.error_message, Some(refusal.message()));
        assert_eq!(
            server.refusals().await,
            [Refused {
                organization_id: Some(organization),
                resource_type: resources::TASK.into(),
                resource_id: Some(run.task_id),
                new_values: Some(json!({
                    "code": refusal.code(),
                    "minimum": "9.0.0",
                    "labels": ["ssd"],
                })),
            }]
        );
        server.remove().await;
    }

    #[tokio::test]
    async fn a_run_another_runner_can_take_is_left_for_it_to_pick_up() {
        let server = Server::new(Some(Plan::Enterprise)).await;
        let (organization, workspace) = server.workspace().await;
        server.require(organization, "ssd").await;
        runners::upsert(
            &server.database.pool,
            "another",
            Uuid::new_v4(),
            &version(),
            &Labels::parse_list("ssd").unwrap(),
        )
        .await
        .unwrap();
        let run = server.admit(workspace, 0.0).await;

        task::execute_task_run(&server.state, run.id, run.task_id).await;

        assert_eq!(server.run(run.id).await.status, "running");
        assert_eq!(server.owner(run.id).await, None);
        assert!(server.refusals().await.is_empty());
        server.remove().await;
    }
}
