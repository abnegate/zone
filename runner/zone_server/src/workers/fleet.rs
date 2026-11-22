//! This server's part in its runner fleet: keeping its registration alive, and picking up task
//! runs that the runner that admitted them may not take.

use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::{Instant, Interval, MissedTickBehavior};
use uuid::Uuid;

use crate::db::fleet::candidate::Candidate;
use crate::db::fleet::eligibility;
use crate::services::fleet::{self, registration};
use crate::state::AppState;
use crate::workers::task;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
pub const PICKUP_INTERVAL: Duration = Duration::from_secs(5);

/// Runs [`heartbeats`] and [`pickups`] for as long as the server does.
pub fn spawn(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::join!(heartbeats(state.clone()), pickups(state));
    })
}

/// Registers this runner every [`HEARTBEAT_INTERVAL`], starting one interval in, since boot
/// registered it. Registering is an upsert, so a row removed while the runner was online returns.
async fn heartbeats(state: AppState) {
    let mut interval = every(HEARTBEAT_INTERVAL, Instant::now() + HEARTBEAT_INTERVAL);
    loop {
        interval.tick().await;
        heartbeat(&state).await;
    }
}

async fn pickups(state: AppState) {
    let mut interval = every(PICKUP_INTERVAL, Instant::now());
    loop {
        interval.tick().await;
        pick_up(&state).await;
    }
}

/// Starts an execution of each run this runner should pick up, as many as [`task::capacity`]
/// allows. An execution claims its run before it waits for a free slot and heartbeats while it
/// waits, so a busy runner still takes runs off one that may not run them.
async fn pick_up(state: &AppState) -> Vec<JoinHandle<()>> {
    pickup(state, task::capacity())
        .await
        .into_iter()
        .map(|candidate| {
            let state = state.clone();
            tokio::spawn(async move {
                task::execute_task_run(&state, candidate.run, candidate.task).await;
            })
        })
        .collect()
}

fn every(period: Duration, start: Instant) -> Interval {
    let mut interval = tokio::time::interval_at(start, period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval
}

/// Registers this runner, or refreshes its registration. A failure is logged rather than
/// returned, because the next heartbeat retries it: a server whose database was briefly
/// unreachable as it booted registers once it can.
pub async fn heartbeat(state: &AppState) -> Option<Uuid> {
    match registration::register(state).await {
        Ok(runner) => Some(runner),
        Err(error) => {
            tracing::warn!(%error, "Could not register as a runner; the next heartbeat retries");
            None
        }
    }
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
    use crate::db::fleet::state::State;
    use crate::db::fleet::{labels, policy, runners};
    use crate::db::tasks::{self, TaskRunRow};
    use crate::services::fleet::identity::version;
    use crate::services::fleet::tests::{Server, logged};
    use crate::state::{test_config, unreachable_pool};
    use crate::workers::task::handoff;

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
            self.claimed(run).await.0
        }

        /// The owner that claimed `run` and the runner it claimed it on.
        async fn claimed(&self, run: Uuid) -> (Option<Uuid>, Option<Uuid>) {
            sqlx::query_as("SELECT owner, runner_id FROM task_runs WHERE id = $1")
                .bind(run)
                .fetch_one(&self.database.pool)
                .await
                .unwrap()
        }

        /// Polls `run` until `done` holds of it, failing with where it stands if it never does.
        async fn until(&self, run: Uuid, done: impl Fn(&TaskRunRow, Option<Uuid>) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let row = self.run(run).await;
                let (_, runner) = self.claimed(run).await;
                if done(&row, runner) {
                    return;
                }
                assert!(Instant::now() < deadline, "the run never settled: {row:?}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        async fn cordon(&self, runner: Uuid) {
            sqlx::query("UPDATE fleet_runners SET state = $2 WHERE id = $1")
                .bind(runner)
                .bind(State::Cordoned.as_str())
                .execute(&self.database.pool)
                .await
                .unwrap();
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
    async fn a_heartbeat_registers_the_runner_again_after_its_row_was_removed() {
        let server = Server::new(None).await;
        let first = heartbeat(&server.state).await.expect("a registered runner");
        assert_eq!(heartbeat(&server.state).await, Some(first));
        sqlx::query("DELETE FROM fleet_runners WHERE id = $1")
            .bind(first)
            .execute(&server.database.pool)
            .await
            .unwrap();

        let returned = heartbeat(&server.state).await.expect("the runner again");

        assert_ne!(returned, first);
        assert_eq!(server.state.runner(), Some(returned));
        let row = runners::get(&server.database.pool, returned)
            .await
            .unwrap()
            .expect("the returned row");
        assert_eq!(row.name, server.state.identity().name);
        assert_eq!(row.labels, server.state.identity().labels);
        assert_eq!(row.instance, server.state.instance());
        server.remove().await;
    }

    #[tokio::test]
    async fn a_heartbeat_that_cannot_reach_the_database_is_left_to_the_next_one() {
        let state = AppState::new(test_config(), unreachable_pool(), None);
        state.disable_mcp();

        assert_eq!(heartbeat(&state).await, None);
        assert_eq!(state.runner(), None);
    }

    #[test]
    fn a_runner_heartbeats_well_inside_the_window_that_counts_it_connected() {
        let connected = runners::CONNECTED.to_std().unwrap();
        assert!(HEARTBEAT_INTERVAL * 3 < connected, "{HEARTBEAT_INTERVAL:?}");
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

    #[tokio::test]
    async fn a_busy_runner_still_picks_up_a_run_a_cordoned_runner_left_for_it() {
        let _execution = task::EXECUTION.lock().await;
        let server = Server::new(Some(Plan::Enterprise)).await;
        let cordoned = registration::register(&server.state).await.unwrap();
        server.cordon(cordoned).await;
        let busy = server.peer("");
        let taker = registration::register(&busy).await.unwrap();
        let (_, workspace) = server.workspace().await;
        let run = server.admit(workspace, 5.0).await;
        task::execute_task_run(&server.state, run.id, run.task_id).await;
        let left = server.claimed(run.id).await;

        let slots = task::saturate().await;
        let executions = pick_up(&busy).await;
        server
            .until(run.id, |_, runner| runner == Some(taker))
            .await;
        let queued = server.run(run.id).await;
        let (owner, _) = server.claimed(run.id).await;
        for execution in executions {
            execution.abort();
        }
        drop(slots);

        assert_eq!(left, (None, None), "the cordoned runner left the run");
        assert!(owner.is_some(), "the busy runner claimed the run");
        assert_eq!(
            queued.status, "running",
            "the claimed run waits for a slot rather than lapsing as orphaned"
        );
        server.remove().await;
    }

    #[test]
    fn a_run_left_for_another_runner_fails_once_none_claims_it_in_time() {
        let refused = Refusal::LabelsMissing {
            missing: Labels::parse_list("ssd").unwrap(),
        };
        let log = logged(async {
            let server = Server::new(Some(Plan::Enterprise)).await;
            let (organization, workspace) = server.workspace().await;
            server.require(organization, "ssd").await;
            runners::upsert(
                &server.database.pool,
                "gone",
                Uuid::new_v4(),
                &version(),
                &Labels::parse_list("ssd").unwrap(),
            )
            .await
            .unwrap();
            let waited = handoff::DEADLINE.as_seconds_f64() + 1.0;
            let run = server.admit(workspace, waited).await;

            task::execute_task_run(&server.state, run.id, run.task_id).await;
            server.until(run.id, |row, _| row.status != "running").await;

            let failed = server.run(run.id).await;
            let refusal = Refusal::NoRunner {
                minimum: None,
                labels: Labels::parse_list("ssd").unwrap(),
            };
            assert_eq!(failed.status, tasks::FAILED);
            assert_eq!(failed.error_message, Some(refusal.message()));
            assert_eq!(server.owner(run.id).await, None);
            assert_eq!(
                server.refusals().await,
                [Refused {
                    organization_id: Some(organization),
                    resource_type: resources::TASK.into(),
                    resource_id: Some(run.task_id),
                    new_values: Some(json!({
                        "code": refusal.code(),
                        "minimum": null,
                        "labels": ["ssd"],
                    })),
                }]
            );
            server.remove().await;
        });

        let failure = log
            .lines()
            .find(|line| line.contains("No runner in the fleet took the task run"))
            .unwrap_or_else(|| panic!("the fail is logged: {log}"));
        assert!(
            failure.contains(refused.code()) && failure.contains(&refused.to_string()),
            "the fail names why this runner refused the run: {failure}"
        );
    }
}
