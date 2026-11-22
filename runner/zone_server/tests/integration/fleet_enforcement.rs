//! Runner fleet policy end to end: several server processes share one database, and each claims
//! task runs, chat turns, reminder turns and auto-project drives through its own production path.
//! Under an Enterprise license a cordoned, revoked, outdated or unlabelled runner takes nothing it
//! may not, the work goes to a runner that may, and a revoke ends what the runner holds; without
//! one, none of it applies.

use std::sync::Arc;
use std::time::Duration;

use abnegate_vcs::PullRequestService;
use axum::http::StatusCode;
use dashmap::DashMap;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as SocketMessage;
use uuid::Uuid;
use zone_enterprise::fleet::{
    Labels, NO_RUNNER, RUNNER_OUTDATED, RUNNER_REVOKED, Refusal, Version,
};
use zone_server::config::AutoProjectConfig;
use zone_server::db::audit::{actions, resources};
use zone_server::db::auto_projects::{self, Kind, PlannedTask, ProjectPlan, Stage, WriteError};
use zone_server::db::context::Error as ContextError;
use zone_server::db::fleet::revocation::{self, RUN_REVOKED};
use zone_server::db::fleet::shortfall::Shortfall;
use zone_server::db::fleet::state::State;
use zone_server::db::fleet::{policy, runners};
use zone_server::db::reminders::{self, Reminder, Sweep};
use zone_server::db::tasks::{self, Origin};
use zone_server::services::chat::session::Session;
use zone_server::services::fleet::{self, identity::version};
use zone_server::state::AppState;
use zone_server::workers::auto_project::driver::{Admission, Drive};
use zone_server::workers::auto_project::{self, CLAIM_LEASE, PROJECTS_PER_TICK, Services, lease};

use crate::common::context::{Socket, successful};
use crate::common::fleet::{
    ANSWER, COMPLETED, FAILED, Fleet, RUNNING, Spec, WAITING, refused_with,
};
use crate::common::sso::{active, lapsed, team};

const PREFIX: &str = "fleet_enforcement";
const RACES: usize = 50;
const PROMPT: &str = "Report any dependency PR that has been red for a week.";
const OUTDATED: &str = "0.0.1";
const UNREACHABLE: &str = "9.0.0";
const LIVE: Duration = Duration::from_secs(3);
const HUNG: Duration = Duration::from_secs(120);
const SETTLES_WITHIN: Duration = Duration::from_secs(60);
const IDLE_FOR: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(50);
const RENEWAL: Duration = Duration::from_millis(100);
const STAGGERS: usize = 25;

/// How long race `race` holds back its claim, so across the races some claims land before the
/// revoke and some after it.
fn stagger(race: usize) -> Duration {
    Duration::from_millis(u64::try_from(race % STAGGERS).unwrap_or_default())
}

/// This build's version, which a runner on it meets as a minimum.
fn current() -> String {
    version().to_string()
}

fn outdated() -> Version {
    Version::parse(OUTDATED).unwrap()
}

fn no_runner(minimum: Option<&str>, labels: &str) -> Refusal {
    Refusal::NoRunner {
        minimum: minimum.map(|minimum| Version::parse(minimum).unwrap()),
        labels: Labels::parse_list(labels).unwrap(),
    }
}

/// An auto project in `workspace` holding one agentic task, and that task.
async fn auto_project(fleet: &Fleet, workspace: Uuid) -> (Uuid, Uuid) {
    let finalized = auto_projects::finalize(
        fleet.pool(),
        workspace,
        fleet.admin.id,
        None,
        Origin::Member,
        &ProjectPlan {
            name: "Fleet",
            description: None,
            repository: None,
            brief: &json!({}),
            tasks: &[PlannedTask {
                kind: Kind::Feature,
                title: "Ship it".into(),
                description: "Ships a change.".into(),
                acceptance_criteria: None,
                depends_on: Vec::new(),
                priority: None,
            }],
        },
    )
    .await
    .expect("an auto project");
    (finalized.project_id, finalized.task_ids[0])
}

/// The drives one server runs: its tick claims what it may and drives each claim to its
/// release.
struct Driver {
    state: AppState,
    services: Arc<Services>,
    config: Arc<AutoProjectConfig>,
    driving: Arc<DashMap<Uuid, ()>>,
}

impl Driver {
    fn of(state: &AppState) -> Self {
        Self {
            state: state.clone(),
            services: Arc::new(Services::standing_in_for(
                PullRequestService::new().expect("GitHub's own origin"),
            )),
            config: Arc::new(AutoProjectConfig::default()),
            driving: Arc::new(DashMap::new()),
        }
    }

    async fn tick(&self) {
        auto_project::tick(&self.state, &self.services, &self.config, &self.driving).await;
        tokio::time::timeout(SETTLES_WITHIN, async {
            while !self.driving.is_empty() {
                tokio::time::sleep(POLL).await;
            }
        })
        .await
        .expect("every claimed project is driven and released");
    }
}

async fn runs_of(fleet: &Fleet, task: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM task_runs WHERE task_id = $1 ORDER BY started_at, id")
        .bind(task)
        .fetch_all(fleet.pool())
        .await
        .unwrap()
}

async fn driven(fleet: &Fleet, project: Uuid) -> bool {
    sqlx::query_scalar("SELECT auto_driven_at IS NOT NULL FROM projects WHERE id = $1")
        .bind(project)
        .fetch_one(fleet.pool())
        .await
        .unwrap()
}

/// The claim on `project` and the runner it was taken on.
async fn project_claim(fleet: &Fleet, project: Uuid) -> (Option<Uuid>, Option<Uuid>) {
    sqlx::query_as("SELECT auto_claim_id, auto_claim_runner_id FROM projects WHERE id = $1")
        .bind(project)
        .fetch_one(fleet.pool())
        .await
        .unwrap()
}

/// A prompt reminder in the fixture's chat whose firing is owed as a turn.
async fn owed_reminder(fleet: &Fleet) -> Uuid {
    let created = reminders::create(
        fleet.pool(),
        fleet.workspace,
        fleet.admin.id,
        fleet.chat,
        Reminder {
            content: "Dependency check".into(),
            due_at: (chrono::Utc::now() + chrono::Duration::hours(1)).fixed_offset(),
            rrule: None,
            prompt: Some(PROMPT.into()),
            timing_mode: None,
            time_zone: None,
        },
    )
    .await
    .expect("a prompt reminder");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(reminder)
        .execute(fleet.pool())
        .await
        .unwrap();
    reminders::deliver_next(fleet.pool(), Sweep::Workspace(fleet.workspace))
        .await
        .expect("the due firing is written down as an owed turn");
    reminder
}

/// The turns `reminder` still owes, as how many claims each has had and whether one holds it.
async fn owed(fleet: &Fleet, reminder: Uuid) -> Vec<(i32, bool)> {
    sqlx::query_as(
        "SELECT attempts, claimed_at IS NOT NULL FROM reminder_turns WHERE reminder_id = $1",
    )
    .bind(reminder)
    .fetch_all(fleet.pool())
    .await
    .unwrap()
}

/// Waits for `reminder` to owe nothing more and the chat to hold an answer.
async fn until_answered(fleet: &Fleet, reminder: Uuid) {
    let deadline = tokio::time::Instant::now() + SETTLES_WITHIN;
    loop {
        let answered = fleet
            .messages()
            .await
            .iter()
            .any(|(role, content)| role == "assistant" && content == ANSWER);
        if answered && owed(fleet, reminder).await.is_empty() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the firing was never answered: owed {:?}, chat {:?}",
            owed(fleet, reminder).await,
            fleet.messages().await
        );
        tokio::time::sleep(POLL).await;
    }
}

/// Every frame `socket` receives within `window`.
async fn frames_within(socket: &mut Socket, window: Duration) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + window;
    let mut frames = Vec::new();
    while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, socket.next()).await {
        if let SocketMessage::Text(text) = message {
            frames.push(serde_json::from_str(&text).unwrap());
        }
    }
    frames
}

/// Revokes `runner` as a revoke on another server process does: committed there, with no stop
/// signal reaching the process that runs the work.
async fn revoke_elsewhere(fleet: &Fleet, runner: Uuid) {
    let mut transaction = fleet.pool().begin().await.unwrap();
    policy::lock(&mut transaction).await.unwrap();
    let revoked = revocation::revoke(&mut transaction, runner, Some(fleet.admin.id))
        .await
        .unwrap()
        .expect("the runner");
    transaction.commit().await.unwrap();
    let _ = revoked.publish();
}

#[tokio::test]
async fn a_revoked_runner_hands_its_run_to_an_active_one() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet.set_state(a.id, State::Revoked).await;
    let run = fleet.admit_in(fleet.workspace).await;

    fleet.execute(&a.state, &run).await;
    let left = (fleet.run(run.id).await.status, fleet.claimed(run.id).await);
    fleet.settle(run.id).await;
    let pickup = zone_server::workers::fleet::spawn(b.state.clone());
    let finished = fleet.settles_on(run.id, COMPLETED).await;
    pickup.abort();
    let (_, runner) = fleet.claimed(run.id).await;
    fleet.cleanup().await;

    assert_eq!(
        left,
        (RUNNING.to_string(), (None, None)),
        "a revoked runner claims nothing and leaves the run for another runner"
    );
    assert_eq!(finished.error_message, None);
    assert_eq!(runner, Some(b.id), "the active runner picked the run up");
}

#[tokio::test]
async fn a_cordoned_runner_takes_no_new_run_but_finishes_its_live_one() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet.hold(LIVE);
    let live = fleet.admit_in(fleet.workspace).await;
    let running = fleet.spawn(&a.state, &live);
    fleet.until_claimed(live.id).await;

    fleet.set_state(a.id, State::Cordoned).await;
    let next = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &next).await;
    let refused = fleet.claimed(next.id).await;
    tokio::time::timeout(SETTLES_WITHIN, running)
        .await
        .expect("the live run finishes")
        .unwrap();
    let finished = fleet.run(live.id).await;
    let (_, finished_on) = fleet.claimed(live.id).await;
    fleet.hold(Duration::ZERO);
    fleet.settle(next.id).await;
    let offered = (fleet.candidates(a.id).await, fleet.candidates(b.id).await);
    fleet.execute(&b.state, &next).await;
    let handed = fleet.run(next.id).await;
    let (_, handed_to) = fleet.claimed(next.id).await;
    fleet.cleanup().await;

    assert_eq!(refused, (None, None), "a cordoned runner claims no new run");
    assert_eq!(
        finished.status, COMPLETED,
        "the run it held when cordoned finishes: {:?}",
        finished.error_message
    );
    assert_eq!(finished_on, Some(a.id));
    assert_eq!(offered, (Vec::new(), vec![next.id]));
    assert_eq!(handed.status, COMPLETED, "{:?}", handed.error_message);
    assert_eq!(handed_to, Some(b.id));
}

#[tokio::test]
async fn an_outdated_runner_is_refused_and_the_run_fails_when_no_runner_is_current() {
    let fleet = Fleet::new(PREFIX, &[Spec::on(OUTDATED), Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let a = fleet.runner(0).clone();
    let (elsewhere, elsewhere_workspace) = fleet.another_organization().await;

    fleet.set_minimum(Some(&current())).await;
    let handed = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &handed).await;
    let left = (
        fleet.run(handed.id).await.status,
        fleet.claimed(handed.id).await,
    );

    fleet.set_minimum(Some(UNREACHABLE)).await;
    let stranded = fleet.admit_in(elsewhere_workspace).await;
    fleet.execute(&a.state, &stranded).await;
    let failed = fleet.run(stranded.id).await;
    let claimed = fleet.claimed(stranded.id).await;
    let audits = fleet.audits(actions::FLEET_RUN_REFUSED).await;
    fleet.cleanup().await;

    assert_eq!(
        left,
        (RUNNING.to_string(), (None, None)),
        "an outdated runner leaves the run to a current one"
    );
    let refusal = no_runner(Some(UNREACHABLE), "");
    assert_eq!(failed.status, FAILED);
    assert_eq!(failed.error_message, Some(refusal.message()));
    assert!(refusal.message().contains(UNREACHABLE));
    assert_eq!(claimed, (None, None));
    let [audit] = audits.as_slice() else {
        panic!("the refusal was audited {} times", audits.len());
    };
    assert_eq!(
        audit.organization_id,
        Some(elsewhere),
        "the refusal is recorded in the run's own organization"
    );
    assert_eq!(audit.actor_id, None);
    assert_eq!(audit.resource_type, resources::TASK);
    assert_eq!(audit.resource_id, Some(stranded.task_id));
    assert_eq!(
        audit.new_values,
        Some(json!({"code": NO_RUNNER, "minimum": UNREACHABLE, "labels": []}))
    );
}

#[tokio::test]
async fn workspace_labels_place_runs_on_labelled_runners() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::labelled("gpu")]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet
        .require_workspace(fleet.organization, fleet.workspace, &["gpu"])
        .await;
    let unplaced = fleet.another_workspace(fleet.organization).await;

    let placed = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &placed).await;
    let left = fleet.claimed(placed.id).await;
    let free = fleet.admit_in(unplaced).await;
    fleet.execute(&a.state, &free).await;
    let free_status = fleet.run(free.id).await.status;
    let (_, free_on) = fleet.claimed(free.id).await;
    fleet.settle(placed.id).await;
    let offered = (fleet.candidates(a.id).await, fleet.candidates(b.id).await);
    fleet.execute(&b.state, &placed).await;
    let placed_status = fleet.run(placed.id).await.status;
    let (_, placed_on) = fleet.claimed(placed.id).await;
    fleet.cleanup().await;

    assert_eq!(left, (None, None), "an unlabelled runner leaves the run");
    assert_eq!(offered, (Vec::new(), vec![placed.id]));
    assert_eq!(placed_status, COMPLETED);
    assert_eq!(placed_on, Some(b.id), "the labelled runner ran it");
    assert_eq!(free_status, COMPLETED);
    assert_eq!(
        free_on,
        Some(a.id),
        "a workspace that requires nothing runs anywhere"
    );
}

#[tokio::test]
async fn a_process_sharing_a_runners_name_is_judged_on_its_own_labels() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let unlabelled = fleet.runner(0).clone();
    fleet
        .require_workspace(fleet.organization, fleet.workspace, &["gpu"])
        .await;
    let labelled = fleet.twin(0, Spec::labelled("gpu")).await;

    let run = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&unlabelled.state, &run).await;
    let left = (fleet.run(run.id).await.status, fleet.claimed(run.id).await);
    fleet.execute(&labelled, &run).await;
    let ran = fleet.run(run.id).await.status;
    let (_, ran_on) = fleet.claimed(run.id).await;
    fleet.cleanup().await;

    assert_eq!(labelled.runner(), Some(unlabelled.id), "both share the row");
    assert_eq!(
        left,
        (RUNNING.to_string(), (None, None)),
        "the unlabelled process claimed labelled work on the other process's registration"
    );
    assert_eq!(ran, COMPLETED, "the labelled process takes the run it left");
    assert_eq!(ran_on, Some(unlabelled.id));
}

#[tokio::test]
async fn organization_and_workspace_labels_combine() {
    let fleet = Fleet::new(
        PREFIX,
        &[
            Spec::labelled("region=eu"),
            Spec::labelled("gpu"),
            Spec::labelled("gpu,region=eu"),
        ],
    )
    .await;
    fleet.license(&active()).await;
    let (a, b, c) = (
        fleet.runner(0).clone(),
        fleet.runner(1).clone(),
        fleet.runner(2).clone(),
    );
    fleet
        .require_organization(fleet.organization, &["region=eu"])
        .await;
    fleet
        .require_workspace(fleet.organization, fleet.workspace, &["gpu"])
        .await;
    let organization_only = fleet.another_workspace(fleet.organization).await;

    let both = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &both).await;
    fleet.execute(&b.state, &both).await;
    let left = fleet.claimed(both.id).await;
    fleet.settle(both.id).await;
    let offered = [
        fleet.candidates(a.id).await,
        fleet.candidates(b.id).await,
        fleet.candidates(c.id).await,
    ];
    fleet.execute(&c.state, &both).await;
    let both_status = fleet.run(both.id).await.status;
    let (_, both_on) = fleet.claimed(both.id).await;

    let unlabelled_region = fleet.admit_in(organization_only).await;
    fleet.execute(&b.state, &unlabelled_region).await;
    let refused = fleet.claimed(unlabelled_region.id).await;
    let region = fleet.admit_in(organization_only).await;
    fleet.execute(&a.state, &region).await;
    let region_status = fleet.run(region.id).await.status;
    let (_, region_on) = fleet.claimed(region.id).await;
    fleet.cleanup().await;

    assert_eq!(
        left,
        (None, None),
        "neither the organization's labels nor the workspace's alone place the run"
    );
    assert_eq!(offered, [Vec::new(), Vec::new(), vec![both.id]]);
    assert_eq!(both_status, COMPLETED);
    assert_eq!(
        both_on,
        Some(c.id),
        "the runner carrying both labels ran it"
    );
    assert_eq!(
        refused,
        (None, None),
        "the organization's labels apply to every workspace in it"
    );
    assert_eq!(region_status, COMPLETED);
    assert_eq!(region_on, Some(a.id));
}

#[tokio::test]
async fn another_organizations_labels_never_apply() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let a = fleet.runner(0).clone();
    let (elsewhere, elsewhere_workspace) = fleet.another_organization().await;
    fleet.require_organization(elsewhere, &["gpu"]).await;
    fleet
        .require_workspace(elsewhere, elsewhere_workspace, &["ssd"])
        .await;

    let ours = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &ours).await;
    let ours_status = fleet.run(ours.id).await.status;
    let (_, ours_on) = fleet.claimed(ours.id).await;
    let theirs = fleet.admit_in(elsewhere_workspace).await;
    fleet.execute(&a.state, &theirs).await;
    let theirs = fleet.run(theirs.id).await;
    let audits = fleet.audits(actions::FLEET_RUN_REFUSED).await;
    fleet.cleanup().await;

    assert_eq!(
        ours_status, COMPLETED,
        "another organization's labels place nothing here"
    );
    assert_eq!(ours_on, Some(a.id));
    assert_eq!(theirs.status, FAILED);
    assert_eq!(
        theirs.error_message,
        Some(no_runner(None, "gpu,ssd").message())
    );
    let organizations: Vec<_> = audits.iter().map(|audit| audit.organization_id).collect();
    assert_eq!(organizations, [Some(elsewhere)]);
}

#[tokio::test]
async fn revoking_stops_a_live_run_keeps_its_task_reserved_and_lands_no_fenced_write() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let a = fleet.runner(0).clone();
    fleet.hold(LIVE);
    let run = fleet.admit_in(fleet.workspace).await;
    let running = fleet.spawn(&a.state, &run);
    let (owner, claimed_on) = fleet.until_claimed(run.id).await;

    revoke_elsewhere(&fleet, a.id).await;
    let stopped = fleet.run(run.id).await;
    let reserved = fleet.reservation(run.task_id).await;
    let readmitted = fleet
        .client
        .post_json_auth(
            &format!("/api/tasks/{}/runs", run.task_id),
            &json!({}),
            &fleet.admin.token,
        )
        .await;
    let renewed = tasks::heartbeat_task_run(fleet.pool(), run.id, owner)
        .await
        .unwrap();
    tokio::time::timeout(SETTLES_WITHIN, running)
        .await
        .expect("the revoked run's owner ends")
        .unwrap();
    let ended = fleet.run(run.id).await;
    let released = fleet.reservation(run.task_id).await;
    let asked = fleet.streamed().await;
    let runs = runs_of(&fleet, run.task_id).await;
    fleet.cleanup().await;

    assert_eq!(claimed_on, a.id);
    assert_eq!(stopped.status, tasks::CANCELLED);
    assert_eq!(stopped.error_message.as_deref(), Some(RUN_REVOKED));
    assert_eq!(
        reserved,
        (tasks::PAUSED.to_string(), Some(run.id)),
        "the task stays reserved while the revoked run's owner drains"
    );
    readmitted.assert_status(StatusCode::CONFLICT);
    assert_eq!(readmitted.json_value(), json!({"error": tasks::STOPPING}));
    assert!(!renewed, "the revoked owner's heartbeat does not land");
    assert!(asked >= 1, "the run's turn was answered");
    assert_eq!(
        (ended.status.as_str(), ended.error_message.as_deref()),
        (tasks::CANCELLED, Some(RUN_REVOKED)),
        "the answer the revoked owner went on to record did not land"
    );
    assert_eq!(ended.artifacts, stopped.artifacts);
    assert_eq!(
        released,
        (tasks::PAUSED.to_string(), None),
        "the owner gave the task back once it let go"
    );
    assert_eq!(runs, [run.id], "no second run started while it drained");
}

#[tokio::test]
async fn a_revoke_racing_a_claim_leaves_no_live_run_on_the_revoked_runner() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let a = fleet.runner(0).clone();
    fleet.hold(HUNG);

    for race in 0..RACES {
        fleet.heartbeat().await;
        let run = fleet.admit_in(fleet.workspace).await;
        tokio::join!(
            async {
                tokio::time::sleep(stagger(race)).await;
                fleet.execute(&a.state, &run).await;
            },
            fleet.set_state(a.id, State::Revoked)
        );
        let live = fleet.live_runs(a.id).await;
        let ended = fleet.run(run.id).await;
        let claimed = fleet.claimed(run.id).await;
        assert_eq!(
            live, 0,
            "race {race}: a run stayed live on the revoked runner"
        );
        match claimed {
            (Some(_), Some(runner)) => {
                assert_eq!(runner, a.id, "race {race}");
                assert_eq!(
                    (ended.status.as_str(), ended.error_message.as_deref()),
                    (tasks::CANCELLED, Some(RUN_REVOKED)),
                    "race {race}: a run claimed first is stopped by the revoke"
                );
            }
            (None, None) => assert_eq!(
                ended.status, RUNNING,
                "race {race}: a run the revoke refused is left for another runner"
            ),
            other => panic!("race {race}: the run was half claimed: {other:?}"),
        }
        fleet.set_state(a.id, State::Active).await;
    }
    fleet.cleanup().await;
}

#[tokio::test]
async fn a_revoke_racing_a_chat_acquire_leaves_no_live_lease() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let a = fleet.runner(0).clone();

    for race in 0..RACES {
        fleet.heartbeat().await;
        let (acquired, _) = tokio::join!(
            async {
                tokio::time::sleep(stagger(race)).await;
                Session::acquire(&a.state, fleet.chat, fleet.workspace, Uuid::new_v4()).await
            },
            fleet.set_state(a.id, State::Revoked)
        );
        let live = fleet.live_leases(a.id).await;
        let refused = match acquired {
            Ok(mut session) => {
                let _ = session.close().await;
                false
            }
            Err(ContextError::Refused(Shortfall::Revoked)) => true,
            Err(error) => panic!("race {race}: the acquire failed otherwise: {error}"),
        };
        assert_eq!(
            live, 0,
            "race {race}: a lease stayed live on the revoked runner (refused: {refused})"
        );
        fleet.set_state(a.id, State::Active).await;
    }
    fleet.cleanup().await;
}

#[tokio::test]
async fn two_pickups_of_one_run_claim_it_once() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b, c) = (
        fleet.runner(0).clone(),
        fleet.runner(1).clone(),
        fleet.runner(2).clone(),
    );
    fleet.set_state(a.id, State::Cordoned).await;
    let run = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &run).await;
    let left = fleet.claimed(run.id).await;
    fleet.settle(run.id).await;
    let offered = (fleet.candidates(b.id).await, fleet.candidates(c.id).await);

    let pickups = [
        zone_server::workers::fleet::spawn(b.state.clone()),
        zone_server::workers::fleet::spawn(c.state.clone()),
    ];
    fleet.settles_on(run.id, COMPLETED).await;
    for pickup in pickups {
        pickup.abort();
    }
    let (_, runner) = fleet.claimed(run.id).await;
    let asked = fleet.streamed().await;
    fleet.cleanup().await;

    assert_eq!(left, (None, None));
    assert_eq!(
        offered,
        (vec![run.id], vec![run.id]),
        "both runners may take it"
    );
    assert!(
        runner == Some(b.id) || runner == Some(c.id),
        "a picking runner claimed it: {runner:?}"
    );
    assert_eq!(asked, 1, "only one runner ran it");
}

#[tokio::test]
async fn a_resumed_run_keeps_its_runner_and_the_next_run_is_checked_again() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet.call(
        "ask_user",
        json!({"questions": [{
            "header": "Scope",
            "question": "How far back should the fix reach?",
            "options": [
                {"label": "Backfill", "description": "Repair every existing row"},
                {"label": "Forward only", "description": "Leave history alone"}
            ]
        }]}),
    );
    let task = fleet.task(fleet.workspace).await;
    let first = fleet.admit(task).await;
    let running = fleet.spawn(&a.state, &first);
    fleet.settles_on(first.id, WAITING).await;
    let parked = fleet.claimed(first.id).await;

    fleet.set_state(a.id, State::Cordoned).await;
    let answered = fleet
        .client
        .post_json_auth(
            &format!("/api/tasks/runs/{}/answers", first.id),
            &json!({"answers": [{"header": "Scope", "labels": ["Forward only"]}]}),
            &fleet.admin.token,
        )
        .await;
    tokio::time::timeout(SETTLES_WITHIN, running)
        .await
        .expect("the resumed run finishes")
        .unwrap();
    let resumed = fleet.run(first.id).await;
    let finished = fleet.claimed(first.id).await;
    let next = fleet.admit(task).await;
    fleet.execute(&a.state, &next).await;
    let refused = fleet.claimed(next.id).await;
    fleet.settle(next.id).await;
    fleet.execute(&b.state, &next).await;
    let handed = fleet.run(next.id).await;
    let (_, handed_to) = fleet.claimed(next.id).await;
    fleet.cleanup().await;

    assert_eq!(parked.1, Some(a.id));
    answered.assert_status(StatusCode::ACCEPTED);
    assert_eq!(resumed.status, COMPLETED, "{:?}", resumed.error_message);
    assert_eq!(
        finished, parked,
        "the resumed run keeps the owner and runner it claimed on"
    );
    assert_eq!(
        refused,
        (None, None),
        "the task's next run is checked against the runner again"
    );
    assert_eq!(handed.status, COMPLETED, "{:?}", handed.error_message);
    assert_eq!(handed_to, Some(b.id));
}

#[tokio::test]
async fn a_reminder_turn_is_left_owed_on_a_revoked_runner_and_runs_on_an_active_one() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    let sweep = Sweep::Workspace(fleet.workspace);
    let lease = zone_server::workers::reminders::Lease::default();
    let reminder = owed_reminder(&fleet).await;
    let claimant = fleet::claimant(&a.state).await;
    let turn = reminders::claim_turn(fleet.pool(), sweep, lease.duration, claimant)
        .await
        .unwrap()
        .expect("the owed turn is offered to an active runner");

    fleet.set_state(a.id, State::Revoked).await;
    let mut watching = fleet.connect(&fleet.serve(&a.state).await).await;
    zone_server::workers::reminders::run(&a.state, turn.clone(), lease, claimant).await;
    let told = frames_within(&mut watching, IDLE_FOR).await;
    let refused = owed(&fleet, reminder).await;
    let unanswered = fleet.messages().await;
    let reclaimed = reminders::claim_turn(
        fleet.pool(),
        sweep,
        lease.duration,
        fleet::claimant(&a.state).await,
    )
    .await
    .unwrap();
    let worker = zone_server::workers::reminders::spawn(b.state.clone(), sweep);
    until_answered(&fleet, reminder).await;
    worker.abort();
    let answered = fleet.messages().await;
    fleet.cleanup().await;

    assert_eq!(
        refused,
        [(turn.attempts - 1, false)],
        "the refused turn gave its claim back without counting an attempt"
    );
    assert!(
        !told
            .iter()
            .any(|frame| frame["type"] == "denied" || frame["type"] == "error"),
        "a refused unattended turn tells whoever watches the chat nothing: {told:?}"
    );
    assert!(unanswered.is_empty(), "{unanswered:?}");
    assert!(
        reclaimed.is_none(),
        "a revoked runner claims no reminder turn"
    );
    assert_eq!(
        answered,
        [
            ("user".to_string(), PROMPT.to_string()),
            ("assistant".to_string(), ANSWER.to_string())
        ]
    );
}

#[tokio::test]
async fn an_auto_project_is_not_driven_by_a_cordoned_runner() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet.set_state(a.id, State::Cordoned).await;
    let (project, task) = auto_project(&fleet, fleet.workspace).await;

    Driver::of(&a.state).tick().await;
    let cordoned = (driven(&fleet, project).await, runs_of(&fleet, task).await);
    Driver::of(&b.state).tick().await;
    let runs = runs_of(&fleet, task).await;
    let claimed_on = match runs.as_slice() {
        [run] => Some(fleet.until_claimed(*run).await.1),
        _ => None,
    };
    let (_, drove_on) = project_claim(&fleet, project).await;
    fleet.cleanup().await;

    assert_eq!(
        cordoned,
        (false, Vec::new()),
        "a cordoned runner neither drives the project nor starts its task"
    );
    assert_eq!(runs.len(), 1, "the active runner drove the project");
    assert_eq!(drove_on, Some(b.id));
    assert_eq!(claimed_on, Some(b.id));
}

#[tokio::test]
async fn revoking_ends_an_auto_project_drive_and_its_fenced_writes() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    let (project, task) = auto_project(&fleet, fleet.workspace).await;
    let claimant = fleet::claimant(&a.state).await;
    let [claim] =
        auto_projects::claim_due(fleet.pool(), CLAIM_LEASE, PROJECTS_PER_TICK, &[], claimant)
            .await
            .unwrap()[..]
    else {
        panic!("the active runner claims the due project");
    };
    let claimed = project_claim(&fleet, project).await;
    let automation = auto_projects::automation(fleet.pool(), project)
        .await
        .unwrap()
        .expect("an automated project");
    let driver = Driver::of(&a.state);

    let (noticed, admission, write) = lease::holding(
        fleet.pool().clone(),
        claim,
        claimant,
        RENEWAL,
        |lease: lease::Lease| {
            let (fleet, driver, automation) = (&fleet, &driver, &automation);
            async move {
                fleet.set_state(a.id, State::Revoked).await;
                let noticed = tokio::time::timeout(SETTLES_WITHIN, async {
                    while !lease.lost() {
                        tokio::time::sleep(RENEWAL).await;
                    }
                })
                .await
                .is_ok();
                let drive = Drive {
                    state: &driver.state,
                    services: &driver.services,
                    config: &driver.config,
                    project: automation,
                    workspace_id: fleet.workspace,
                    actor: fleet.admin.id,
                    lease: &lease,
                };
                let admission = drive.admit(task, None).await;
                let write = auto_projects::set_stage(
                    fleet.pool(),
                    Some(claim),
                    task,
                    project,
                    Stage::Paused,
                    Some("the revoked drive's reason"),
                )
                .await;
                (noticed, admission, write)
            }
        },
    )
    .await;
    let ended = (
        project_claim(&fleet, project).await.0,
        runs_of(&fleet, task).await,
    );
    Driver::of(&b.state).tick().await;
    let reserved = runs_of(&fleet, task).await;
    sqlx::query(
        "UPDATE projects SET auto_claimed_at = auto_claimed_at - make_interval(secs => $2) \
         WHERE id = $1",
    )
    .bind(project)
    .bind(CLAIM_LEASE.as_secs_f64())
    .execute(fleet.pool())
    .await
    .expect("the revoked drive's lease lapses");
    Driver::of(&b.state).tick().await;
    let taken_over = runs_of(&fleet, task).await;
    let stage = auto_projects::task_stage(fleet.pool(), task)
        .await
        .unwrap()
        .map(|row| row.stage());
    fleet.cleanup().await;

    assert_eq!(claimed, (Some(claim.owner), Some(a.id)));
    assert!(
        noticed,
        "the revoked runner's drive is told it lost its claim"
    );
    assert_eq!(admission, Ok(Admission::Lost), "the drive admits no run");
    assert!(
        matches!(write, Err(WriteError::Lost)),
        "the drive's fenced write does not land: {write:?}"
    );
    assert_eq!(
        ended,
        (None, Vec::new()),
        "the revoke cleared the claim and nothing started"
    );
    assert!(
        reserved.is_empty(),
        "no runner drives the project while the revoked drive's lease runs"
    );
    assert_eq!(
        taken_over.len(),
        1,
        "an active runner drives it once that lease lapses"
    );
    assert_ne!(stage, Some(Stage::Paused));
}

#[tokio::test]
async fn a_chat_turn_is_refused_on_a_revoked_or_outdated_runner() {
    let fleet = Fleet::new(PREFIX, &[Spec::on(OUTDATED), Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    let (on_a, on_b) = (fleet.serve(&a.state).await, fleet.serve(&b.state).await);

    fleet.set_state(a.id, State::Revoked).await;
    let revoked = fleet.turn(&on_a, "Hello").await;
    let revoked_post = fleet.post_message(&a.state, "Hello").await;
    let served_while_revoked = fleet.turn(&on_b, "Hello").await;
    fleet.set_state(a.id, State::Active).await;
    fleet.set_minimum(Some(&current())).await;
    let outdated_turn = fleet.turn(&on_a, "Hello").await;
    let outdated_post = fleet.post_message(&a.state, "Hello").await;
    let served_while_outdated = fleet.turn(&on_b, "Hello").await;
    let asked = fleet.streamed().await;
    fleet.cleanup().await;

    let revoked_refusal = Refusal::Revoked;
    let outdated_refusal = Refusal::Outdated {
        version: outdated(),
        minimum: version(),
    };
    assert!(
        refused_with(&revoked, &revoked_refusal),
        "a revoked runner denies the turn with its code: {revoked:?}"
    );
    revoked_post.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        revoked_post.json_value(),
        json!({"error": revoked_refusal.message(), "code": RUNNER_REVOKED})
    );
    assert!(
        refused_with(&outdated_turn, &outdated_refusal),
        "an outdated runner denies the turn with its code: {outdated_turn:?}"
    );
    outdated_post.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        outdated_post.json_value(),
        json!({"error": outdated_refusal.message(), "code": RUNNER_OUTDATED})
    );
    successful(&served_while_revoked);
    successful(&served_while_outdated);
    assert_eq!(
        asked, 2,
        "only the turns the current runner served asked the model"
    );
}

#[tokio::test]
async fn a_cordoned_runner_still_serves_chat_turns() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let a = fleet.runner(0).clone();
    let on_a = fleet.serve(&a.state).await;
    fleet.set_state(a.id, State::Cordoned).await;

    let turn = fleet.turn(&on_a, "Hello").await;
    let posted = fleet.post_message(&a.state, "Noted").await;
    let unattended = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &unattended).await;
    let refused = fleet.claimed(unattended.id).await;
    fleet.cleanup().await;

    successful(&turn);
    posted.assert_status(StatusCode::CREATED);
    assert_eq!(
        refused,
        (None, None),
        "the cordoned runner still takes no unattended work"
    );
}

#[tokio::test]
async fn without_an_enterprise_license_no_fleet_rule_applies() {
    let fleet = Fleet::new(PREFIX, &[Spec::on(OUTDATED), Spec::labelled("gpu")]).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet.seed_state(a.id, State::Revoked).await;
    fleet.seed_minimum(&current()).await;
    fleet.seed_labels("gpu", "").await;
    let on_a = fleet.serve(&a.state).await;

    for edition in ["community", "team", "removed"] {
        match edition {
            "team" => fleet.license(&team()).await,
            "removed" => {
                fleet.license(&active()).await;
                fleet.remove_license().await;
            }
            _ => {}
        }
        fleet.heartbeat().await;

        let run = fleet.admit_in(fleet.workspace).await;
        fleet.execute(&a.state, &run).await;
        let ran = fleet.run(run.id).await;
        let (_, ran_on) = fleet.claimed(run.id).await;
        assert_eq!(ran.status, COMPLETED, "{edition}: {:?}", ran.error_message);
        assert_eq!(
            ran_on,
            Some(a.id),
            "{edition}: the runner is still recorded"
        );

        successful(&fleet.turn(&on_a, "Hello").await);
        fleet
            .post_message(&a.state, "Noted")
            .await
            .assert_status(StatusCode::CREATED);

        let reminder = owed_reminder(&fleet).await;
        let claimant = fleet::claimant(&a.state).await;
        let lease = zone_server::workers::reminders::Lease::default();
        let turn = reminders::claim_turn(
            fleet.pool(),
            Sweep::Workspace(fleet.workspace),
            lease.duration,
            claimant,
        )
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{edition}: the reminder turn is claimed"));
        zone_server::workers::reminders::run(&a.state, turn, lease, claimant).await;
        assert!(
            owed(&fleet, reminder).await.is_empty(),
            "{edition}: the reminder turn ran"
        );

        let (project, _) = auto_project(&fleet, fleet.workspace).await;
        let claims =
            auto_projects::claim_due(fleet.pool(), CLAIM_LEASE, PROJECTS_PER_TICK, &[], claimant)
                .await
                .unwrap();
        let claimed: Vec<_> = claims.iter().map(|claim| claim.project_id).collect();
        assert_eq!(claimed, [project], "{edition}: the auto project is claimed");
        for claim in claims {
            auto_projects::release(fleet.pool(), claim).await.unwrap();
        }
        sqlx::query("UPDATE projects SET auto = FALSE WHERE id = $1")
            .bind(project)
            .execute(fleet.pool())
            .await
            .unwrap();

        let waiting = fleet.admit_in(fleet.workspace).await;
        fleet.settle(waiting.id).await;
        let pickup = zone_server::workers::fleet::spawn(b.state.clone());
        tokio::time::sleep(IDLE_FOR).await;
        pickup.abort();
        assert_eq!(
            fleet.claimed(waiting.id).await,
            (None, None),
            "{edition}: pickup stays idle"
        );
        tasks::complete_task_run(fleet.pool(), waiting.id, FAILED, None, None)
            .await
            .unwrap();
    }
    fleet.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_keeps_enforcing() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT, Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let (a, b) = (fleet.runner(0).clone(), fleet.runner(1).clone());
    fleet.set_state(a.id, State::Revoked).await;
    fleet.license(&lapsed()).await;

    let run = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&a.state, &run).await;
    let left = fleet.claimed(run.id).await;
    let posted = fleet.post_message(&a.state, "Hello").await;
    fleet.settle(run.id).await;
    fleet.execute(&b.state, &run).await;
    let handed = fleet.run(run.id).await;
    let (_, handed_to) = fleet.claimed(run.id).await;
    fleet.cleanup().await;

    assert_eq!(
        left,
        (None, None),
        "a lapsed license still refuses a revoked runner"
    );
    posted.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(posted.json_value()["error"], Refusal::Revoked.message());
    assert_eq!(posted.json_value()["code"], RUNNER_REVOKED);
    assert_eq!(handed.status, COMPLETED, "{:?}", handed.error_message);
    assert_eq!(handed_to, Some(b.id));
}

#[tokio::test]
async fn a_licensed_server_without_a_registered_runner_registers_on_first_claim() {
    let fleet = Fleet::new(PREFIX, &[Spec::CURRENT]).await;
    fleet.license(&active()).await;
    let running = fleet.unregistered(Spec::labelled("gpu")).await;
    let chatting = fleet.unregistered(Spec::CURRENT).await;
    let unregistered = (running.runner(), chatting.runner());

    let run = fleet.admit_in(fleet.workspace).await;
    fleet.execute(&running, &run).await;
    let ran = fleet.run(run.id).await;
    let (_, ran_on) = fleet.claimed(run.id).await;
    let posted = fleet.post_message(&chatting, "Hello").await;
    let registered = (running.runner(), chatting.runner());
    let row = match registered.0 {
        Some(id) => runners::get(fleet.pool(), id).await.unwrap(),
        None => None,
    };
    let audits = fleet.audits(actions::RUNNER_REGISTERED).await;
    fleet.cleanup().await;

    assert_eq!(unregistered, (None, None));
    assert_eq!(ran.status, COMPLETED, "{:?}", ran.error_message);
    assert!(registered.0.is_some(), "the running server registered");
    assert_eq!(ran_on, registered.0, "it claimed the run as its new runner");
    let row = row.expect("the new runner's row");
    assert_eq!(row.name, running.identity().name);
    assert_eq!(row.labels, Labels::parse_list("gpu").unwrap());
    assert_eq!(row.state, State::Active);
    posted.assert_status(StatusCode::CREATED);
    assert!(registered.1.is_some(), "the chatting server registered");
    let audited: Vec<_> = audits
        .iter()
        .filter(|audit| audit.resource_id == registered.0 || audit.resource_id == registered.1)
        .map(|audit| (audit.organization_id, audit.actor_id))
        .collect();
    assert_eq!(
        audited,
        [(None, None), (None, None)],
        "each new runner name is audited at server scope"
    );
}
