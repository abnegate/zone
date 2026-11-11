//! Every open auto project gets its turn, however many of them a pass has
//! nothing to do for. Each test owns its database: a tick claims across all
//! of one.

use std::sync::Arc;
use std::time::Duration;

use abnegate_vcs::PullRequestService;
use dashmap::DashMap;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::config::AutoProjectConfig;
use zone_server::db::auto_projects::{self, Kind, PlannedTask, ProjectPlan, Stage};
use zone_server::db::migrations;
use zone_server::db::tasks::{self, Origin};
use zone_server::state::AppState;
use zone_server::workers::auto_project::{self, PROJECTS_PER_TICK, Services};

use crate::common::{self, Database};

const PREFIX: &str = "auto_project_turns";
const DRIVES_SETTLE_WITHIN: Duration = Duration::from_secs(30);

struct Fixture {
    database: Database,
    state: AppState,
    services: Arc<Services>,
    config: Arc<AutoProjectConfig>,
    driving: Arc<DashMap<Uuid, ()>>,
    workspace: Uuid,
    actor: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        leave_runs_to_another_instance(&database.pool).await;
        let (_, workspace, actor) = common::setup_workspace_member(&database.pool).await;
        let state = common::create_test_state(common::test_config(), database.pool.clone());
        Self {
            database,
            state,
            services: Arc::new(Services::standing_in_for(
                PullRequestService::new().expect("GitHub's own origin"),
            )),
            config: Arc::new(AutoProjectConfig::default()),
            driving: Arc::new(DashMap::new()),
            workspace,
            actor,
        }
    }

    /// An auto project holding one agentic task, which is returned.
    async fn project(&self, name: &str) -> Uuid {
        let finalized = auto_projects::finalize(
            &self.database.pool,
            self.workspace,
            self.actor,
            None,
            Origin::Member,
            &ProjectPlan {
                name,
                description: None,
                repository: None,
                brief: &json!({}),
                tasks: &[PlannedTask {
                    kind: Kind::Feature,
                    title: format!("{name}'s task"),
                    description: "Ships a change.".into(),
                    acceptance_criteria: None,
                    depends_on: Vec::new(),
                    priority: None,
                }],
            },
        )
        .await
        .expect("an auto project");
        finalized.task_ids[0]
    }

    async fn pause(&self, task: Uuid) {
        let paused = tasks::update_task_authorized(
            &self.database.pool,
            self.actor,
            tasks::Patch {
                id: task,
                title: None,
                description: None,
                acceptance_criteria: None,
                status: tasks::Status::parse(tasks::PAUSED),
                priority: None,
                project_ids: None,
                require_plan_approval: None,
                is_agentic: None,
                source_id: None,
                model_name: None,
            },
        )
        .await;
        assert!(
            matches!(paused, Ok(tasks::Mutation::Applied(_))),
            "a person pauses the task: {paused:?}"
        );
    }

    /// One tick of the driver, and every drive it started through to its release.
    async fn tick(&self) {
        auto_project::tick(&self.state, &self.services, &self.config, &self.driving).await;
        tokio::time::timeout(DRIVES_SETTLE_WITHIN, async {
            while !self.driving.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("every claimed project is driven and released");
    }

    async fn unattended_runs(&self, task: Uuid) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM task_runs WHERE task_id = $1 AND unattended")
            .bind(task)
            .fetch_one(&self.database.pool)
            .await
            .unwrap()
    }

    async fn stopped(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects \
             WHERE auto_paused_reason IS NOT NULL OR auto_completed_at IS NOT NULL",
        )
        .fetch_one(&self.database.pool)
        .await
        .unwrap()
    }
}

/// Every run is owned from the moment it is admitted, as if another instance's
/// executor had claimed it, so the executor in this process declines it: the
/// driver admits runs here and nothing runs one.
async fn leave_runs_to_another_instance(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE FUNCTION claimed_elsewhere() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN NEW.owner := gen_random_uuid(); RETURN NEW; END $$; \
         CREATE TRIGGER claimed_elsewhere BEFORE INSERT ON task_runs \
         FOR EACH ROW EXECUTE FUNCTION claimed_elsewhere();",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn projects_held_by_a_person_do_not_keep_another_from_its_turn() {
    let fixture = Fixture::new().await;
    let mut held = Vec::new();
    for index in 0..PROJECTS_PER_TICK {
        let task = fixture.project(&format!("Held {index}")).await;
        fixture.pause(task).await;
        held.push(task);
    }
    let waiting = fixture.project("Waiting").await;

    fixture.tick().await;
    fixture.tick().await;

    let admitted = fixture.unattended_runs(waiting).await;
    let stage = auto_projects::task_stage(&fixture.database.pool, waiting)
        .await
        .unwrap()
        .map(|row| row.stage());
    let mut started_while_held = 0;
    for task in held {
        started_while_held += fixture.unattended_runs(task).await;
    }
    let stopped = fixture.stopped().await;
    let undriven: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE auto_driven_at IS NULL")
            .fetch_one(&fixture.database.pool)
            .await
            .unwrap();
    let claimed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE auto_claimed_at IS NOT NULL")
            .fetch_one(&fixture.database.pool)
            .await
            .unwrap();
    fixture.database.cleanup().await;

    assert_eq!(
        admitted, 1,
        "two ticks reach the project behind {PROJECTS_PER_TICK} that have nothing to start"
    );
    assert_eq!(stage, Some(Stage::Running));
    assert_eq!(started_while_held, 0, "a paused task is left alone");
    assert_eq!(stopped, 0, "a project waiting on a person stays open");
    assert_eq!(undriven, 0, "every project had a pass");
    assert_eq!(claimed, 0, "every pass gave its claim back");
}
