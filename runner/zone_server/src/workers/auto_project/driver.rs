//! One pass over one project: settle what its runs left, advance every
//! change through the pipeline, start what can start, and say when it is done.

use std::sync::Arc;

use uuid::Uuid;
use zone_notify::{Fanout, Notification, Notifier};

use crate::config::AutoProjectConfig;
use crate::db::auto_projects::{self, Kind, ProjectAutomation, SettledRun, Stage};
use crate::db::chats::{self, ChatPurpose};
use crate::db::tasks::{self, RunMutation};
use crate::db::workspace_members;
use crate::state::AppState;
use crate::workers::notify::{self, ChatNotifier};

use super::{Services, notification, pipeline};

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
        match auto_projects::pause(self.state.db(), self.project.project_id, reason).await {
            Ok(true) => {
                tracing::warn!(project_id = %self.project.project_id, reason, "Auto project paused");
                self.notify(
                    "paused",
                    notification::paused(&self.project.name, reason, None),
                )
                .await;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(project_id = %self.project.project_id, %error, "Could not pause the project")
            }
        }
    }

    /// Start a run of a task, as the person automation runs as.
    pub async fn admit(&self, task_id: Uuid, reason: Option<&str>) -> Result<bool, String> {
        match tasks::create_unattended_task_run(self.state.db(), task_id, self.actor).await {
            Ok(RunMutation::Created(run)) => {
                auto_projects::record_admission(
                    self.state.db(),
                    task_id,
                    self.project.project_id,
                    run.id,
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
                    crate::workers::task::execute_task_run(&state, run.id, task_id).await;
                });
                tracing::info!(project_id = %self.project.project_id, %task_id, run_id = %run.id, "Auto project admitted a run");
                Ok(true)
            }
            Ok(RunMutation::Active(_)) => Ok(false),
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

    // 1. What earlier runs left behind, whether the driver started them or not.
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

    // 2. Every change on its way from a run to a merge.
    for task in auto_projects::pipeline(pool, project_id)
        .await
        .map_err(|error| error.to_string())?
    {
        if let Err(error) = pipeline::advance(&drive, &task).await {
            tracing::warn!(%project_id, task_id = %task.task_id, %error, "Pipeline step failed; retrying next tick");
        }
    }

    // 3. Start what can start.
    admit_runnable(&drive).await?;

    // 4. Done, stuck, or still going.
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
                .filter(|task| !task.is_agentic && task.status != "complete")
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
        if runnable.is_none() {
            if !paused.is_empty() {
                drive
                    .pause_project(&format!(
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
                    .filter(|task| task.status != "complete")
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
                    .pause_project(&format!(
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

/// Put a task whose run ended where the pipeline expects it.
async fn settle(drive: &Drive<'_>, run: &SettledRun) -> Result<(), String> {
    let pool = drive.state.db();
    let project_id = drive.project.project_id;
    let stage = auto_projects::task_stage(pool, run.task_id)
        .await
        .map_err(|error| error.to_string())?;
    let runs = stage.as_ref().map_or(0, |row| row.runs);
    match (run.status.as_str(), run.pr_url.as_deref()) {
        ("review", Some(_)) => {
            auto_projects::set_stage(pool, run.task_id, project_id, Stage::AwaitingChecks, None)
                .await
                .map_err(|error| error.to_string())?;
        }
        ("complete", _) => {
            auto_projects::set_stage(pool, run.task_id, project_id, Stage::Merged, None)
                .await
                .map_err(|error| error.to_string())?;
        }
        ("review", None) => {
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
        let title = tasks::get_task(pool, task_id)
            .await
            .ok()
            .flatten()
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
    if !drive.admit(task_id, Some(reason)).await? {
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
        if !drive.admit(task_id, None).await? {
            break;
        }
    }
    Ok(())
}
