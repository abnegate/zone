//! Revoking a fleet runner: it claims nothing more, and the work it holds ends.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::fleet::runners::undecodable;
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
    pub previous: State,
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

/// Revoke `runner` on `actor`'s behalf, and end the work it holds: its chat
/// leases expire, its auto-project claims are cleared, and each live task run
/// is stopped as a user's stop would, keeping its task reserved while its
/// owner drains. A cleared project stays unclaimable for one claim lease from
/// the revoke, so no other runner drives it while the revoked drive, whose
/// fenced writes and renewal already fail, winds down. `None` when there is no
/// such runner.
///
/// Lock order is `fleet_policy` → `fleet_runners` → `chat_leases` /
/// `projects` → `tasks` → `task_runs` → `chats`, the order chat writes
/// (`chat_leases`, then `chats`) and auto-project drives (`projects`, then
/// `tasks`) take them in. The caller must already hold `fleet_policy`
/// `FOR UPDATE` and no other lock. This locks the runner row, which waits for
/// every claim holding it `FOR SHARE`, so a claim that committed first is seen
/// and stopped here; then the runner's leases; then its projects in id order,
/// as `auto_projects::reopen` takes them; then each live run's task before the
/// run, and the chat it announces the stop in, in task id order.
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
    let previous = State::parse(&previous)
        .ok_or_else(|| undecodable("state", format!("\"{previous}\" is not a runner state")))?;
    sqlx::query(
        "UPDATE fleet_runners SET state = $2, state_changed_by = $3, state_changed_at = NOW() \
         WHERE id = $1 AND state <> $2",
    )
    .bind(runner)
    .bind(State::Revoked.as_str())
    .bind(actor)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "UPDATE chat_leases SET expires_at = clock_timestamp() \
         WHERE runner_id = $1 AND expires_at > clock_timestamp()",
    )
    .bind(runner)
    .execute(&mut *transaction)
    .await?;
    let driven: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE auto_claim_runner_id = $1 AND auto_claim_id IS NOT NULL \
         ORDER BY id FOR NO KEY UPDATE",
    )
    .bind(runner)
    .fetch_all(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE projects SET auto_claim_id = NULL, auto_claimed_at = statement_timestamp() \
         WHERE id = ANY($1)",
    )
    .bind(&driven)
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
    use crate::db::fleet::claimant::Claimant;
    use crate::db::recovery;
    use crate::db::tasks::{Mutation, RunMutation};
    use crate::db::testing::{Tenant, queued_behind};

    const LIFETIME: Duration = Duration::from_secs(30);

    struct Work {
        task: Uuid,
        run: Uuid,
        owner: Uuid,
        chat: Uuid,
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
    /// recorded against `runner`. The run's task belongs to the leased chat,
    /// so stopping the run announces it there.
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
        let claimant = Claimant {
            runner: Some(runner),
            instance: Uuid::nil(),
            enforced: false,
        };
        assert!(
            tasks::claim_task_run(pool, run.id, owner, claimant)
                .await
                .expect("the claim")
                .claimed(),
            "the run was not claimed"
        );

        let chat: Uuid = sqlx::query_scalar(
            "INSERT INTO chats (workspace_id, title, model_name) \
             VALUES ($1, 'Revocation', 'test') RETURNING id",
        )
        .bind(tenant.workspace)
        .fetch_one(pool)
        .await
        .expect("a chat");
        sqlx::query("UPDATE tasks SET chat_id = $2 WHERE id = $1")
            .bind(task.id)
            .bind(chat)
            .execute(pool)
            .await
            .expect("the task's chat");
        let store =
            Store::new(pool.clone(), chat, Some(tenant.workspace)).claiming(Claimant::UNENFORCED);
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
            chat,
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
        let previous = revoked.previous;
        let stopped: Vec<Uuid> = revoked.publish().iter().map(|run| run.id).collect();

        let heartbeat = tasks::heartbeat_task_run(pool, revoked_work.run, revoked_work.owner).await;
        let plan = tasks::record_run_plan(pool, revoked_work.run, revoked_work.owner, "plan").await;
        let renewed = revoked_work
            .store
            .renew(&revoked_work.lease, LIFETIME)
            .await;
        let project = auto_projects::renew(pool, revoked_work.claim, Claimant::UNENFORCED).await;
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
        let kept_project = auto_projects::renew(pool, kept_work.claim, Claimant::UNENFORCED).await;
        let kept_state: String =
            sqlx::query_scalar("SELECT state FROM fleet_runners WHERE id = $1")
                .bind(kept_runner)
                .fetch_one(pool)
                .await
                .expect("the kept runner");

        remove(pool, &[revoked_runner, kept_runner]).await;
        tenant.cleanup().await;

        assert_eq!(previous, State::Active);
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
        let previous = twice.previous;
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
        assert_eq!(previous, State::Revoked);
        assert_eq!(changed_by, Some(first));
        assert!(unknown.is_none(), "an unknown runner was revoked");
    }

    fn revoking(pool: &PgPool, runner: Uuid) -> tokio::task::JoinHandle<Vec<Uuid>> {
        let pool = pool.clone();
        tokio::spawn(async move {
            revoke_committed(&pool, runner, None)
                .await
                .expect("the runner exists")
                .publish()
                .iter()
                .map(|run| run.id)
                .collect()
        })
    }

    /// A chat write locks the chat's lease and then the chat. A revoke queued
    /// behind that lease, which belongs to the runner, holds no lock on the
    /// chat its stopped run is announced in, so the write finishes and then
    /// the revoke does.
    #[tokio::test]
    async fn a_revoke_waiting_on_a_chat_write_lets_the_write_finish() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let writer = tenant.workspace_holder(&[Permission::WorkspaceWrite]).await;
        let revoked_runner = runner(pool).await;
        let work = work(&tenant, writer, revoked_runner).await;

        let mut write = pool.begin().await.expect("a chat write");
        let backend: i32 = sqlx::query_scalar(
            "SELECT pg_backend_pid() FROM chat_leases WHERE chat_id = $1 FOR UPDATE",
        )
        .bind(work.chat)
        .fetch_one(&mut *write)
        .await
        .expect("the chat's lease");
        let revoke = revoking(pool, revoked_runner);
        queued_behind(pool, backend).await;
        let touched = sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
            .bind(work.chat)
            .execute(&mut *write)
            .await
            .map(|_| ());
        let committed = write.commit().await;
        let stopped = revoke.await;

        remove(pool, &[revoked_runner]).await;
        tenant.cleanup().await;

        assert!(
            touched.is_ok() && committed.is_ok(),
            "the chat write failed behind the revoke: {touched:?} {committed:?}"
        );
        assert_eq!(
            stopped.expect("the revoke completes"),
            vec![work.run],
            "the revoke stopped the runner's run"
        );
    }

    /// An auto-project drive locks its project's row and then a task, as it
    /// does to admit the task's next run. A revoke queued behind that project,
    /// which the runner drives, holds no lock on the task whose live run it
    /// stops, so the drive finishes and then the revoke does.
    #[tokio::test]
    async fn a_revoke_waiting_on_a_drive_lets_the_drive_finish() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let writer = tenant.workspace_holder(&[Permission::WorkspaceWrite]).await;
        let revoked_runner = runner(pool).await;
        let work = work(&tenant, writer, revoked_runner).await;

        let mut drive = pool.begin().await.expect("a drive's admission");
        let backend: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *drive)
            .await
            .expect("the drive's session");
        let held = auto_projects::holds(&mut drive, work.claim)
            .await
            .expect("the project's claim");
        let revoke = revoking(pool, revoked_runner);
        queued_behind(pool, backend).await;
        let task: Result<String, _> =
            sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1 FOR UPDATE")
                .bind(work.task)
                .fetch_one(&mut *drive)
                .await;
        let active: Result<bool, _> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT EXISTS(SELECT 1 FROM task_runs \
             WHERE task_id = $1 AND status IN {ACTIVE_RUN_STATUSES})"
        )))
        .bind(work.task)
        .fetch_one(&mut *drive)
        .await;
        let committed = drive.commit().await;
        let stopped = revoke.await;

        remove(pool, &[revoked_runner]).await;
        tenant.cleanup().await;

        assert!(held, "the drive held its project");
        assert!(
            task.is_ok() && committed.is_ok(),
            "the drive failed behind the revoke: {task:?} {committed:?}"
        );
        assert!(
            active.expect("the task's runs"),
            "the drive saw the live run"
        );
        assert_eq!(
            stopped.expect("the revoke completes"),
            vec![work.run],
            "the revoke stopped the runner's run"
        );
    }

    /// Two projects the runner drives, the later id stored first, so a revoke
    /// that locked them in storage order would take the later one first.
    async fn driven_in_reverse(tenant: &Tenant, runner: Uuid) -> (Uuid, Uuid) {
        let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
        ids.sort();
        let [first, second] = ids;
        sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, auto, auto_claim_id, auto_claimed_at, \
               auto_claim_runner_id) \
             SELECT id, $1, 'Revocation', TRUE, gen_random_uuid(), NOW(), $2 \
             FROM unnest($3::uuid[]) WITH ORDINALITY AS stored(id, position) ORDER BY position",
        )
        .bind(tenant.workspace)
        .bind(runner)
        .bind(vec![second, first])
        .execute(&tenant.pool)
        .await
        .expect("two claimed projects");
        (first, second)
    }

    /// `auto_projects::reopen` locks projects in id order. A revoke queued
    /// behind the first of two projects the runner drives holds no lock on the
    /// second, so the reopen takes the second and finishes, and then the revoke
    /// clears both claims.
    #[tokio::test]
    async fn a_revoke_waiting_on_a_reopen_lets_the_reopen_finish() {
        let tenant = Tenant::new().await;
        let pool = &tenant.pool;
        let revoked_runner = runner(pool).await;
        let (first, second) = driven_in_reverse(&tenant, revoked_runner).await;
        let stored_in_reverse: bool = sqlx::query_scalar(
            "SELECT (SELECT ctid FROM projects WHERE id = $2) < \
                    (SELECT ctid FROM projects WHERE id = $1)",
        )
        .bind(first)
        .bind(second)
        .fetch_one(pool)
        .await
        .expect("where the projects are stored");

        let mut reopen = pool.begin().await.expect("a reopen");
        let backend: i32 = sqlx::query_scalar(
            "SELECT pg_backend_pid() FROM projects WHERE id = $1 FOR NO KEY UPDATE",
        )
        .bind(first)
        .fetch_one(&mut *reopen)
        .await
        .expect("the first project");
        let revoke = revoking(pool, revoked_runner);
        queued_behind(pool, backend).await;
        let locked = sqlx::query("SELECT id FROM projects WHERE id = $1 FOR NO KEY UPDATE")
            .bind(second)
            .execute(&mut *reopen)
            .await
            .map(|_| ());
        let committed = reopen.commit().await;
        let revoked = revoke.await;
        let claims: Vec<Option<Uuid>> =
            sqlx::query_scalar("SELECT auto_claim_id FROM projects WHERE id = ANY($1)")
                .bind(vec![first, second])
                .fetch_all(pool)
                .await
                .expect("the projects");

        sqlx::query("DELETE FROM projects WHERE id = ANY($1)")
            .bind(vec![first, second])
            .execute(pool)
            .await
            .expect("the projects are removed");
        remove(pool, &[revoked_runner]).await;
        tenant.cleanup().await;

        assert!(stored_in_reverse, "the later project was not stored first");
        assert!(
            locked.is_ok() && committed.is_ok(),
            "the reopen failed behind the revoke: {locked:?} {committed:?}"
        );
        assert!(revoked.is_ok(), "the revoke failed: {revoked:?}");
        assert_eq!(claims, vec![None, None], "a project kept its claim");
    }
}
