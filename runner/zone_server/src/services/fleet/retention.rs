//! Forgetting runners that went away: a row unseen for a week, holding no live work, is removed
//! and the removal audited at server scope. Revoked runners are kept, so a quarantined name stays
//! quarantined.

use chrono::TimeDelta;
use sqlx::PgPool;

use crate::db::DbResult;
use crate::db::audit::{AuditContext, actions, log_action, resources};
use crate::db::fleet::runners::{self, Runner};
use crate::state::AppState;

pub const SWEEP_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
pub const OFFLINE: TimeDelta = TimeDelta::days(7);

/// Removes every runner offline for [`OFFLINE`] that [`runners::forget`] may remove, and audits
/// each one. Answers how many went.
pub async fn sweep(state: &AppState) -> DbResult<usize> {
    let removed = runners::forget(state.db(), OFFLINE).await?;
    for runner in &removed {
        audit(state.db(), runner).await;
    }
    Ok(removed.len())
}

async fn audit(pool: &PgPool, runner: &Runner) {
    let context = AuditContext {
        org_id: None,
        workspace_id: None,
        actor_id: None,
        actor_email: None,
        ip_address: None,
        user_agent: None,
    };
    if let Err(error) = log_action(
        pool,
        &context,
        actions::RUNNER_REMOVED,
        resources::RUNNER,
        Some(runner.id),
        Some(runner.audited()),
        None,
    )
    .await
    {
        tracing::error!(%error, action = actions::RUNNER_REMOVED, "Failed to record audit log");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use uuid::Uuid;
    use zone_enterprise::fleet::Labels;

    use super::*;
    use crate::db::fleet::runners::Registered;
    use crate::db::fleet::state::State;
    use crate::db::tasks;
    use crate::services::fleet::identity::version;
    use crate::services::fleet::tests::Server;

    #[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
    struct Removed {
        organization_id: Option<Uuid>,
        actor_id: Option<Uuid>,
        resource_type: String,
        resource_id: Option<Uuid>,
        old_values: Option<Value>,
        new_values: Option<Value>,
    }

    impl Server {
        async fn runner(&self, name: &str, offline: TimeDelta) -> Registered {
            let registered = runners::upsert(
                &self.database.pool,
                name,
                Uuid::new_v4(),
                &version(),
                &Labels::parse_list("ssd").unwrap(),
            )
            .await
            .expect("a runner");
            sqlx::query("UPDATE fleet_runners SET last_seen_at = now() - $2 WHERE id = $1")
                .bind(registered.runner.id)
                .bind(offline)
                .execute(&self.database.pool)
                .await
                .expect("an offline runner");
            registered
        }

        async fn revoke(&self, runner: Uuid) {
            sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
                .bind(runner)
                .bind(State::Revoked.as_str())
                .execute(&self.database.pool)
                .await
                .expect("a revoked runner");
        }

        async fn live(&self, runner: Uuid) {
            let organization: Uuid = sqlx::query_scalar(
                "INSERT INTO organizations (name, slug) \
                 VALUES ('Retention', gen_random_uuid()::text) RETURNING id",
            )
            .fetch_one(&self.database.pool)
            .await
            .expect("an organization");
            let workspace: Uuid = sqlx::query_scalar(
                "INSERT INTO workspaces (organization_id, name, slug) \
                 VALUES ($1, 'Retention', gen_random_uuid()::text) RETURNING id",
            )
            .bind(organization)
            .fetch_one(&self.database.pool)
            .await
            .expect("a workspace");
            let task: Uuid = sqlx::query_scalar(
                "INSERT INTO tasks (workspace_id, title, description) \
                 VALUES ($1, 'Retention', 'Retention') RETURNING id",
            )
            .bind(workspace)
            .fetch_one(&self.database.pool)
            .await
            .expect("a task");
            let run = tasks::create_task_run(&self.database.pool, task)
                .await
                .expect("a run");
            sqlx::query("UPDATE task_runs SET runner_id = $2, status = 'running' WHERE id = $1")
                .bind(run.id)
                .bind(runner)
                .execute(&self.database.pool)
                .await
                .expect("a live run on the runner");
        }

        async fn names(&self) -> Vec<String> {
            sqlx::query_scalar("SELECT name FROM fleet_runners ORDER BY name")
                .fetch_all(&self.database.pool)
                .await
                .unwrap()
        }

        async fn removals(&self) -> Vec<Removed> {
            sqlx::query_as(
                "SELECT organization_id, actor_id, resource_type, resource_id, old_values, \
                 new_values FROM audit_logs WHERE action = $1 ORDER BY resource_id",
            )
            .bind(actions::RUNNER_REMOVED)
            .fetch_all(&self.database.pool)
            .await
            .unwrap()
        }
    }

    #[tokio::test]
    async fn a_runner_gone_a_week_is_removed_and_its_removal_audited_at_server_scope() {
        let server = Server::new(None).await;
        let gone = server.runner("gone", TimeDelta::days(8)).await.runner;
        server.runner("recent", TimeDelta::days(6)).await;
        server.runner("online", TimeDelta::zero()).await;

        assert_eq!(sweep(&server.state).await.unwrap(), 1);

        assert_eq!(server.names().await, ["online", "recent"]);
        assert_eq!(
            server.removals().await,
            [Removed {
                organization_id: None,
                actor_id: None,
                resource_type: resources::RUNNER.into(),
                resource_id: Some(gone.id),
                old_values: Some(gone.audited()),
                new_values: None,
            }]
        );
        assert_eq!(
            gone.audited(),
            serde_json::json!({
                "name": "gone",
                "version": version().to_string(),
                "labels": ["ssd"],
                "state": "active",
            })
        );
        server.remove().await;
    }

    #[tokio::test]
    async fn a_revoked_runner_and_a_runner_with_live_work_are_kept() {
        let server = Server::new(None).await;
        let revoked = server.runner("revoked", TimeDelta::days(30)).await.runner;
        server.revoke(revoked.id).await;
        let working = server.runner("working", TimeDelta::days(30)).await.runner;
        server.live(working.id).await;

        assert_eq!(sweep(&server.state).await.unwrap(), 0);

        assert_eq!(server.names().await, ["revoked", "working"]);
        assert!(server.removals().await.is_empty());
        server.remove().await;
    }

    #[tokio::test]
    async fn a_second_sweep_finds_nothing_left_to_remove() {
        let server = Server::new(None).await;
        server.runner("gone", TimeDelta::days(8)).await;

        assert_eq!(sweep(&server.state).await.unwrap(), 1);
        assert_eq!(sweep(&server.state).await.unwrap(), 0);

        assert_eq!(server.removals().await.len(), 1);
        server.remove().await;
    }

    #[test]
    fn the_sweep_runs_daily_and_forgets_after_a_week() {
        assert_eq!(SWEEP_INTERVAL_SECONDS, 86_400);
        assert_eq!(OFFLINE, TimeDelta::days(7));
    }
}
