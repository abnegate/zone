//! One pass over one project: settle what its runs left, advance every
//! change through the pipeline, start what can start, and say when it is done.

use std::collections::HashSet;
use std::sync::Arc;

use abnegate_notify::Fanout;
use abnegate_notify::Notification;
use abnegate_notify::Notifier;
use uuid::Uuid;

use crate::config::AutoProjectConfig;
use crate::db::DbResult;
use crate::db::auto_projects::{
    self, Kind, ProjectAutomation, ProjectTask, SettledRun, Stage, TaskAutomation,
};
use crate::db::chats::{self, ChatPurpose};
use crate::db::tasks::{self, Unattended};
use crate::db::workspace_members;
use crate::state::AppState;
use crate::workers::notify::{self, ChatNotifier};

use super::{Services, notification, pipeline};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Started,
    Active,
    Withheld,
}

/// What the driver has to hand a pipeline step.
pub struct Drive<'a> {
    pub state: &'a AppState,
    pub services: &'a Services,
    pub config: &'a AutoProjectConfig,
    pub project: &'a ProjectAutomation,
    pub workspace_id: Uuid,
    pub actor: Uuid,
}

impl Drive<'_> {
    /// Deliver a notice everywhere the operator configured, and into the
    /// project's updates chat whatever they configured.
    pub async fn notify(&self, kind: &'static str, notification: Notification) {
        let chat = match chats::ensure_project_chat(
            self.state.db(),
            self.workspace_id,
            self.project.project_id,
            &self.project.name,
            ChatPurpose::ProjectUpdates,
        )
        .await
        {
            Ok(chat) => Some(
                Arc::new(ChatNotifier::new(self.state.db().clone(), chat, kind))
                    as Arc<dyn Notifier>,
            ),
            Err(error) => {
                tracing::warn!(project_id = %self.project.project_id, %error, "Could not open the project's updates chat");
                None
            }
        };
        let fanout: Fanout = notify::fanout_with(
            self.services.channels.clone(),
            chat,
            self.services.notify_timeout,
        );
        let report = fanout.deliver(&notification).await;
        for failure in report.failures() {
            tracing::warn!(
                project_id = %self.project.project_id,
                channel = failure.name(),
                error = failure.error().map(ToString::to_string).unwrap_or_default(),
                "A project notice was not delivered"
            );
        }
    }

    /// Stop the whole project and say why, once.
    pub async fn pause_project(&self, reason: &str) {
        let paused = auto_projects::pause(self.state.db(), self.project.project_id, reason).await;
        self.announce_pause(paused, reason).await;
    }

    /// Stop a project nothing can start in. Nothing is said when something
    /// can start after all by the time the pause is written: the next pass
    /// starts it.
    async fn pause_stalled_project(&self, reason: &str) {
        let paused =
            auto_projects::pause_stalled(self.state.db(), self.project.project_id, reason).await;
        self.announce_pause(paused, reason).await;
    }

    async fn announce_pause(&self, paused: DbResult<bool>, reason: &str) {
        match paused {
            Ok(true) => {
                tracing::warn!(project_id = %self.project.project_id, reason, "Auto project paused");
                self.notify(
                    "paused",
                    notification::paused(&self.project.name, reason, None),
                )
                .await;
            }
            Ok(false) => {
                tracing::debug!(project_id = %self.project.project_id, reason, "Auto project pause was not applied");
            }
            Err(error) => {
                tracing::warn!(project_id = %self.project.project_id, %error, "Could not pause the project")
            }
        }
    }

    pub async fn park(&self) -> Result<(), String> {
        auto_projects::park(self.state.db(), self.project.project_id)
            .await
            .map_err(|error| error.to_string())
    }

    /// Start a run of a task, as the person automation runs as. A task a
    /// person paused or cancelled since the pass began is parked instead.
    pub async fn admit(&self, task_id: Uuid, reason: Option<&str>) -> Result<Admission, String> {
        match tasks::create_unattended_task_run(self.state.db(), task_id, self.actor).await {
            Ok(Unattended::Created(run_id)) => {
                auto_projects::record_admission(
                    self.state.db(),
                    task_id,
                    self.project.project_id,
                    run_id,
                )
                .await
                .map_err(|error| error.to_string())?;
                if let Some(reason) = reason {
                    auto_projects::set_stage(
                        self.state.db(),
                        task_id,
                        self.project.project_id,
                        Stage::Running,
                        Some(reason),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                }
                let state = self.state.clone();
                tokio::spawn(async move {
                    crate::workers::task::execute_task_run(&state, run_id, task_id).await;
                });
                tracing::info!(project_id = %self.project.project_id, %task_id, %run_id, "Auto project admitted a run");
                Ok(Admission::Started)
            }
            Ok(Unattended::Active) => Ok(Admission::Active),
            Ok(Unattended::Withheld) => {
                self.park().await?;
                tracing::info!(project_id = %self.project.project_id, %task_id, "Auto project left a paused or cancelled task alone");
                Ok(Admission::Withheld)
            }
            Err(error) => Err(format!("could not admit a run for task {task_id}: {error}")),
        }
    }

    /// Add a task the driver decided the project needs.
    pub async fn add_task(
        &self,
        kind: Kind,
        title: &str,
        description: &str,
        criteria: Option<&str>,
        why: &str,
    ) -> Result<Uuid, String> {
        let id = auto_projects::insert_auto_task(
            self.state.db(),
            self.project.project_id,
            self.workspace_id,
            self.actor,
            kind,
            title,
            description,
            criteria,
        )
        .await
        .map_err(|error| error.to_string())?;
        self.notify(
            "task_added",
            notification::task_added(&self.project.name, kind.as_str(), title, why),
        )
        .await;
        Ok(id)
    }
}

/// One pass over a claimed project: settle finished runs, advance every task's pipeline, admit what can run, and notice completion.
pub async fn drive_project(
    state: &AppState,
    services: &Services,
    config: &AutoProjectConfig,
    project_id: Uuid,
) -> Result<(), String> {
    let pool = state.db();
    let Some(project) = auto_projects::automation(pool, project_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if !project.auto || project.paused_reason.is_some() || project.completed_at.is_some() {
        return Ok(());
    }
    let Some(workspace_id) = project.workspace_id else {
        return Ok(());
    };
    let drive = Drive {
        state,
        services,
        config,
        project: &project,
        workspace_id,
        actor: project.actor_id.unwrap_or(Uuid::nil()),
    };
    let Some(actor) = project.actor_id else {
        drive
            .pause_project("nobody is recorded as having turned automation on")
            .await;
        return Ok(());
    };
    if !workspace_members::can_write(pool, workspace_id, actor)
        .await
        .map_err(|error| error.to_string())?
    {
        drive
            .pause_project(
                "the person who turned automation on no longer has write access to the workspace",
            )
            .await;
        return Ok(());
    }

    drive.park().await?;
    auto_projects::unpark(pool, project_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut settled = auto_projects::enrollable(pool, project_id)
        .await
        .map_err(|error| error.to_string())?;
    settled.extend(
        auto_projects::settled_runs(pool, project_id)
            .await
            .map_err(|error| error.to_string())?,
    );
    for run in settled {
        settle(&drive, &run).await?;
    }

    let changes = auto_projects::pipeline(pool, project_id)
        .await
        .map_err(|error| error.to_string())?;
    services.outages.retain(project_id, &reviewing(&changes));
    for task in changes {
        if let Err(error) = pipeline::advance(&drive, &task).await {
            tracing::warn!(%project_id, task_id = %task.task_id, %error, "Pipeline step failed; retrying next tick");
        }
    }

    admit_runnable(&drive).await?;

    let remaining = auto_projects::remaining(pool, project_id)
        .await
        .map_err(|error| error.to_string())?;
    if remaining == 0 {
        if auto_projects::complete(pool, project_id)
            .await
            .map_err(|error| error.to_string())?
        {
            let tasks = auto_projects::project_tasks(pool, project_id)
                .await
                .unwrap_or_default();
            let merged = tasks
                .iter()
                .filter(|task| task.stage.as_deref() == Some(Stage::Merged.as_str()))
                .count();
            let manual = tasks
                .iter()
                .filter(|task| !task.is_agentic && open(&task.status))
                .count();
            let post_merge = tasks
                .iter()
                .filter(|task| task.stage.as_deref() == Some(Stage::Merged.as_str()))
                .filter_map(|task| task.reason.clone())
                .next_back();
            drive
                .notify(
                    "completed",
                    notification::completed(&project.name, merged, manual, post_merge.as_deref()),
                )
                .await;
            tracing::info!(%project_id, merged, "Auto project complete");
        } else {
            tracing::debug!(%project_id, "Auto project completion was not applied");
        }
        return Ok(());
    }
    let in_flight = auto_projects::in_flight(pool, project_id)
        .await
        .map_err(|error| error.to_string())?;
    if in_flight == 0 {
        let stuck = auto_projects::project_tasks(pool, project_id)
            .await
            .map_err(|error| error.to_string())?;
        let paused: Vec<String> = stuck
            .iter()
            .filter(|task| task.stage.as_deref() == Some(Stage::Paused.as_str()))
            .filter(|task| task.status != tasks::CANCELLED)
            .map(|task| {
                format!(
                    "{}: {}",
                    task.title,
                    task.reason.as_deref().unwrap_or("needs a person")
                )
            })
            .collect();
        let runnable = auto_projects::next_runnable(pool, project_id)
            .await
            .map_err(|error| error.to_string())?;
        if runnable.is_none() && !held_by_a_person(&stuck) {
            if !paused.is_empty() {
                drive
                    .pause_stalled_project(&format!(
                        "nothing can start until a person looks at: {}",
                        paused.join("; ")
                    ))
                    .await;
            } else {
                // Nothing paused, nothing running, nothing admissible, work
                // left: the remaining tasks wait on something automation
                // cannot supply -- a task nobody made agentic, a dependency
                // that never finished. Spinning here would look like progress.
                let waiting: Vec<String> = stuck
                    .iter()
                    .filter(|task| open(&task.status))
                    .map(|task| {
                        format!(
                            "{} ({}{})",
                            task.title,
                            task.status,
                            if task.is_agentic { "" } else { ", not agentic" }
                        )
                    })
                    .collect();
                drive
                    .pause_stalled_project(&format!(
                        "nothing can start: {} task(s) remain but none is runnable; they wait on \
                         tasks that are not agentic or did not finish: {}",
                        remaining,
                        waiting.join("; ")
                    ))
                    .await;
            }
        }
    }
    Ok(())
}

fn open(status: &str) -> bool {
    !tasks::CLOSED.contains(&status)
}

/// Whether every agentic task left to do is one a person paused. The project
/// is then waiting on that person, not stuck: pausing it would tell them what
/// they did, and leave their un-pausing with nothing to restart.
fn held_by_a_person(listed: &[ProjectTask]) -> bool {
    listed
        .iter()
        .filter(|task| task.is_agentic && open(&task.status))
        .all(|task| task.status == tasks::PAUSED)
}

/// The tasks among `changes` that are waiting on a review.
fn reviewing(changes: &[TaskAutomation]) -> HashSet<Uuid> {
    changes
        .iter()
        .filter(|task| task.stage() == Stage::AwaitingReviews)
        .map(|task| task.task_id)
        .collect()
}

/// Put a task whose run ended where the pipeline expects it.
async fn settle(drive: &Drive<'_>, run: &SettledRun) -> Result<(), String> {
    let pool = drive.state.db();
    let project_id = drive.project.project_id;
    let stage = auto_projects::task_stage(pool, run.task_id)
        .await
        .map_err(|error| error.to_string())?;
    let runs = stage.as_ref().map_or(0, |row| row.runs);
    match (run.status.as_str(), run.pr_url.as_deref()) {
        (tasks::REVIEW, Some(_)) => {
            auto_projects::set_stage(pool, run.task_id, project_id, Stage::AwaitingChecks, None)
                .await
                .map_err(|error| error.to_string())?;
        }
        (tasks::COMPLETE, _) => {
            auto_projects::set_stage(pool, run.task_id, project_id, Stage::Merged, None)
                .await
                .map_err(|error| error.to_string())?;
        }
        (tasks::REVIEW, None) => {
            retry(
                drive,
                run.task_id,
                runs,
                "the last run finished without changing anything; do the task's work and leave the \
                 change in the checkout so it can be published",
            )
            .await?;
        }
        _ => {
            let error = run
                .error
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or("the run failed without a recorded reason");
            retry(
                drive,
                run.task_id,
                runs,
                &format!("the last run failed: {error}"),
            )
            .await?;
        }
    }
    Ok(())
}

/// Run a task again with the reason in its prompt, or give up on it.
async fn retry(drive: &Drive<'_>, task_id: Uuid, runs: i32, reason: &str) -> Result<(), String> {
    let pool = drive.state.db();
    let project_id = drive.project.project_id;
    if runs >= i32::try_from(drive.config.max_runs_per_task).unwrap_or(i32::MAX) {
        let task = tasks::get_task(pool, task_id)
            .await
            .map_err(|error| error.to_string())?;
        if task
            .as_ref()
            .is_some_and(|task| tasks::WITHHELD.contains(&task.status.as_str()))
        {
            return drive.park().await;
        }
        let title = task
            .map(|task| task.title)
            .unwrap_or_else(|| task_id.to_string());
        let why = format!("task \"{title}\" used its {runs} runs; {reason}");
        auto_projects::set_stage(pool, task_id, project_id, Stage::Paused, Some(&why))
            .await
            .map_err(|error| error.to_string())?;
        drive
            .notify(
                "paused",
                notification::paused(&drive.project.name, &why, None),
            )
            .await;
        return Ok(());
    }
    // The reason rides on the row so the run's guidance can quote it.
    auto_projects::set_stage(pool, task_id, project_id, Stage::Idle, Some(reason))
        .await
        .map_err(|error| error.to_string())?;
    if drive.admit(task_id, Some(reason)).await? == Admission::Active {
        auto_projects::set_stage(pool, task_id, project_id, Stage::Running, Some(reason))
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Start tasks while the project and the server have room for them.
async fn admit_runnable(drive: &Drive<'_>) -> Result<(), String> {
    let pool = drive.state.db();
    let project_id = drive.project.project_id;
    let mut admitted: Vec<Uuid> = Vec::new();
    loop {
        let in_flight = auto_projects::in_flight(pool, project_id)
            .await
            .map_err(|error| error.to_string())?;
        if in_flight >= i64::try_from(drive.config.parallel_tasks).unwrap_or(i64::MAX) {
            break;
        }
        let active = auto_projects::active_unattended_runs(pool)
            .await
            .map_err(|error| error.to_string())?;
        if active >= i64::try_from(drive.config.max_active_runs).unwrap_or(i64::MAX) {
            break;
        }
        let Some(task_id) = auto_projects::next_runnable(pool, project_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            break;
        };
        if admitted.contains(&task_id) {
            break;
        }
        admitted.push(task_id);
        // The row is made before the run so the task counts as in flight at
        // once; `next_runnable` reads `created`, which admission moves past.
        if drive.admit(task_id, None).await? != Admission::Started {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::PullRequestService;
    use chrono::Utc;

    use super::*;
    use crate::db::auto_projects::testing::Project;

    struct Driven {
        state: AppState,
        services: Services,
        config: AutoProjectConfig,
    }

    impl Driven {
        fn new(project: &Project) -> Self {
            Self {
                state: AppState::new(crate::state::test_config(), project.pool.clone(), None),
                services: Services::standing_in_for(
                    PullRequestService::new().expect("GitHub's own origin"),
                ),
                config: AutoProjectConfig::default(),
            }
        }

        /// A driver with no room for a run, so a pass shows what it would
        /// start without starting it.
        fn without_room(project: &Project) -> Self {
            Self {
                config: AutoProjectConfig {
                    max_active_runs: 0,
                    ..AutoProjectConfig::default()
                },
                ..Self::new(project)
            }
        }

        async fn drive(&self, project: &Project) -> Result<(), String> {
            drive_project(&self.state, &self.services, &self.config, project.id).await
        }
    }

    async fn automation(project: &Project) -> ProjectAutomation {
        auto_projects::automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project")
    }

    async fn runnable(project: &Project) -> Option<Uuid> {
        auto_projects::next_runnable(&project.pool, project.id)
            .await
            .expect("the next runnable task")
    }

    #[tokio::test]
    async fn a_project_waiting_only_on_a_task_a_person_paused_is_neither_paused_nor_announced() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        let driven = Driven::without_room(&project);

        project.move_to(task, tasks::PAUSED).await;
        let first = driven.drive(&project).await;
        let waiting = automation(&project).await;
        let parked = project.automation(task).await;
        let announced = project.notices().await;
        project.move_to(task, tasks::CREATED).await;
        let second = driven.drive(&project).await;
        let after = automation(&project).await;
        let unparked = project.automation(task).await;
        let next = runnable(&project).await;
        let admitted = tasks::create_unattended_task_run(&project.pool, task, project.actor)
            .await
            .expect("the admission");
        project.remove().await;

        first.expect("the drive while the task is paused");
        assert_eq!(
            waiting.paused_reason, None,
            "the project waits on the person who paused its task"
        );
        assert_eq!(waiting.completed_at, None);
        assert_eq!(parked.stage(), Stage::Paused);
        assert_eq!(announced, Vec::<String>::new());
        second.expect("the drive after the task is un-paused");
        assert_eq!(after.paused_reason, None);
        assert_eq!(unparked.stage(), Stage::Idle);
        assert_eq!(next, Some(task), "the next pass starts it");
        assert!(matches!(admitted, Unattended::Created(_)), "{admitted:?}");
    }

    #[tokio::test]
    async fn un_cancelling_the_last_open_task_opens_a_finished_project_again() {
        let project = Project::new(2).await;
        let [merged, open] = project.tasks[..] else {
            panic!("two tasks");
        };
        project
            .place(merged, tasks::COMPLETE, None, Stage::Merged)
            .await;
        let driven = Driven::without_room(&project);

        project.move_to(open, tasks::CANCELLED).await;
        let first = driven.drive(&project).await;
        let finished = automation(&project).await;
        project.move_to(open, tasks::CREATED).await;
        let reopened = automation(&project).await;
        let second = driven.drive(&project).await;
        let after = automation(&project).await;
        let unparked = project.automation(open).await;
        let next = runnable(&project).await;
        project.remove().await;

        first.expect("the drive that finishes the project");
        assert!(
            finished.completed_at.is_some(),
            "nothing is left once its last open task is cancelled"
        );
        assert_eq!(
            reopened.completed_at, None,
            "the task a person gave back is work the project has again"
        );
        second.expect("the drive after the task is un-cancelled");
        assert_eq!(after.completed_at, None);
        assert_eq!(after.paused_reason, None);
        assert_eq!(unparked.stage(), Stage::Idle);
        assert_eq!(next, Some(open), "the next pass starts it");
    }

    #[tokio::test]
    async fn queueing_or_running_a_cancelled_task_opens_its_finished_project_too() {
        let mut reopened = Vec::new();
        for queue in [true, false] {
            let project = Project::new(2).await;
            let [merged, open] = project.tasks[..] else {
                panic!("two tasks");
            };
            project
                .place(merged, tasks::COMPLETE, None, Stage::Merged)
                .await;
            project.move_to(open, tasks::CANCELLED).await;
            let driven = Driven::without_room(&project);
            let drove = driven.drive(&project).await;
            let finished = automation(&project).await.completed_at.is_some();

            let started = if queue {
                tasks::queue_task_authorized(&project.pool, project.actor, open)
                    .await
                    .map(|queued| matches!(queued, tasks::Mutation::Applied(_)))
                    .map_err(|error| error.to_string())
            } else {
                tasks::create_task_run_authorized(&project.pool, project.actor, open)
                    .await
                    .map(|admitted| {
                        matches!(
                            admitted,
                            tasks::Mutation::Applied(tasks::RunMutation::Created(_))
                        )
                    })
                    .map_err(|error| error.to_string())
            };
            let after = automation(&project).await;
            project.remove().await;

            drove.expect("the drive that finishes the project");
            reopened.push((finished, started, after.completed_at.is_none()));
        }

        assert_eq!(
            reopened,
            vec![(true, Ok(true), true), (true, Ok(true), true)],
            "queued, then run: each takes the task back, so the project has work again"
        );
    }

    #[tokio::test]
    async fn a_project_stopped_behind_a_paused_task_names_it_and_starts_again_once_it_is_un_paused()
    {
        let project = Project::new(2).await;
        let [blocker, dependent] = project.tasks[..] else {
            panic!("two tasks");
        };
        tasks::set_dependencies(&project.pool, dependent, &[blocker])
            .await
            .expect("the dependency");
        let driven = Driven::without_room(&project);

        project.move_to(blocker, tasks::PAUSED).await;
        let first = driven.drive(&project).await;
        let stopped = automation(&project).await;
        project.move_to(blocker, tasks::CREATED).await;
        let released = automation(&project).await;
        let second = driven.drive(&project).await;
        let after = automation(&project).await;
        let next = runnable(&project).await;
        project.remove().await;

        first.expect("the drive while the dependency is paused");
        assert!(
            stopped
                .paused_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("Task 0")),
            "the reason names the paused task the other waits on: {:?}",
            stopped.paused_reason
        );
        assert_eq!(
            released.paused_reason, None,
            "un-pausing the task is all it takes to start the project again"
        );
        second.expect("the drive after the dependency is un-paused");
        assert_eq!(after.paused_reason, None);
        assert_eq!(next, Some(blocker));
    }

    /// Drive the project while its row is held, and have a person's move of
    /// `task` to `status` commit once the pass has read the project and
    /// queued to stop it: the move lands between the pass's read and its write.
    async fn drive_as_a_task_is_moved(
        driven: &Driven,
        project: &Project,
        task: Uuid,
        status: &str,
    ) -> Result<(), String> {
        let hold = project.hold().await;
        let (drove, ()) = tokio::join!(driven.drive(project), async move {
            project.queued_behind(&hold).await;
            project.set_status(task, status).await;
            hold.release().await;
        });
        drove
    }

    #[tokio::test]
    async fn a_task_un_cancelled_as_the_pass_finishes_the_project_keeps_it_open() {
        let project = Project::new(2).await;
        let [merged, open] = project.tasks[..] else {
            panic!("two tasks");
        };
        project
            .place(merged, tasks::COMPLETE, None, Stage::Merged)
            .await;
        project.move_to(open, tasks::CANCELLED).await;
        let driven = Driven::without_room(&project);

        let drove = drive_as_a_task_is_moved(&driven, &project, open, tasks::CREATED).await;
        let after = automation(&project).await;
        let announced = project.notices().await;
        let second = driven.drive(&project).await;
        let next = runnable(&project).await;
        project.remove().await;

        drove.expect("the drive the move landed in");
        assert_eq!(
            after.completed_at, None,
            "the project is not finished over a task that has work left"
        );
        assert_eq!(after.paused_reason, None);
        assert_eq!(announced, Vec::<String>::new(), "nothing is announced");
        second.expect("the next drive");
        assert_eq!(next, Some(open), "the next pass starts it");
    }

    #[tokio::test]
    async fn a_task_un_paused_as_the_pass_stops_the_project_keeps_it_going() {
        let project = Project::new(2).await;
        let [blocker, dependent] = project.tasks[..] else {
            panic!("two tasks");
        };
        tasks::set_dependencies(&project.pool, dependent, &[blocker])
            .await
            .expect("the dependency");
        project.move_to(blocker, tasks::PAUSED).await;
        let driven = Driven::without_room(&project);

        let drove = drive_as_a_task_is_moved(&driven, &project, blocker, tasks::CREATED).await;
        let after = automation(&project).await;
        let announced = project.notices().await;
        let second = driven.drive(&project).await;
        let next = runnable(&project).await;
        project.remove().await;

        drove.expect("the drive the move landed in");
        assert_eq!(
            after.paused_reason, None,
            "the project is not paused over a task that can start"
        );
        assert_eq!(after.completed_at, None);
        assert_eq!(announced, Vec::<String>::new(), "nothing is announced");
        second.expect("the next drive");
        assert_eq!(next, Some(blocker), "the next pass starts it");
    }

    #[tokio::test]
    async fn a_task_a_person_pauses_as_the_pass_stops_the_project_leaves_it_waiting_on_them() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::BLOCKED, None, Stage::Paused)
            .await;
        let driven = Driven::without_room(&project);

        let drove = drive_as_a_task_is_moved(&driven, &project, task, tasks::PAUSED).await;
        let after = automation(&project).await;
        let announced = project.notices().await;
        let second = driven.drive(&project).await;
        let waiting = automation(&project).await;
        project.remove().await;

        drove.expect("the drive the move landed in");
        assert_eq!(
            after.paused_reason, None,
            "the project waits on the person who paused its last task"
        );
        assert_eq!(announced, Vec::<String>::new(), "nothing is announced");
        second.expect("the next drive");
        assert_eq!(waiting.paused_reason, None);
        assert_eq!(waiting.completed_at, None);
    }

    #[tokio::test]
    async fn a_task_a_person_cancels_as_the_pass_stops_the_project_finishes_it_instead() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::BLOCKED, None, Stage::Paused)
            .await;
        let driven = Driven::without_room(&project);

        let drove = drive_as_a_task_is_moved(&driven, &project, task, tasks::CANCELLED).await;
        let after = automation(&project).await;
        let announced = project.notices().await;
        let second = driven.drive(&project).await;
        let finished = automation(&project).await;
        project.remove().await;

        drove.expect("the drive the move landed in");
        assert_eq!(
            after.paused_reason, None,
            "the project is not paused over a task that is no longer left to do"
        );
        assert_eq!(announced, Vec::<String>::new(), "nothing is announced");
        second.expect("the next drive");
        assert!(
            finished.completed_at.is_some(),
            "the next pass finds nothing left and finishes the project"
        );
    }

    /// A pool whose connections find a `tasks` table no task can be read
    /// from ahead of the real one, and every other table where it is.
    async fn pool_that_cannot_read_tasks(project: &Project, schema: &str) -> sqlx::PgPool {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE SCHEMA {schema} CREATE TABLE tasks (id uuid)"
        )))
        .execute(&project.pool)
        .await
        .expect("a schema that shadows the tasks table");
        let search_path = format!("SET search_path TO {schema}, public");
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |connection, _| {
                let search_path = search_path.clone();
                Box::pin(async move {
                    sqlx::query(sqlx::AssertSqlSafe(search_path))
                        .execute(connection)
                        .await
                        .map(drop)
                })
            })
            .connect(&std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"))
            .await
            .expect("the test database")
    }

    #[tokio::test]
    async fn a_task_that_cannot_be_read_is_not_given_up_on() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::CANCELLED, None, Stage::Running)
            .await;
        let schema = format!("unreadable_{}", Uuid::new_v4().simple());
        let driven = Driven {
            state: AppState::new(
                crate::state::test_config(),
                pool_that_cannot_read_tasks(&project, &schema).await,
                None,
            ),
            ..Driven::new(&project)
        };
        let driven_project = automation(&project).await;
        let drive = Drive {
            state: &driven.state,
            services: &driven.services,
            config: &driven.config,
            project: &driven_project,
            workspace_id: project.workspace,
            actor: project.actor,
        };
        let spent = i32::try_from(driven.config.max_runs_per_task).expect("a small cap");

        let retried = retry(&drive, task, spent, "the last run failed: boom").await;
        let after = project.automation(task).await;
        let announced = project.notices().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&project.pool)
            .await
            .expect("the shadowing schema is removed");
        project.remove().await;

        assert!(
            retried.is_err(),
            "a read that failed says nothing about the task: {retried:?}"
        );
        assert_eq!(
            after.stage(),
            Stage::Running,
            "the next tick settles the run"
        );
        assert_eq!(
            announced,
            Vec::<String>::new(),
            "a cancelled task is not reported as out of runs"
        );
    }

    #[tokio::test]
    async fn a_settled_run_on_a_paused_task_is_not_retried() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::PAUSED, None, Stage::Running)
            .await;

        let driven = Driven::new(&project);
        let drove = driven.drive(&project).await;
        let runs = project.runs(task).await;
        let after = project.automation(task).await;
        let status = project.status(task).await;
        project.remove().await;

        drove.expect("the drive");
        assert_eq!(runs, 0, "no run is started for a paused task");
        assert_eq!(after.stage(), Stage::Paused);
        assert_eq!(
            after.reason.as_deref(),
            Some(auto_projects::PAUSED_BY_PERSON)
        );
        assert_eq!(status, tasks::PAUSED);
    }

    #[tokio::test]
    async fn a_task_cancelled_after_the_park_gets_no_run_from_a_retry() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::CANCELLED, None, Stage::Running)
            .await;
        let driven = Driven::new(&project);
        let automation = auto_projects::automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        let drive = Drive {
            state: &driven.state,
            services: &driven.services,
            config: &driven.config,
            project: &automation,
            workspace_id: project.workspace,
            actor: project.actor,
        };

        let retried = retry(&drive, task, 1, "the last run failed: boom").await;
        let runs = project.runs(task).await;
        let after = project.automation(task).await;
        let status = project.status(task).await;
        project.remove().await;

        retried.expect("the retry");
        assert_eq!(runs, 0, "the admission is refused under the task's lock");
        assert_eq!(after.stage(), Stage::Paused);
        assert_eq!(
            after.reason.as_deref(),
            Some(auto_projects::CANCELLED_BY_PERSON)
        );
        assert_eq!(status, tasks::CANCELLED);
    }

    #[tokio::test]
    async fn a_task_cancelled_after_its_last_run_is_parked_not_given_up_on() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::CANCELLED, None, Stage::Running)
            .await;
        let driven = Driven::new(&project);
        let automation = auto_projects::automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        let drive = Drive {
            state: &driven.state,
            services: &driven.services,
            config: &driven.config,
            project: &automation,
            workspace_id: project.workspace,
            actor: project.actor,
        };
        let spent = i32::try_from(driven.config.max_runs_per_task).expect("a small cap");

        let retried = retry(&drive, task, spent, "the last run failed: boom").await;
        let after = project.automation(task).await;
        project.remove().await;

        retried.expect("the retry");
        assert_eq!(after.stage(), Stage::Paused);
        assert_eq!(
            after.reason.as_deref(),
            Some(auto_projects::CANCELLED_BY_PERSON),
            "a cancelled task is parked quietly, not reported as out of runs"
        );
    }

    #[tokio::test]
    async fn a_project_whose_only_open_agentic_task_is_cancelled_completes() {
        let project = Project::new(2).await;
        let [merged, cancelled] = project.tasks[..] else {
            panic!("two tasks");
        };
        project
            .place(merged, tasks::COMPLETE, None, Stage::Merged)
            .await;
        project
            .place(cancelled, tasks::CANCELLED, None, Stage::Idle)
            .await;

        let driven = Driven::new(&project);
        let drove = driven.drive(&project).await;
        let automation = auto_projects::automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        let runs = project.runs(cancelled).await;
        project.remove().await;

        drove.expect("the drive");
        assert!(
            automation.completed_at.is_some(),
            "the project is complete, not paused: {:?}",
            automation.paused_reason
        );
        assert_eq!(automation.paused_reason, None);
        assert_eq!(runs, 0);
    }

    #[test]
    fn a_cancelled_task_has_no_work_left_and_a_paused_one_does() {
        assert!(!open(tasks::COMPLETE));
        assert!(!open(tasks::CANCELLED));
        assert!(open(tasks::PAUSED));
        assert!(open(tasks::BLOCKED));
    }

    fn change(stage: Stage) -> TaskAutomation {
        TaskAutomation {
            task_id: Uuid::new_v4(),
            project_id: Uuid::nil(),
            kind: None,
            stage: stage.as_str().to_string(),
            reason: None,
            runs: 1,
            review_rounds: 0,
            head: None,
            checks: None,
            checks_since: None,
            bot_trigger_head: None,
            merge_sha: None,
            auto_created: false,
            last_run_id: None,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn only_changes_awaiting_review_keep_their_reviewer_outages() {
        let changes: Vec<TaskAutomation> = [
            Stage::AwaitingChecks,
            Stage::AwaitingReviews,
            Stage::Fixing,
            Stage::Merging,
            Stage::PostMerge,
        ]
        .into_iter()
        .map(change)
        .collect();

        assert_eq!(reviewing(&changes), HashSet::from([changes[1].task_id]));
    }
}
