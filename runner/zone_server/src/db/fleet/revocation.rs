//! Revoking a fleet runner: it claims nothing more, and the work it holds ends.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::fleet::state::State;
use crate::db::tasks::{self, ACTIVE_RUN_STATUSES, TaskRunRow};
use crate::services::chat::notice::Notice;
use crate::services::task_progress;

/// Why a revoked runner's live run ended, in the row and in the frame
/// published to whoever was waiting on that run.
pub const RUN_REVOKED: &str = "The runner this task was running on was revoked from the fleet.";

/// What a revoke changed: the runner's state before it, and what to publish
/// once the transaction that revoked it commits.
#[must_use]
#[derive(Debug)]
pub struct Revoked {
    pub previous: String,
    pub notices: Vec<Notice>,
    pub runs: Vec<TaskRunRow>,
}

impl Revoked {
    /// Publish the chat notices and terminal frames of the stopped runs. Call
    /// it only after the revoke commits. Gives the stopped runs back, so the
    /// caller can signal their owners as a stop does.
    pub fn publish(self) -> Vec<TaskRunRow> {
        for notice in self.notices {
            notice.publish();
        }
        for run in &self.runs {
            task_progress::publish_terminal(run.id, tasks::CANCELLED, Some(RUN_REVOKED));
        }
        self.runs
    }
}

/// Revoke `runner` on `actor`'s behalf, and end the work it holds: each live
/// task run is stopped as a user's stop would, keeping its task reserved
/// while its owner drains, its chat leases expire, and its auto-project
/// claims are cleared. `None` when there is no such runner.
///
/// Lock order is `fleet_policy` → `fleet_runners` → `tasks` → `task_runs` →
/// `chat_leases` / `projects`. The caller must already hold `fleet_policy`
/// `FOR UPDATE` and no task, run, lease or project lock. This locks the
/// runner row, which waits for every claim holding it `FOR SHARE`, so a claim
/// that committed first is seen and stopped here; then each live run's task
/// before the run, in task id order.
pub async fn revoke(
    transaction: &mut PgConnection,
    runner: Uuid,
    actor: Option<Uuid>,
) -> DbResult<Option<Revoked>> {
    let previous: Option<String> =
        sqlx::query_scalar("SELECT state FROM fleet_runners WHERE id = $1 FOR UPDATE")
            .bind(runner)
            .fetch_optional(&mut *transaction)
            .await?;
    let Some(previous) = previous else {
        return Ok(None);
    };
    sqlx::query(
        "UPDATE fleet_runners SET state = $2, state_changed_by = $3, state_changed_at = NOW() \
         WHERE id = $1 AND state <> $2",
    )
    .bind(runner)
    .bind(State::Revoked.as_str())
    .bind(actor)
    .execute(&mut *transaction)
    .await?;

    let live: Vec<(Uuid, Uuid)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT id, task_id FROM task_runs \
         WHERE runner_id = $1 AND status IN {ACTIVE_RUN_STATUSES} ORDER BY task_id, id"
    )))
    .bind(runner)
    .fetch_all(&mut *transaction)
    .await?;
    let mut runs = Vec::with_capacity(live.len());
    let mut notices = Vec::new();
    for (run, task) in live {
        sqlx::query("SELECT id FROM tasks WHERE id = $1 FOR UPDATE")
            .bind(task)
            .execute(&mut *transaction)
            .await?;
        if let Some((row, notice)) = tasks::cancel_run(transaction, run, RUN_REVOKED).await? {
            notices.extend(notice);
            runs.push(row);
        }
    }

    sqlx::query(
        "UPDATE chat_leases SET expires_at = clock_timestamp() \
         WHERE runner_id = $1 AND expires_at > clock_timestamp()",
    )
    .bind(runner)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE projects SET auto_claim_id = NULL, auto_claimed_at = NULL \
         WHERE auto_claim_runner_id = $1 AND auto_claim_id IS NOT NULL",
    )
    .bind(runner)
    .execute(&mut *transaction)
    .await?;

    Ok(Some(Revoked {
        previous,
        notices,
        runs,
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::PgPool;
    use uuid::Uuid;

    use super::*;
    use crate::auth::Permission;
    use crate::db::auto_projects::{self, Claim};
    use crate::db::context::{self, Lease, Store};
    use crate::db::recovery;
    use crate::db::tasks::{Mutation, RunMutation};
    use crate::db::testing::Tenant;

    const LIFETIME: Duration = Duration::from_secs(30);

    struct Work {
        task: Uuid,
        run: Uuid,
        owner: Uuid,
        store: Store,
        lease: Lease,
        claim: Claim,
    }

    async fn runner(pool: &PgPool) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO fleet_runners (name, instance_id, version, version_key) \
             VALUES ($1, $2, '0.1.0', '{0,1,0,1}') RETURNING id",
        )
        .bind(format!("runner-{}", Uuid::new_v4()))
        .bind(Uuid::new_v4())
        .fetch_one(pool)
        .await
        .expect("a runner")
    }

    async fn remove(pool: &PgPool, runners: &[Uuid]) {
        sqlx::query("DELETE FROM fleet_runners WHERE id = ANY($1)")
            .bind(runners)
            .execute(pool)
            .await
            .expect("the runners are removed");
    }

    /// A claimed task run, a chat lease and an auto-project claim, each
    /// recorded against `runner`.
    async fn work(tenant: &Tenant, writer: Uuid, runner: Uuid) -> Work {
        let pool = &tenant.pool;
        let task = tasks::create_task(
            pool,
            tenant.workspace,
            &[],
            "Revocation",
            "Runs on a runner",
            None,
            None,
            true,
            None,
        )
        .await
        .expect("a task");
        let Mutation::Applied(RunMutation::Created(run)) =
            tasks::create_task_run_authorized(pool, writer, task.id)
                .await
                .expect("the admission")
        else {
            panic!("the run was not admitted");
        };
        let owner = Uuid::new_v4();
        assert!(
            tasks::claim_task_run(pool, run.id, owner)
                .await
                .expect("the claim"),
            "the run was not claimed"
        );
        sqlx::query("UPDATE task_runs SET runner_id = $2 WHERE id = $1")
            .bind(run.id)
            .bind(runner)
            .execute(pool)
            .await
            .expect("the run's runner");

        let chat: Uuid = sqlx::query_scalar(
            "INSERT INTO chats (workspace_id, title, model_name) \
             VALUES ($1, 'Revocation', 'test') RETURNING id",
        )
        .bind(tenant.workspace)
        .fetch_one(pool)
        .await
        .expect("a chat");
        let store = Store::new(pool.clone(), chat, Some(tenant.workspace));
        let lease = store
            .acquire(Uuid::new_v4(), LIFETIME)
            .await
            .expect("a lease");
        sqlx::query("UPDATE chat_leases SET runner_id = $2 WHERE chat_id = $1")
            .bind(chat)
            .bind(runner)
            .execute(pool)
            .await
            .expect("the lease's runner");

        let owner_of_claim = Uuid::new_v4();
        let project: Uuid = sqlx::query_scalar(
            "INSERT INTO projects (workspace_id, name, auto_claim_id, auto_claimed_at, \
               auto_claim_runner_id) \
             VALUES ($1, 'Revocation', $2, NOW(), $3) RETURNING id",
        )
        .bind(tenant.workspace)
        .bind(owner_of_claim)
        .bind(runner)
        .fetch_one(pool)
        .await
        .expect("a claimed project");

        Work {
            task: task.id,
            run: run.id,
            owner,
            store,
            lease,
            claim: Claim {
                project_id: project,
                owner: owner_of_claim,
            },
        }
    }

    async fn revoke_committed(pool: &PgPool, runner: Uuid, actor: Option<Uuid>) -> Option<Revoked> {
        let mut transaction = pool.begin().await.expect("a transaction");
        sqlx::query("SELECT id FROM fleet_policy FOR UPDATE")
            .execute(&mut *transaction)
            .await
            .expect("the fleet policy lock");
        let revoked = revoke(&mut transaction, runner, actor)
            .await
            .expect("the revoke");
        transaction.commit().await.expect("the revoke commits");
        revoked
    }

    async fn task_state(pool: &PgPool, task: Uuid) -> (String, Option<Uuid>) {
        sqlx::query_as("SELECT status, active_run_id FROM tasks WHERE id = $1")
            .bind(task)
            .fetch_one(pool)
            .await
            .expect("the task")
    }

    async fn runs_of(pool: &PgPool, task: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM task_runs WHERE task_id = $1")
            .bind(task)
            .fetch_one(pool)
            .await
            .expect("the task's runs")
    }

    /// Every fenced write the revoked runner's work makes fails at once, the
    /// task stays reserved for the draining owner with no second run, and a
    /// runner that was not revoked keeps all of its work.
    #[tokio::test]
    async fn revoking_ends_the_runners_work_and_fences_its_writes() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let writer = tenant.workspace_holder(&[Permission::WorkspaceWrite]).await;
        let actor = tenant.user().await;
        let revoked_runner = runner(pool).await;
        let kept_runner = runner(pool).await;
        let revoked_work = work(&tenant, writer, revoked_runner).await;
        let kept_work = work(&tenant, writer, kept_runner).await;

        let revoked = revoke_committed(pool, revoked_runner, Some(actor))
            .await
            .expect("the runner exists");
        let previous = revoked.previous.clone();
        let stopped: Vec<Uuid> = revoked.publish().iter().map(|run| run.id).collect();

        let heartbeat = tasks::heartbeat_task_run(pool, revoked_work.run, revoked_work.owner).await;
        let plan = tasks::record_run_plan(pool, revoked_work.run, revoked_work.owner, "plan").await;
        let renewed = revoked_work
            .store
            .renew(&revoked_work.lease, LIFETIME)
            .await;
        let project = auto_projects::renew(pool, revoked_work.claim).await;
        let cleared: Option<Uuid> =
            sqlx::query_scalar("SELECT auto_claim_id FROM projects WHERE id = $1")
                .bind(revoked_work.claim.project_id)
                .fetch_one(pool)
                .await
                .expect("the project");
        let run = tasks::get_task_run(pool, revoked_work.run)
            .await
            .expect("the run")
            .expect("the run exists");
        let task = task_state(pool, revoked_work.task).await;
        let draining = tasks::drain_task_run(pool, revoked_work.run, revoked_work.owner).await;
        let second = tasks::create_task_run_authorized(pool, writer, revoked_work.task).await;
        let runs = runs_of(pool, revoked_work.task).await;
        let state: (String, Option<Uuid>) =
            sqlx::query_as("SELECT state, state_changed_by FROM fleet_runners WHERE id = $1")
                .bind(revoked_runner)
                .fetch_one(pool)
                .await
                .expect("the revoked runner");

        let kept_heartbeat = tasks::heartbeat_task_run(pool, kept_work.run, kept_work.owner).await;
        let kept_renewed = kept_work.store.renew(&kept_work.lease, LIFETIME).await;
        let kept_project = auto_projects::renew(pool, kept_work.claim).await;
        let kept_state: String =
            sqlx::query_scalar("SELECT state FROM fleet_runners WHERE id = $1")
                .bind(kept_runner)
                .fetch_one(pool)
                .await
                .expect("the kept runner");

        remove(pool, &[revoked_runner, kept_runner]).await;
        tenant.cleanup().await;

        assert_eq!(previous, "active");
        assert_eq!(stopped, vec![revoked_work.run]);
        assert!(
            !heartbeat.expect("the heartbeat"),
            "the revoked owner heartbeat"
        );
        assert!(
            !plan.expect("the plan"),
            "the revoked owner recorded a plan"
        );
        assert!(
            matches!(renewed, Err(context::Error::LeaseLost)),
            "the revoked runner's lease renewed: {renewed:?}"
        );
        assert!(!project.expect("the renewal"), "the revoked drive renewed");
        assert_eq!(cleared, None);
        assert_eq!(run.status, tasks::CANCELLED);
        assert_eq!(run.error_message.as_deref(), Some(RUN_REVOKED));
        assert_eq!(task, (tasks::PAUSED.to_string(), Some(revoked_work.run)));
        assert!(
            draining.expect("the drain"),
            "the owner lost the reservation"
        );
        assert!(
            matches!(second, Ok(Mutation::Applied(RunMutation::Stopping))),
            "a second run was admitted while the owner drains: {second:?}"
        );
        assert_eq!(runs, 1, "a second run exists");
        assert_eq!(state, (State::Revoked.as_str().to_string(), Some(actor)));

        assert!(
            kept_heartbeat.expect("the heartbeat"),
            "the kept run lost its lease"
        );
        assert!(
            kept_renewed.is_ok(),
            "the kept lease was lost: {kept_renewed:?}"
        );
        assert!(
            kept_project.expect("the renewal"),
            "the kept drive lost its claim"
        );
        assert_eq!(kept_state, "active");
    }

    #[tokio::test]
    async fn the_owner_releases_a_revoked_runs_task_for_the_next_run() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let writer = tenant.workspace_holder(&[Permission::WorkspaceWrite]).await;
        let revoked_runner = runner(pool).await;
        let work = work(&tenant, writer, revoked_runner).await;
        let _ = revoke_committed(pool, revoked_runner, None)
            .await
            .expect("the runner exists")
            .publish();

        let released = tasks::release_task_run(pool, work.run, work.owner).await;
        let task = task_state(pool, work.task).await;
        let next = tasks::create_task_run_authorized(pool, writer, work.task).await;

        remove(pool, &[revoked_runner]).await;
        tenant.cleanup().await;

        assert!(
            released.expect("the release"),
            "the owner could not release"
        );
        assert_eq!(task, (tasks::PAUSED.to_string(), None));
        assert!(
            matches!(next, Ok(Mutation::Applied(RunMutation::Created(_)))),
            "the next run was not admitted: {next:?}"
        );
    }

    #[tokio::test]
    async fn recovery_frees_a_revoked_runs_task_once_its_owner_is_gone() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let writer = tenant.workspace_holder(&[Permission::WorkspaceWrite]).await;
        let revoked_runner = runner(pool).await;
        let work = work(&tenant, writer, revoked_runner).await;
        let _ = revoke_committed(pool, revoked_runner, None)
            .await
            .expect("the runner exists")
            .publish();

        recovery::reconcile(pool).await.expect("a reconcile");
        let draining = task_state(pool, work.task).await;
        sqlx::query(
            "UPDATE task_runs SET heartbeat_at = NOW() - INTERVAL '61 seconds' WHERE id = $1",
        )
        .bind(work.run)
        .execute(pool)
        .await
        .expect("the owner's lease lapses");
        let mut freed = task_state(pool, work.task).await;
        for _ in 0..50 {
            recovery::reconcile(pool).await.expect("a reconcile");
            freed = task_state(pool, work.task).await;
            if freed.1.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        remove(pool, &[revoked_runner]).await;
        tenant.cleanup().await;

        assert_eq!(draining, (tasks::PAUSED.to_string(), Some(work.run)));
        assert_eq!(freed, (tasks::PAUSED.to_string(), None));
    }

    #[tokio::test]
    async fn revoking_twice_keeps_the_first_change_and_an_unknown_runner_is_none() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let first = tenant.user().await;
        let second = tenant.user().await;
        let revoked_runner = runner(pool).await;

        let once = revoke_committed(pool, revoked_runner, Some(first))
            .await
            .expect("the runner exists")
            .publish();
        let twice = revoke_committed(pool, revoked_runner, Some(second))
            .await
            .expect("the runner exists");
        let previous = twice.previous.clone();
        let _ = twice.publish();
        let changed_by: Option<Uuid> =
            sqlx::query_scalar("SELECT state_changed_by FROM fleet_runners WHERE id = $1")
                .bind(revoked_runner)
                .fetch_one(pool)
                .await
                .expect("the runner");
        let unknown = revoke_committed(pool, Uuid::new_v4(), Some(first)).await;

        remove(pool, &[revoked_runner]).await;
        tenant.cleanup().await;

        assert!(once.is_empty(), "a runner with no work stopped runs");
        assert_eq!(previous, State::Revoked.as_str());
        assert_eq!(changed_by, Some(first));
        assert!(unknown.is_none(), "an unknown runner was revoked");
    }
}
