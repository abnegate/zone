//! What an auto project records about itself.
//!
//! Three things live here and nowhere else: the project-level switch, the
//! actor it runs as and the driver's lease; each task's place in the
//! review-and-merge pipeline; and the reviews a task's pull request received,
//! from Zone's own reviewer model and from the bots installed on the
//! repository. Every query is written out rather than checked at compile time,
//! so the offline query cache the build reads from does not have to move and
//! the row structs the rest of the server shares stay exactly as they are.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use std::collections::HashSet;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

use super::DbResult;
use super::actions::invalid;
use super::tasks::{self, Create, MutationError, Origin};

/// The project-level switch and everything the driver needs to run it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectAutomation {
    pub project_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub name: String,
    pub auto: bool,
    pub actor_id: Option<Uuid>,
    pub brief: Option<Value>,
    pub paused_reason: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub repository_url: Option<String>,
}

const PROJECT_COLUMNS: &str = "id AS project_id, workspace_id, name, auto, auto_actor_id AS actor_id, brief, \
     auto_paused_reason AS paused_reason, auto_claimed_at AS claimed_at, \
     auto_completed_at AS completed_at, github_repo_url AS repository_url";

/// What automation knows about one project, if the project exists.
pub async fn automation(pool: &PgPool, project_id: Uuid) -> DbResult<Option<ProjectAutomation>> {
    sqlx::query_as::<_, ProjectAutomation>(sqlx::AssertSqlSafe(format!(
        "SELECT {PROJECT_COLUMNS} FROM projects WHERE id = $1"
    )))
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

/// The automation state of every project in a workspace, for the list.
pub async fn automation_for_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<Vec<ProjectAutomation>> {
    sqlx::query_as::<_, ProjectAutomation>(sqlx::AssertSqlSafe(format!(
        "SELECT {PROJECT_COLUMNS} FROM projects WHERE workspace_id = $1 ORDER BY created_at DESC"
    )))
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

/// Turn a project's automation on or off.
///
/// Enabling records who did it -- every run the driver starts is authorized as
/// that person -- and clears a pause and a completion, so a project that was
/// paused or finished can be picked up again. Disabling leaves the actor in
/// place: a run already going keeps its authority, and nothing new is admitted.
pub async fn set_auto(
    pool: &PgPool,
    project_id: Uuid,
    auto: bool,
    actor: Uuid,
) -> DbResult<Option<ProjectAutomation>> {
    sqlx::query_as::<_, ProjectAutomation>(sqlx::AssertSqlSafe(format!(
        "UPDATE projects SET auto = $2, \
           auto_actor_id = CASE WHEN $2 THEN $3 ELSE auto_actor_id END, \
           auto_paused_reason = CASE WHEN $2 THEN NULL ELSE auto_paused_reason END, \
           auto_stalled = auto_stalled AND NOT $2, \
           auto_completed_at = CASE WHEN $2 THEN NULL ELSE auto_completed_at END, \
           updated_at = NOW() \
         WHERE id = $1 RETURNING {PROJECT_COLUMNS}"
    )))
    .bind(project_id)
    .bind(auto)
    .bind(actor)
    .fetch_optional(pool)
    .await
}

/// Store the interview's decisions on the project.
pub async fn set_brief(
    connection: &mut PgConnection,
    project_id: Uuid,
    brief: &Value,
) -> DbResult<()> {
    sqlx::query("UPDATE projects SET brief = $2, updated_at = NOW() WHERE id = $1")
        .bind(project_id)
        .bind(brief)
        .execute(connection)
        .await?;
    Ok(())
}

/// Claim the projects due for a drive, across every server instance.
///
/// `FOR UPDATE SKIP LOCKED` keeps two instances from taking the same project in
/// the same instant; the lease keeps an instance that died mid-drive from
/// holding it for ever. A claim past its lease is offered again.
///
/// The projects longest undriven go first, and [`release`] stamps each pass,
/// so every open project gets its turn however many of them a pass has
/// nothing to do for.
pub async fn claim_due(pool: &PgPool, lease: Duration, limit: i64) -> DbResult<Vec<Uuid>> {
    let seconds = lease.as_secs_f64();
    let mut transaction = pool.begin().await?;
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE auto AND status = 'active' \
           AND auto_paused_reason IS NULL AND auto_completed_at IS NULL \
           AND (auto_claimed_at IS NULL OR auto_claimed_at < NOW() - make_interval(secs => $1)) \
         ORDER BY auto_driven_at NULLS FIRST, id LIMIT $2 FOR UPDATE SKIP LOCKED",
    )
    .bind(seconds)
    .bind(limit)
    .fetch_all(&mut *transaction)
    .await?;
    if !due.is_empty() {
        sqlx::query("UPDATE projects SET auto_claimed_at = NOW() WHERE id = ANY($1)")
            .bind(&due)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(due)
}

/// Give up the driver's lease on a project once its pass is over, and send
/// the project to the back of the order [`claim_due`] takes projects in.
pub async fn release(pool: &PgPool, project_id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE projects SET auto_claimed_at = NULL, auto_driven_at = NOW() WHERE id = $1")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Stop driving a project and say why. Nothing already running is touched.
pub async fn pause(pool: &PgPool, project_id: Uuid, reason: &str) -> DbResult<bool> {
    Ok(sqlx::query(
        "UPDATE projects SET auto_paused_reason = $2, auto_stalled = FALSE, \
           auto_claimed_at = NULL, updated_at = NOW() \
         WHERE id = $1 AND auto AND auto_paused_reason IS NULL",
    )
    .bind(project_id)
    .bind(reason)
    .execute(pool)
    .await?
    .rows_affected()
        == 1)
}

/// Hold a project's row until the transaction ends. Read what the write
/// depends on in a later statement, never this one, or it predates the lock.
async fn hold(connection: &mut PgConnection, project_id: Uuid) -> DbResult<()> {
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR NO KEY UPDATE")
        .bind(project_id)
        .execute(connection)
        .await?;
    Ok(())
}

/// Pause a project nothing can start in, unless the driver would no longer
/// stop it: false when a task became runnable or went in flight after the
/// driver looked, or when every task left to do is by now one a person paused
/// or cancelled. The next pass then decides afresh.
pub async fn pause_stalled(pool: &PgPool, project_id: Uuid, reason: &str) -> DbResult<bool> {
    let mut transaction = pool.begin().await?;
    hold(&mut transaction, project_id).await?;
    let paused = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE projects SET auto_paused_reason = $3, auto_stalled = TRUE, \
           auto_claimed_at = NULL, updated_at = NOW() \
         WHERE id = $1 AND auto AND auto_paused_reason IS NULL \
           AND NOT EXISTS (SELECT 1 {IN_FLIGHT}) AND NOT EXISTS (SELECT 1 {RUNNABLE}) \
           AND EXISTS (SELECT 1 {REMAINING} AND t.status <> $4)"
    )))
    .bind(project_id)
    .bind(tasks::CLOSED.as_slice())
    .bind(reason)
    .bind(tasks::PAUSED)
    .execute(&mut *transaction)
    .await?
    .rows_affected()
        == 1;
    transaction.commit().await?;
    Ok(paused)
}

/// Clear a project's pause so the driver picks it up again; false when it does not exist.
pub async fn resume(pool: &PgPool, project_id: Uuid) -> DbResult<bool> {
    Ok(sqlx::query(
        "UPDATE projects SET auto_paused_reason = NULL, auto_stalled = FALSE, updated_at = NOW() \
         WHERE id = $1 AND auto_paused_reason IS NOT NULL",
    )
    .bind(project_id)
    .execute(pool)
    .await?
    .rows_affected()
        == 1)
}

/// What a parked row shows while a person withholds its task. Shown only:
/// `parked_stage` is what marks the row as parked.
pub const PAUSED_BY_PERSON: &str = "The task was paused";
pub const CANCELLED_BY_PERSON: &str = "The task was cancelled";

/// A row parked already keeps the stage and reason saved when it was first
/// parked; only the reason it shows follows its task between paused and
/// cancelled. A row automation paused for its own reasons is not parked, so
/// un-withholding its task does not release it, and a merge is a fact.
pub async fn park(pool: &PgPool, project_id: Uuid) -> DbResult<()> {
    sqlx::query(
        "UPDATE task_automation a SET \
           parked_stage = CASE WHEN a.stage = $2 THEN a.parked_stage ELSE a.stage END, \
           parked_reason = CASE WHEN a.stage = $2 THEN a.parked_reason ELSE a.reason END, \
           stage = $2, reason = CASE WHEN t.status = $4 THEN $5 ELSE $6 END, updated_at = NOW() \
         FROM tasks t WHERE t.id = a.task_id AND a.project_id = $1 AND t.status = ANY($3) \
           AND a.stage <> $7 \
           AND (a.stage <> $2 OR (a.parked_stage IS NOT NULL \
             AND a.reason IS DISTINCT FROM CASE WHEN t.status = $4 THEN $5 ELSE $6 END))",
    )
    .bind(project_id)
    .bind(Stage::Paused.as_str())
    .bind(tasks::WITHHELD.as_slice())
    .bind(tasks::PAUSED)
    .bind(PAUSED_BY_PERSON)
    .bind(CANCELLED_BY_PERSON)
    .bind(Stage::Merged.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

/// A row goes back to the stage and reason it was parked with, except where
/// its task has outgrown them: one a person completed has nothing left to
/// drive, and a pull request the pipeline was not yet driving, opened by a run
/// a person started meanwhile or by one not settled when the row was parked,
/// is checked from the start. A task a person started a run on stays parked
/// until that run ends. The clock starts over so the time spent parked does
/// not count against checks and reviews.
pub async fn unpark(pool: &PgPool, project_id: Uuid) -> DbResult<()> {
    let driving: Vec<&str> = Stage::DRIVING.into_iter().map(Stage::as_str).collect();
    sqlx::query(
        "UPDATE task_automation a SET \
           stage = CASE WHEN t.status = $4 THEN $5 \
             WHEN t.pr_url IS NOT NULL AND a.parked_stage <> ALL($6) THEN $7 \
             ELSE a.parked_stage END, \
           reason = CASE \
             WHEN t.status = $4 OR (t.pr_url IS NOT NULL AND a.parked_stage <> ALL($6)) THEN NULL \
             ELSE a.parked_reason END, \
           parked_stage = NULL, parked_reason = NULL, \
           checks_since = CASE WHEN a.checks_since IS NULL THEN NULL ELSE NOW() END, \
           updated_at = NOW() \
         FROM tasks t WHERE t.id = a.task_id AND a.project_id = $1 AND a.stage = $2 \
           AND a.parked_stage IS NOT NULL AND t.status <> ALL($3) AND t.active_run_id IS NULL",
    )
    .bind(project_id)
    .bind(Stage::Paused.as_str())
    .bind(tasks::WITHHELD.as_slice())
    .bind(tasks::COMPLETE)
    .bind(Stage::Idle.as_str())
    .bind(driving)
    .bind(Stage::AwaitingChecks.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

/// Open the auto projects an agentic task belongs to again: one that stalled
/// or finished while a person withheld the task has work to do once they move
/// it back. A pause no task caused stays until the project is resumed. Called
/// after that move has committed, never inside it, so no project row is locked
/// while a task row is held. The rows are locked in id order, and read for a
/// stop to undo only once locked, as [`hold`] has it.
pub async fn reopen(pool: &PgPool, task_id: Uuid) -> DbResult<()> {
    let mut transaction = pool.begin().await?;
    let held: Vec<Uuid> = sqlx::query_scalar(
        "SELECT p.id FROM projects p JOIN task_projects tp ON tp.project_id = p.id \
           JOIN tasks t ON t.id = tp.task_id \
         WHERE tp.task_id = $1 AND p.auto AND t.is_agentic \
         ORDER BY p.id FOR NO KEY UPDATE OF p",
    )
    .bind(task_id)
    .fetch_all(&mut *transaction)
    .await?;
    if !held.is_empty() {
        sqlx::query(
            "UPDATE projects SET \
               auto_paused_reason = CASE WHEN auto_stalled THEN NULL ELSE auto_paused_reason END, \
               auto_stalled = FALSE, auto_completed_at = NULL, updated_at = NOW() \
             WHERE id = ANY($1) AND auto AND (auto_stalled OR auto_completed_at IS NOT NULL)",
        )
        .bind(&held)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Put every paused task of a project back where it was going: a task with a
/// pull request is checked again, one without is started again. A task a
/// person paused or cancelled stays parked until they move it themselves.
pub async fn resume_paused_tasks(pool: &PgPool, project_id: Uuid) -> DbResult<u64> {
    let checked = sqlx::query(
        "UPDATE task_automation a SET stage = 'awaiting_checks', reason = NULL, \
           parked_stage = NULL, parked_reason = NULL, updated_at = NOW() \
         FROM tasks t WHERE t.id = a.task_id AND a.project_id = $1 AND a.stage = 'paused' \
           AND t.pr_url IS NOT NULL AND t.status <> 'complete' AND t.status <> ALL($2)",
    )
    .bind(project_id)
    .bind(tasks::WITHHELD.as_slice())
    .execute(pool)
    .await?
    .rows_affected();
    // Admission only reads tasks in `created`; a task that exhausted its runs
    // sits in `blocked` (or `review` without a pull request), so it is put
    // back to `created` with its counter, or resume would change nothing. A
    // task someone started a run on meanwhile is left alone, and the
    // candidates are locked before either table changes so an admission
    // cannot slip in between the check and the reset.
    let restarted = sqlx::query(
        "WITH candidates AS ( \
           SELECT t.id FROM tasks t JOIN task_automation a ON a.task_id = t.id \
           WHERE a.project_id = $1 AND a.stage = 'paused' AND t.pr_url IS NULL \
             AND t.status <> 'complete' AND t.status <> ALL($2) AND t.active_run_id IS NULL \
           FOR UPDATE OF t), \
         restarted AS ( \
           UPDATE task_automation a SET stage = 'idle', reason = NULL, runs = 0, \
             parked_stage = NULL, parked_reason = NULL, updated_at = NOW() \
           FROM candidates c WHERE a.task_id = c.id \
           RETURNING a.task_id) \
         UPDATE tasks SET status = 'created', updated_at = NOW() \
         WHERE id IN (SELECT task_id FROM restarted)",
    )
    .bind(project_id)
    .bind(tasks::WITHHELD.as_slice())
    .execute(pool)
    .await?
    .rows_affected();
    Ok(checked + restarted)
}

/// Record that no agentic task of the project has work left; false when the
/// project does not exist or one has by now, as when a person un-cancels a
/// task mid-pass. The next pass then decides afresh.
pub async fn complete(pool: &PgPool, project_id: Uuid) -> DbResult<bool> {
    let mut transaction = pool.begin().await?;
    hold(&mut transaction, project_id).await?;
    let completed = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE projects SET auto_completed_at = NOW(), auto_claimed_at = NULL, updated_at = NOW() \
         WHERE id = $1 AND auto_completed_at IS NULL AND NOT EXISTS (SELECT 1 {REMAINING})"
    )))
    .bind(project_id)
    .bind(tasks::CLOSED.as_slice())
    .execute(&mut *transaction)
    .await?
    .rows_affected()
        == 1;
    transaction.commit().await?;
    Ok(completed)
}

/// Where a task is in the pipeline that takes its change from a run to a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Running,
    NoChanges,
    AwaitingChecks,
    AwaitingReviews,
    Fixing,
    Merging,
    PostMerge,
    Merged,
    Paused,
}

impl Stage {
    /// The stages at which the pipeline is driving a task's pull request.
    pub const DRIVING: [Self; 5] = [
        Self::AwaitingChecks,
        Self::AwaitingReviews,
        Self::Fixing,
        Self::Merging,
        Self::PostMerge,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::NoChanges => "no_changes",
            Self::AwaitingChecks => "awaiting_checks",
            Self::AwaitingReviews => "awaiting_reviews",
            Self::Fixing => "fixing",
            Self::Merging => "merging",
            Self::PostMerge => "post_merge",
            Self::Merged => "merged",
            Self::Paused => "paused",
        }
    }

    /// The stage a stored name denotes, if any.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "idle" => Self::Idle,
            "running" => Self::Running,
            "no_changes" => Self::NoChanges,
            "awaiting_checks" => Self::AwaitingChecks,
            "awaiting_reviews" => Self::AwaitingReviews,
            "fixing" => Self::Fixing,
            "merging" => Self::Merging,
            "post_merge" => Self::PostMerge,
            "merged" => Self::Merged,
            "paused" => Self::Paused,
            _ => return None,
        })
    }

    /// Whether a task at this stage still occupies one of the project's slots.
    pub fn in_flight(self) -> bool {
        matches!(
            self,
            Self::Running
                | Self::AwaitingChecks
                | Self::AwaitingReviews
                | Self::Fixing
                | Self::Merging
        )
    }
}

/// What a task is for, which decides where it sits in a project's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Scaffold,
    Ci,
    Tests,
    Feature,
    Deployment,
    Docs,
    Fix,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scaffold => "scaffold",
            Self::Ci => "ci",
            Self::Tests => "tests",
            Self::Feature => "feature",
            Self::Deployment => "deployment",
            Self::Docs => "docs",
            Self::Fix => "fix",
        }
    }

    /// The task kind a stored name denotes, if any.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "scaffold" => Self::Scaffold,
            "ci" => Self::Ci,
            "tests" => Self::Tests,
            "feature" => Self::Feature,
            "deployment" => Self::Deployment,
            "docs" => Self::Docs,
            "fix" => Self::Fix,
            _ => return None,
        })
    }
}

/// One task's automation row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskAutomation {
    pub task_id: Uuid,
    pub project_id: Uuid,
    pub kind: Option<String>,
    pub stage: String,
    pub reason: Option<String>,
    pub runs: i32,
    pub review_rounds: i32,
    pub head: Option<String>,
    pub checks: Option<String>,
    pub checks_since: Option<DateTime<Utc>>,
    pub bot_trigger_head: Option<String>,
    pub merge_sha: Option<String>,
    pub auto_created: bool,
    pub last_run_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

impl TaskAutomation {
    /// Where the task is in the pipeline.
    pub fn stage(&self) -> Stage {
        Stage::parse(&self.stage).unwrap_or(Stage::Idle)
    }

    /// What kind of task this is, when the planner or the driver said.
    pub fn kind(&self) -> Option<Kind> {
        self.kind.as_deref().and_then(Kind::parse)
    }
}

const TASK_COLUMNS: &str = "task_id, project_id, kind, stage, reason, runs, review_rounds, head, checks, \
     checks_since, bot_trigger_head, merge_sha, auto_created, last_run_id, updated_at";

/// A task's automation row, if the driver has touched the task.
pub async fn task_stage(pool: &PgPool, task_id: Uuid) -> DbResult<Option<TaskAutomation>> {
    sqlx::query_as::<_, TaskAutomation>(sqlx::AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM task_automation WHERE task_id = $1"
    )))
    .bind(task_id)
    .fetch_optional(pool)
    .await
}

/// Move a task to a stage, creating its row on the way if it has none. What
/// a park saved goes with the move: `task_automation_parked_check` lets only
/// a paused row carry it, and a row automation pauses here is not one a
/// person's move un-parks.
pub async fn set_stage(
    pool: &PgPool,
    task_id: Uuid,
    project_id: Uuid,
    stage: Stage,
    reason: Option<&str>,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO task_automation (task_id, project_id, stage, reason) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (task_id) DO UPDATE SET stage = EXCLUDED.stage, reason = EXCLUDED.reason, \
         parked_stage = NULL, parked_reason = NULL, updated_at = NOW()",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(stage.as_str())
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record what kind of task this is, creating its automation row when needed.
pub async fn set_kind(
    connection: &mut PgConnection,
    task_id: Uuid,
    project_id: Uuid,
    kind: Kind,
    auto_created: bool,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO task_automation (task_id, project_id, kind, auto_created) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (task_id) DO UPDATE SET kind = EXCLUDED.kind, \
         auto_created = task_automation.auto_created OR EXCLUDED.auto_created, updated_at = NOW()",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(kind.as_str())
    .bind(auto_created)
    .execute(connection)
    .await?;
    Ok(())
}

/// Record the head the pipeline is looking at and what its checks said.
///
/// `fresh` restarts the clock that decides when silent checks count as absent;
/// a re-read of the same head keeps the clock it already had.
pub async fn set_head(
    pool: &PgPool,
    task_id: Uuid,
    head: &str,
    checks: Option<&str>,
    fresh: bool,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE task_automation SET head = $2, checks = $3, \
           checks_since = CASE WHEN $4 OR checks_since IS NULL THEN NOW() ELSE checks_since END, \
           updated_at = NOW() WHERE task_id = $1",
    )
    .bind(task_id)
    .bind(head)
    .bind(checks)
    .bind(fresh)
    .execute(pool)
    .await?;
    Ok(())
}

/// Start the wait for reviews over: the grace a bot gets is measured from
/// when the change became reviewable, and again from when the bot was asked.
pub async fn restart_clock(pool: &PgPool, task_id: Uuid) -> DbResult<()> {
    sqlx::query(
        "UPDATE task_automation SET checks_since = NOW(), updated_at = NOW() WHERE task_id = $1",
    )
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remember the head a review bot was asked to review, so it is asked once per head.
pub async fn set_bot_trigger_head(pool: &PgPool, task_id: Uuid, head: &str) -> DbResult<()> {
    sqlx::query(
        "UPDATE task_automation SET bot_trigger_head = $2, updated_at = NOW() WHERE task_id = $1",
    )
    .bind(task_id)
    .bind(head)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record the commit the squash merge produced.
pub async fn set_merge_sha(pool: &PgPool, task_id: Uuid, sha: &str) -> DbResult<()> {
    sqlx::query("UPDATE task_automation SET merge_sha = $2, updated_at = NOW() WHERE task_id = $1")
        .bind(task_id)
        .bind(sha)
        .execute(pool)
        .await?;
    Ok(())
}

/// Count a run admitted for a task and put the task in the running stage.
pub async fn record_admission(
    pool: &PgPool,
    task_id: Uuid,
    project_id: Uuid,
    run_id: Uuid,
) -> DbResult<i32> {
    sqlx::query_scalar(
        "INSERT INTO task_automation (task_id, project_id, stage, runs, last_run_id) \
         VALUES ($1, $2, 'running', 1, $3) \
         ON CONFLICT (task_id) DO UPDATE SET stage = 'running', reason = NULL, \
           parked_stage = NULL, parked_reason = NULL, \
           runs = task_automation.runs + 1, last_run_id = EXCLUDED.last_run_id, updated_at = NOW() \
         RETURNING runs",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(run_id)
    .fetch_one(pool)
    .await
}

/// Count one more review round and return the new total.
pub async fn bump_review_round(pool: &PgPool, task_id: Uuid) -> DbResult<i32> {
    sqlx::query_scalar(
        "UPDATE task_automation SET review_rounds = review_rounds + 1, updated_at = NOW() \
         WHERE task_id = $1 RETURNING review_rounds",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
}

/// Every task of a project the pipeline still has something to do for.
pub async fn pipeline(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<TaskAutomation>> {
    sqlx::query_as::<_, TaskAutomation>(sqlx::AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM task_automation WHERE project_id = $1 \
           AND stage NOT IN ('idle', 'running', 'merged', 'paused') \
         ORDER BY updated_at, task_id"
    )))
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// A task whose run has ended, with what the run left behind.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SettledRun {
    pub task_id: Uuid,
    pub status: String,
    pub pr_url: Option<String>,
    pub last_run_id: Option<Uuid>,
    pub error: Option<String>,
}

/// Tasks the driver sent running whose run has since ended.
pub async fn settled_runs(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<SettledRun>> {
    sqlx::query_as::<_, SettledRun>(
        "SELECT a.task_id, t.status, t.pr_url, a.last_run_id, \
                (SELECT r.error_message FROM task_runs r WHERE r.id = a.last_run_id) AS error \
         FROM task_automation a JOIN tasks t ON t.id = a.task_id \
         WHERE a.project_id = $1 AND a.stage = 'running' AND t.active_run_id IS NULL \
         ORDER BY a.updated_at, a.task_id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// Agentic tasks a manual project brought with it when automation was turned
/// on: settled before the driver knew them, with no row of their own yet.
pub async fn enrollable(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<SettledRun>> {
    sqlx::query_as::<_, SettledRun>(
        "SELECT t.id AS task_id, t.status, t.pr_url, NULL::uuid AS last_run_id, NULL::text AS error \
         FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
         WHERE tp.project_id = $1 AND t.is_agentic AND t.active_run_id IS NULL \
           AND t.status IN ('review', 'blocked', 'in_progress', 'queued') \
           AND NOT EXISTS (SELECT 1 FROM task_automation a WHERE a.task_id = t.id) \
         ORDER BY t.created_at, t.id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// The next task a project can start: created, agentic, and with every task it
/// depends on complete or cancelled. A person cancelling a task takes it out
/// of the plan, so nothing waits on it; a paused one is still to be done.
/// Priority first, then age.
pub async fn next_runnable(pool: &PgPool, project_id: Uuid) -> DbResult<Option<Uuid>> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT t.id {RUNNABLE} ORDER BY t.priority NULLS LAST, t.created_at, t.id LIMIT 1"
    )))
    .bind(project_id)
    .bind(tasks::CLOSED.as_slice())
    .fetch_optional(pool)
    .await
}

/// The tasks project `$1` can start, given [`tasks::CLOSED`] as `$2`. Shared
/// by the driver's read and by [`pause_stalled`], which stops the project
/// only while that read still holds.
const RUNNABLE: &str = "FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
     WHERE tp.project_id = $1 AND t.status = 'created' AND t.is_agentic \
       AND t.active_run_id IS NULL \
       AND NOT EXISTS ( \
         SELECT 1 FROM jsonb_array_elements_text( \
           CASE WHEN jsonb_typeof(t.dependencies) = 'array' THEN t.dependencies ELSE '[]'::jsonb END \
         ) AS d(id) JOIN tasks p ON p.id::text = d.id WHERE p.status <> ALL($2))";

/// Tasks occupying one of the project's slots: running, or between a run and a merge.
pub async fn in_flight(pool: &PgPool, project_id: Uuid) -> DbResult<i64> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) {IN_FLIGHT}")))
        .bind(project_id)
        .fetch_one(pool)
        .await
}

/// The tasks holding a slot of project `$1`, shared with [`pause_stalled`] as
/// [`RUNNABLE`] is.
const IN_FLIGHT: &str = "FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
     LEFT JOIN task_automation a ON a.task_id = t.id \
     WHERE tp.project_id = $1 AND t.status <> 'complete' \
       AND (t.active_run_id IS NOT NULL \
            OR a.stage IN ('running', 'awaiting_checks', 'awaiting_reviews', 'fixing', 'merging'))";

/// Agentic tasks of the project not yet complete. A cancelled task is not
/// left to do, so it does not hold the project open.
pub async fn remaining(pool: &PgPool, project_id: Uuid) -> DbResult<i64> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) {REMAINING}")))
        .bind(project_id)
        .bind(tasks::CLOSED.as_slice())
        .fetch_one(pool)
        .await
}

/// The agentic tasks of project `$1` with work left, given [`tasks::CLOSED`]
/// as `$2`. Shared by the driver's read and by [`complete`] and
/// [`pause_stalled`], which stop the project only while that read still holds.
const REMAINING: &str = "FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
     WHERE tp.project_id = $1 AND t.is_agentic AND t.status <> ALL($2)";

/// Runs the driver started that are still going, across every project.
pub async fn active_unattended_runs(pool: &PgPool) -> DbResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_runs WHERE unattended AND status IN ('running', 'waiting')",
    )
    .fetch_one(pool)
    .await
}

/// The project's continuous-integration task, and its status. One a person
/// cancelled is not the project's any more: the project has none.
pub async fn ci_task(pool: &PgPool, project_id: Uuid) -> DbResult<Option<(Uuid, String)>> {
    sqlx::query_as::<_, (Uuid, String)>(
        "SELECT t.id, t.status FROM task_automation a JOIN tasks t ON t.id = a.task_id \
         WHERE a.project_id = $1 AND a.kind = $2 AND t.status <> $3 \
         ORDER BY t.created_at LIMIT 1",
    )
    .bind(project_id)
    .bind(Kind::Ci.as_str())
    .bind(tasks::CANCELLED)
    .fetch_optional(pool)
    .await
}

/// How many tasks of a kind the driver itself added to the project.
pub async fn count_auto_created(pool: &PgPool, project_id: Uuid, kind: Kind) -> DbResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_automation WHERE project_id = $1 AND auto_created AND kind = $2",
    )
    .bind(project_id)
    .bind(kind.as_str())
    .fetch_one(pool)
    .await
}

/// One task of a project as the console and the roadmap see it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectTask {
    pub task_id: Uuid,
    pub title: String,
    pub status: String,
    pub is_agentic: bool,
    pub priority: Option<i32>,
    pub pr_url: Option<String>,
    pub dependencies: Option<Value>,
    pub kind: Option<String>,
    pub stage: Option<String>,
    pub reason: Option<String>,
    pub runs: Option<i32>,
    pub review_rounds: Option<i32>,
    pub head: Option<String>,
    pub checks: Option<String>,
    pub merge_sha: Option<String>,
    pub auto_created: Option<bool>,
    pub reviewers: Option<String>,
}

/// Every task of a project with its automation state, for the report.
pub async fn project_tasks(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<ProjectTask>> {
    sqlx::query_as::<_, ProjectTask>(
        "SELECT t.id AS task_id, t.title, t.status, t.is_agentic, t.priority, t.pr_url, t.dependencies, \
                a.kind, a.stage, a.reason, a.runs, a.review_rounds, a.head, a.checks, a.merge_sha, \
                a.auto_created, \
                (SELECT string_agg(DISTINCT r.reviewer, ', ') FROM task_reviews r \
                   WHERE r.task_id = t.id AND r.verdict <> ALL($2)) AS reviewers \
         FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
         LEFT JOIN task_automation a ON a.task_id = t.id \
         WHERE tp.project_id = $1 \
         ORDER BY t.priority NULLS LAST, t.created_at, t.id",
    )
    .bind(project_id)
    .bind(Verdict::verdictless())
    .fetch_all(pool)
    .await
}

/// One thing a reviewer asked to have changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// The review thread a bot raised it in, so a fix can answer and resolve it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewerKind {
    Model,
    Bot,
}

impl ReviewerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Bot => "bot",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    RequestChanges,
    /// The reviewer answered without a readable verdict.
    Unparseable,
    /// The reviewer model failed before it answered.
    Failed,
}

impl Verdict {
    pub const ALL: [Self; 4] = [
        Self::Approve,
        Self::RequestChanges,
        Self::Unparseable,
        Self::Failed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::RequestChanges => "request_changes",
            Self::Unparseable => "unparseable",
            Self::Failed => "failed",
        }
    }

    /// The variant a stored name denotes, if any.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|verdict| verdict.as_str() == value)
    }

    /// Whether the round ended without judging the change: it neither
    /// approves nor blocks it, and does not count as anyone's review.
    pub const fn is_verdictless(self) -> bool {
        matches!(self, Self::Unparseable | Self::Failed)
    }

    /// The stored names of the verdictless rounds.
    pub fn verdictless() -> Vec<&'static str> {
        Self::ALL
            .into_iter()
            .filter(|verdict| verdict.is_verdictless())
            .map(Self::as_str)
            .collect()
    }
}

#[derive(Debug, Error)]
#[error("{0:?} is not a review verdict")]
pub struct UnknownVerdict(String);

impl TryFrom<String> for Verdict {
    type Error = UnknownVerdict;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or(UnknownVerdict(value))
    }
}

pub struct ReviewInsert<'a> {
    pub task_id: Uuid,
    pub run_id: Option<Uuid>,
    pub round: i32,
    pub head: &'a str,
    pub reviewer_kind: ReviewerKind,
    pub reviewer: &'a str,
    pub author_model: Option<&'a str>,
    pub same_model: bool,
    pub verdict: Verdict,
    pub summary: &'a str,
    pub findings: &'a [Finding],
    pub addressed: &'a [String],
    pub external_id: Option<&'a str>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReviewRow {
    pub id: Uuid,
    pub task_id: Uuid,
    pub run_id: Option<Uuid>,
    pub round: i32,
    pub head: String,
    pub reviewer_kind: String,
    pub reviewer: String,
    pub author_model: Option<String>,
    pub same_model: bool,
    #[sqlx(try_from = "String")]
    pub verdict: Verdict,
    pub summary: String,
    pub findings: Value,
    pub addressed: Value,
    pub external_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl ReviewRow {
    /// Total: a malformed entry is dropped rather than failing the whole read.
    pub fn findings(&self) -> Vec<Finding> {
        self.findings
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| serde_json::from_value(item.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The finding ids this round declared addressed.
    pub fn addressed(&self) -> Vec<String> {
        self.addressed
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether this round judged the change.
    pub fn reviewed(&self) -> bool {
        !self.verdict.is_verdictless()
    }

    /// Whether a review bot, rather than a Zone reviewer session, wrote this round.
    pub fn is_bot(&self) -> bool {
        self.reviewer_kind == ReviewerKind::Bot.as_str()
    }
}

const REVIEW_COLUMNS: &str = "id, task_id, run_id, round, head, reviewer_kind, reviewer, author_model, same_model, \
     verdict, summary, findings, addressed, external_id, created_at";

/// Store one review round and return its id.
pub async fn record_review(pool: &PgPool, review: ReviewInsert<'_>) -> DbResult<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO task_reviews (task_id, run_id, round, head, reviewer_kind, reviewer, author_model, \
           same_model, verdict, summary, findings, addressed, external_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING id",
    )
    .bind(review.task_id)
    .bind(review.run_id)
    .bind(review.round)
    .bind(review.head)
    .bind(review.reviewer_kind.as_str())
    .bind(review.reviewer)
    .bind(review.author_model)
    .bind(review.same_model)
    .bind(review.verdict.as_str())
    .bind(review.summary)
    .bind(json!(review.findings))
    .bind(json!(review.addressed))
    .bind(review.external_id)
    .fetch_one(pool)
    .await
}

/// Every review round of a task, oldest first.
pub async fn reviews(pool: &PgPool, task_id: Uuid) -> DbResult<Vec<ReviewRow>> {
    sqlx::query_as::<_, ReviewRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {REVIEW_COLUMNS} FROM task_reviews WHERE task_id = $1 ORDER BY round, created_at"
    )))
    .bind(task_id)
    .fetch_all(pool)
    .await
}

/// The number of the last review round recorded; zero before any.
pub async fn latest_round(pool: &PgPool, task_id: Uuid) -> DbResult<i32> {
    sqlx::query_scalar("SELECT COALESCE(MAX(round), 0) FROM task_reviews WHERE task_id = $1")
        .bind(task_id)
        .fetch_one(pool)
        .await
}

/// The findings nobody has yet addressed, from every round of review.
///
/// A model's findings accumulate across rounds until a later round names them
/// as addressed. A bot's findings are whatever its latest round listed: a bot
/// re-reads the whole change on every head, so a thread it no longer lists has
/// been resolved and one it still lists is still open.
pub fn open_findings_of(rows: &[ReviewRow]) -> Vec<Finding> {
    let addressed: HashSet<String> = rows.iter().flat_map(ReviewRow::addressed).collect();
    let mut open: Vec<Finding> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for row in rows {
        if row.is_bot() {
            let latest = rows
                .iter()
                .filter(|other| other.is_bot() && other.reviewer == row.reviewer)
                .max_by_key(|other| (other.round, other.created_at));
            if latest.map(|latest| latest.id) != Some(row.id) {
                continue;
            }
        }
        for finding in row.findings() {
            let key = finding
                .thread_id
                .clone()
                .unwrap_or_else(|| finding.id.clone());
            if addressed.contains(&finding.id)
                || finding
                    .thread_id
                    .as_ref()
                    .is_some_and(|thread| addressed.contains(thread))
                || !seen.insert(key)
            {
                continue;
            }
            open.push(finding);
        }
    }
    open
}

/// Every finding raised on a task that no later round has addressed.
pub async fn open_findings(pool: &PgPool, task_id: Uuid) -> DbResult<Vec<Finding>> {
    Ok(open_findings_of(&reviews(pool, task_id).await?))
}

/// Whether a review by something other than the author's model has been
/// recorded on this head: a Zone review on a different model, or any bot. A
/// round that ended without a verdict reviewed nothing, whoever ran it.
pub async fn distinct_review_on_head(pool: &PgPool, task_id: Uuid, head: &str) -> DbResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_reviews WHERE task_id = $1 AND head = $2 \
           AND (reviewer_kind = 'bot' OR (NOT same_model AND author_model IS NOT NULL)) \
           AND verdict <> ALL($3))",
    )
    .bind(task_id)
    .bind(head)
    .bind(Verdict::verdictless())
    .fetch_one(pool)
    .await
}

/// A task the planner wrote, before it has an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTask {
    pub kind: Kind,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Option<String>,
    /// Indices into the plan's task list, each earlier than this task's own.
    pub depends_on: Vec<usize>,
    pub priority: Option<i32>,
}

pub struct PlannedRepository<'a> {
    pub url: &'a str,
    pub token: Option<&'a str>,
    pub github_installation_id: Option<Uuid>,
}

pub struct ProjectPlan<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub repository: Option<PlannedRepository<'a>>,
    pub brief: &'a Value,
    pub tasks: &'a [PlannedTask],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finalized {
    pub project_id: Uuid,
    pub task_ids: Vec<Uuid>,
    pub updates_chat_id: Uuid,
}

/// Create a project and every task the plan names, in one transaction.
///
/// The planner chat, when there is one, is bound to the project here and
/// refused a second project: one interview makes one project. The updates
/// chat every notice about the project lands in is made at the same time.
pub async fn finalize(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: Uuid,
    chat_id: Option<Uuid>,
    origin: Origin,
    plan: &ProjectPlan<'_>,
) -> DbResult<Finalized> {
    for (index, task) in plan.tasks.iter().enumerate() {
        if task
            .depends_on
            .iter()
            .any(|dependency| *dependency >= index)
        {
            return Err(invalid("A task may only depend on tasks listed before it"));
        }
    }
    let mut transaction = pool.begin().await?;
    super::actions::authorize(&mut transaction, workspace_id, actor, true).await?;
    if let Some(chat_id) = chat_id {
        let bound: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT project_id FROM chats WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
        )
        .bind(chat_id)
        .bind(workspace_id)
        .fetch_optional(&mut *transaction)
        .await?;
        match bound {
            None => return Err(invalid("Chat not found in this workspace")),
            Some(Some(_)) => return Err(invalid("This chat has already created its project")),
            Some(None) => {}
        }
    }
    super::projects::lock_order(&mut transaction, workspace_id).await?;
    let project_id: Uuid = sqlx::query_scalar(
        "INSERT INTO projects (name, description, workspace_id, status, auto, auto_actor_id, brief, \
           github_repo_url, github_access_token, github_installation_id, \"position\") \
         VALUES ($1, $2, $3, 'active', TRUE, $4, $5, $6, $7, $8, \
           COALESCE((SELECT MIN(\"position\") FROM projects WHERE workspace_id = $3), 1) - 1) \
         RETURNING id",
    )
    .bind(plan.name)
    .bind(plan.description)
    .bind(workspace_id)
    .bind(actor)
    .bind(plan.brief)
    .bind(plan.repository.as_ref().map(|repository| repository.url))
    .bind(plan.repository.as_ref().and_then(|repository| repository.token))
    .bind(
        plan.repository
            .as_ref()
            .and_then(|repository| repository.github_installation_id),
    )
    .fetch_one(&mut *transaction)
    .await?;

    let mut task_ids: Vec<Uuid> = Vec::with_capacity(plan.tasks.len());
    for task in plan.tasks {
        let created = tasks::create_task_in(
            &mut transaction,
            &Create {
                workspace_id,
                project_ids: &[project_id],
                title: Some(task.title.trim()),
                description: task.description.trim(),
                acceptance_criteria: task.acceptance_criteria.as_deref(),
                priority: task.priority,
                is_agentic: true,
                require_plan_approval: false,
                source_id: None,
                created_by: Some(actor),
                chat_id: None,
                origin,
            },
        )
        .await
        .map_err(MutationError::into_database)?;
        set_kind(&mut transaction, created.id, project_id, task.kind, false).await?;
        task_ids.push(created.id);
    }
    for (task, id) in plan.tasks.iter().zip(&task_ids) {
        let dependencies: Vec<Uuid> = task
            .depends_on
            .iter()
            .map(|index| task_ids[*index])
            .collect();
        tasks::set_dependencies_in(&mut transaction, *id, &dependencies).await?;
    }
    if let Some(chat_id) = chat_id {
        sqlx::query("UPDATE chats SET project_id = $2, updated_at = NOW() WHERE id = $1")
            .bind(chat_id)
            .bind(project_id)
            .execute(&mut *transaction)
            .await?;
    }
    let updates_chat_id = super::chats::create_project_chat_in(
        &mut transaction,
        workspace_id,
        &format!("{} · updates", plan.name.trim()),
        super::chats::ChatPurpose::ProjectUpdates,
        project_id,
    )
    .await?;
    transaction.commit().await?;
    Ok(Finalized {
        project_id,
        task_ids,
        updates_chat_id,
    })
}

/// A task in one of these has not started, so continuous integration can
/// still go ahead of it. A paused one counts: it starts once it is un-paused.
const YET_TO_START: [&str; 2] = [tasks::CREATED, tasks::PAUSED];

/// Add a task the driver decided the project needs: continuous integration a
/// repository turned out not to have, or a fix for a job that failed after a
/// merge. A `ci` task goes ahead of everything still waiting to start.
#[allow(clippy::too_many_arguments)]
pub async fn insert_auto_task(
    pool: &PgPool,
    project_id: Uuid,
    workspace_id: Uuid,
    actor: Uuid,
    kind: Kind,
    title: &str,
    description: &str,
    acceptance_criteria: Option<&str>,
) -> DbResult<Uuid> {
    let mut transaction = pool.begin().await?;
    let created = tasks::create_task_in(
        &mut transaction,
        &Create {
            workspace_id,
            project_ids: &[project_id],
            title: Some(title),
            description,
            acceptance_criteria,
            priority: Some(1),
            is_agentic: true,
            require_plan_approval: false,
            source_id: None,
            created_by: Some(actor),
            chat_id: None,
            origin: Origin::Automation,
        },
    )
    .await
    .map_err(MutationError::into_database)?;
    set_kind(&mut transaction, created.id, project_id, kind, true).await?;
    if kind == Kind::Ci {
        sqlx::query(
            "UPDATE tasks t SET dependencies = \
               (CASE WHEN jsonb_typeof(t.dependencies) = 'array' THEN t.dependencies ELSE '[]'::jsonb END) \
               || jsonb_build_array($2::text), updated_at = NOW() \
             FROM task_projects tp \
             WHERE tp.task_id = t.id AND tp.project_id = $1 AND t.id <> $3 \
               AND t.is_agentic AND t.status = ANY($4)",
        )
        .bind(project_id)
        .bind(created.id.to_string())
        .bind(created.id)
        .bind(YET_TO_START.as_slice())
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(created.id)
}

#[cfg(test)]
pub(crate) mod testing {
    use serde_json::json;
    use sqlx::{PgPool, Postgres, Transaction};
    use std::time::Duration;
    use uuid::Uuid;

    use super::{Kind, PlannedTask, ProjectPlan, Stage, TaskAutomation, finalize, set_stage};
    use crate::db::tasks::{self, Origin};
    use crate::db::{organizations, users, workspace_members, workspaces};

    pub struct Project {
        pub pool: PgPool,
        pub organization: Uuid,
        pub workspace: Uuid,
        pub actor: Uuid,
        pub id: Uuid,
        pub tasks: Vec<Uuid>,
    }

    /// A project row locked by a session of the test's own.
    pub struct RowHold {
        pub transaction: Transaction<'static, Postgres>,
        backend: i32,
    }

    impl RowHold {
        pub async fn release(self) {
            self.transaction
                .commit()
                .await
                .expect("the held row is released");
        }
    }

    const QUEUES_WITHIN: Duration = Duration::from_secs(10);

    impl Project {
        pub async fn new(count: usize) -> Self {
            let pool = PgPool::connect(
                &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
            )
            .await
            .expect("the test database");
            let suffix = Uuid::new_v4().simple().to_string();
            let organization =
                organizations::create_organization(&pool, "Automation", &suffix, None)
                    .await
                    .expect("an organization");
            let workspace =
                workspaces::create_workspace(&pool, organization.id, "Automation", &suffix, None)
                    .await
                    .expect("a workspace");
            let actor = users::create_user(
                &pool,
                &format!("{suffix}@example.test"),
                "unused",
                Some("Actor"),
                false,
            )
            .await
            .expect("a user");
            workspace_members::add_member(
                &pool,
                workspace.id,
                actor.id,
                workspace_members::WorkspaceRole::Member,
                None,
            )
            .await
            .expect("a membership");
            let planned: Vec<PlannedTask> = (0..count)
                .map(|index| PlannedTask {
                    kind: Kind::Feature,
                    title: format!("Task {index}"),
                    description: "Ships a change.".into(),
                    acceptance_criteria: None,
                    depends_on: Vec::new(),
                    priority: None,
                })
                .collect();
            let finalized = finalize(
                &pool,
                workspace.id,
                actor.id,
                None,
                Origin::Member,
                &ProjectPlan {
                    name: "Automation",
                    description: None,
                    repository: None,
                    brief: &json!({}),
                    tasks: &planned,
                },
            )
            .await
            .expect("an auto project");
            Self {
                pool,
                organization: organization.id,
                workspace: workspace.id,
                actor: actor.id,
                id: finalized.project_id,
                tasks: finalized.task_ids,
            }
        }

        pub async fn place(&self, task: Uuid, status: &str, pr_url: Option<&str>, stage: Stage) {
            sqlx::query("UPDATE tasks SET status = $2, pr_url = $3 WHERE id = $1")
                .bind(task)
                .bind(status)
                .bind(pr_url)
                .execute(&self.pool)
                .await
                .expect("the task is placed");
            set_stage(&self.pool, task, self.id, stage, None)
                .await
                .expect("the task's stage");
        }

        pub async fn status(&self, task: Uuid) -> String {
            sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
                .bind(task)
                .fetch_one(&self.pool)
                .await
                .expect("the task's status")
        }

        pub async fn set_status(&self, task: Uuid, status: &str) {
            sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
                .bind(task)
                .bind(status)
                .execute(&self.pool)
                .await
                .expect("the task's status is set");
        }

        /// Move a task the way the console does, as the project's actor.
        pub async fn move_to(&self, task: Uuid, status: &str) {
            let moved = tasks::update_task_authorized(
                &self.pool,
                self.actor,
                tasks::Patch {
                    id: task,
                    title: None,
                    description: None,
                    acceptance_criteria: None,
                    status: tasks::Status::parse(status),
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
                matches!(moved, Ok(tasks::Mutation::Applied(_))),
                "the task moves to {status}: {moved:?}"
            );
        }

        pub async fn parked(&self, task: Uuid) -> (Option<String>, Option<String>) {
            sqlx::query_as(
                "SELECT parked_stage, parked_reason FROM task_automation WHERE task_id = $1",
            )
            .bind(task)
            .fetch_one(&self.pool)
            .await
            .expect("the task's automation")
        }

        /// The kinds of the notices the project's updates chat received, oldest first.
        pub async fn notices(&self) -> Vec<String> {
            sqlx::query_scalar(
                "SELECT m.metadata ->> 'kind' FROM messages m JOIN chats c ON c.id = m.chat_id \
                 WHERE c.project_id = $1 AND m.metadata ->> 'source' = 'auto_project' \
                 ORDER BY m.created_at, m.id",
            )
            .bind(self.id)
            .fetch_all(&self.pool)
            .await
            .expect("the project's notices")
        }

        pub async fn runs(&self, task: Uuid) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM task_runs WHERE task_id = $1")
                .bind(task)
                .fetch_one(&self.pool)
                .await
                .expect("the task's runs")
        }

        pub async fn automation(&self, task: Uuid) -> TaskAutomation {
            super::task_stage(&self.pool, task)
                .await
                .expect("the task's automation")
                .expect("an automated task")
        }

        /// Hold the project's row as a writer in mid-write does, until the
        /// returned transaction ends.
        pub async fn hold(&self) -> RowHold {
            let mut transaction = self.pool.begin().await.expect("a transaction");
            let backend = sqlx::query_scalar(
                "SELECT pg_backend_pid() FROM projects WHERE id = $1 FOR NO KEY UPDATE",
            )
            .bind(self.id)
            .fetch_one(&mut *transaction)
            .await
            .expect("the project's row");
            RowHold {
                transaction,
                backend,
            }
        }

        /// Wait until another session is queued behind `hold`.
        pub async fn queued_behind(&self, hold: &RowHold) {
            let queued = async {
                loop {
                    let waiting: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM pg_stat_activity \
                         WHERE wait_event_type = 'Lock' AND $1 = ANY(pg_blocking_pids(pid)))",
                    )
                    .bind(hold.backend)
                    .fetch_one(&self.pool)
                    .await
                    .expect("the sessions waiting on a lock");
                    if waiting {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            };
            tokio::time::timeout(QUEUES_WITHIN, queued)
                .await
                .expect("a writer queues for the project's row");
        }

        pub async fn remove(self) {
            sqlx::query("DELETE FROM organizations WHERE id = $1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .expect("the organization to be removed");
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(self.actor)
                .execute(&self.pool)
                .await
                .expect("the user to be removed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Project;
    use super::*;

    fn row(
        round: i32,
        kind: ReviewerKind,
        reviewer: &str,
        findings: Vec<Finding>,
        addressed: Vec<&str>,
    ) -> ReviewRow {
        ReviewRow {
            id: Uuid::new_v4(),
            task_id: Uuid::nil(),
            run_id: None,
            round,
            head: "h".into(),
            reviewer_kind: kind.as_str().into(),
            reviewer: reviewer.into(),
            author_model: None,
            same_model: false,
            verdict: Verdict::RequestChanges,
            summary: String::new(),
            findings: json!(findings),
            addressed: json!(addressed),
            external_id: None,
            created_at: Utc::now() + chrono::Duration::seconds(i64::from(round)),
        }
    }

    fn finding(id: &str, thread: Option<&str>) -> Finding {
        Finding {
            id: id.into(),
            severity: "major".into(),
            file: None,
            line: None,
            title: id.into(),
            detail: String::new(),
            thread_id: thread.map(str::to_string),
            reviewer: None,
        }
    }

    #[test]
    fn a_models_findings_stay_open_until_a_later_round_names_them() {
        let rows = vec![
            row(
                1,
                ReviewerKind::Model,
                "a",
                vec![finding("r1-1", None), finding("r1-2", None)],
                vec![],
            ),
            row(
                2,
                ReviewerKind::Model,
                "b",
                vec![finding("r2-1", None)],
                vec!["r1-1"],
            ),
        ];
        let open: Vec<String> = open_findings_of(&rows).into_iter().map(|f| f.id).collect();
        assert_eq!(open, vec!["r1-2", "r2-1"]);
    }

    #[test]
    fn a_bots_findings_are_whatever_its_latest_round_listed() {
        let rows = vec![
            row(
                1,
                ReviewerKind::Bot,
                "coderabbitai",
                vec![finding("t1", Some("T1")), finding("t2", Some("T2"))],
                vec![],
            ),
            row(
                2,
                ReviewerKind::Bot,
                "coderabbitai",
                vec![finding("t2", Some("T2"))],
                vec![],
            ),
            row(3, ReviewerKind::Model, "m", vec![], vec!["T2"]),
        ];
        assert!(
            open_findings_of(&rows).is_empty(),
            "t1 fell away and T2 was addressed"
        );
    }

    #[test]
    fn stages_and_kinds_round_trip_through_their_names() {
        for stage in [
            Stage::Idle,
            Stage::Running,
            Stage::NoChanges,
            Stage::AwaitingChecks,
            Stage::AwaitingReviews,
            Stage::Fixing,
            Stage::Merging,
            Stage::PostMerge,
            Stage::Merged,
            Stage::Paused,
        ] {
            assert_eq!(Stage::parse(stage.as_str()), Some(stage));
        }
        for kind in [
            Kind::Scaffold,
            Kind::Ci,
            Kind::Tests,
            Kind::Feature,
            Kind::Deployment,
            Kind::Docs,
            Kind::Fix,
        ] {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
        assert!(Stage::AwaitingReviews.in_flight());
        assert!(!Stage::Merged.in_flight());
    }

    #[test]
    fn every_verdict_round_trips_and_an_unknown_name_is_refused_rather_than_guessed() {
        for verdict in Verdict::ALL {
            assert_eq!(Verdict::parse(verdict.as_str()), Some(verdict));
            assert_eq!(
                Verdict::try_from(verdict.as_str().to_string()).ok(),
                Some(verdict)
            );
        }
        assert_eq!(Verdict::parse("failed"), Some(Verdict::Failed));
        assert_eq!(Verdict::parse("approved"), None);
        assert!(
            Verdict::try_from("approved".to_string()).is_err(),
            "a stored name no variant owns fails the read instead of counting as unparseable"
        );
    }

    #[test]
    fn a_failed_round_and_an_unreadable_one_are_both_verdictless() {
        assert_eq!(Verdict::verdictless(), ["unparseable", "failed"]);
        assert!(!Verdict::Approve.is_verdictless());
        assert!(!Verdict::RequestChanges.is_verdictless());
    }

    #[tokio::test]
    async fn finalize_stores_the_repository_installation() {
        let pool = crate::db::sources::testing::pool().await;
        let installation = crate::db::sources::testing::installation(&pool).await;
        let actor: Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash) VALUES ($1, 'hash') RETURNING id",
        )
        .bind(format!("planner-{}@example.test", Uuid::new_v4()))
        .fetch_one(&pool)
        .await
        .expect("a user");
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(installation.workspace)
        .bind(actor)
        .execute(&pool)
        .await
        .expect("a membership");

        let finalized = finalize(
            &pool,
            installation.workspace,
            actor,
            None,
            Origin::Member,
            &ProjectPlan {
                name: "Widgets",
                description: None,
                repository: Some(PlannedRepository {
                    url: "https://github.com/acme/widgets",
                    token: None,
                    github_installation_id: Some(installation.id),
                }),
                brief: &json!({}),
                tasks: &[],
            },
        )
        .await
        .expect("the project");

        let project = crate::db::projects::get_project(&pool, finalized.project_id)
            .await
            .expect("the read")
            .expect("the project");
        assert_eq!(project.github_installation_id, Some(installation.id));
        assert_eq!(project.github_access_token, None);
        assert_eq!(
            project.github_repo_url.as_deref(),
            Some("https://github.com/acme/widgets")
        );
    }

    const PULL: &str = "https://github.com/acme/shop/pull/7";

    #[tokio::test]
    async fn park_leaves_paused_and_cancelled_tasks_out_of_the_pass() {
        let project = Project::new(4).await;
        let [paused, cancelled, working, merged] = project.tasks[..] else {
            panic!("four tasks");
        };
        project
            .place(paused, tasks::PAUSED, None, Stage::Running)
            .await;
        project
            .place(
                cancelled,
                tasks::CANCELLED,
                Some(PULL),
                Stage::AwaitingChecks,
            )
            .await;
        project
            .place(working, tasks::IN_PROGRESS, None, Stage::Running)
            .await;
        project
            .place(merged, tasks::CANCELLED, Some(PULL), Stage::Merged)
            .await;

        park(&project.pool, project.id).await.expect("the park");
        let first = project.automation(paused).await.updated_at;
        park(&project.pool, project.id)
            .await
            .expect("a second park");
        let settled: Vec<Uuid> = settled_runs(&project.pool, project.id)
            .await
            .expect("the settled runs")
            .into_iter()
            .map(|run| run.task_id)
            .collect();
        let changes = pipeline(&project.pool, project.id)
            .await
            .expect("the pipeline");
        let paused_row = project.automation(paused).await;
        let cancelled_row = project.automation(cancelled).await;
        let working_row = project.automation(working).await;
        let merged_row = project.automation(merged).await;
        let saved = (
            project.parked(paused).await.0,
            project.parked(cancelled).await.0,
            project.parked(working).await.0,
            project.parked(merged).await.0,
        );
        project.remove().await;

        assert_eq!(
            paused_row.updated_at, first,
            "a parked row is left as it is"
        );
        assert_eq!(paused_row.stage(), Stage::Paused);
        assert_eq!(paused_row.reason.as_deref(), Some(PAUSED_BY_PERSON));
        assert_eq!(cancelled_row.stage(), Stage::Paused);
        assert_eq!(cancelled_row.reason.as_deref(), Some(CANCELLED_BY_PERSON));
        assert_eq!(working_row.stage(), Stage::Running);
        assert_eq!(merged_row.stage(), Stage::Merged, "a merge is a fact");
        assert_eq!(
            saved,
            (
                Some(Stage::Running.as_str().to_string()),
                Some(Stage::AwaitingChecks.as_str().to_string()),
                None,
                None
            ),
            "only a parked row carries the stage it was parked from"
        );
        assert_eq!(
            settled,
            vec![working],
            "only the task still in work is settled"
        );
        assert!(
            changes.is_empty(),
            "nothing parked is advanced: {changes:?}"
        );
    }

    #[tokio::test]
    async fn resuming_a_project_leaves_tasks_a_person_paused_or_cancelled_parked() {
        let project = Project::new(6).await;
        let [
            paused,
            paused_with_pull,
            cancelled,
            cancelled_with_pull,
            blocked,
            review,
        ] = project.tasks[..]
        else {
            panic!("six tasks");
        };
        project
            .place(paused, tasks::PAUSED, None, Stage::Paused)
            .await;
        project
            .place(paused_with_pull, tasks::PAUSED, Some(PULL), Stage::Paused)
            .await;
        project
            .place(cancelled, tasks::CANCELLED, None, Stage::Paused)
            .await;
        project
            .place(
                cancelled_with_pull,
                tasks::CANCELLED,
                Some(PULL),
                Stage::Paused,
            )
            .await;
        project
            .place(blocked, tasks::BLOCKED, None, Stage::Paused)
            .await;
        project
            .place(review, tasks::REVIEW, Some(PULL), Stage::Paused)
            .await;

        let resumed = resume_paused_tasks(&project.pool, project.id)
            .await
            .expect("the resume");
        let mut withheld = Vec::new();
        for task in [paused, paused_with_pull, cancelled, cancelled_with_pull] {
            withheld.push((
                project.automation(task).await.stage(),
                project.status(task).await,
            ));
        }
        let blocked_after = (
            project.automation(blocked).await.stage(),
            project.status(blocked).await,
        );
        let review_after = project.automation(review).await.stage();
        project.remove().await;

        assert_eq!(resumed, 2, "only the blocked and the review task resume");
        assert_eq!(
            withheld,
            vec![
                (Stage::Paused, tasks::PAUSED.to_string()),
                (Stage::Paused, tasks::PAUSED.to_string()),
                (Stage::Paused, tasks::CANCELLED.to_string()),
                (Stage::Paused, tasks::CANCELLED.to_string()),
            ]
        );
        assert_eq!(blocked_after, (Stage::Idle, tasks::CREATED.to_string()));
        assert_eq!(review_after, Stage::AwaitingChecks);
    }

    #[tokio::test]
    async fn a_cancelled_task_is_not_left_to_do_and_a_paused_one_is() {
        let project = Project::new(3).await;
        let [complete, cancelled, paused] = project.tasks[..] else {
            panic!("three tasks");
        };
        project
            .place(complete, tasks::COMPLETE, None, Stage::Merged)
            .await;
        project
            .place(cancelled, tasks::CANCELLED, None, Stage::Paused)
            .await;
        let with_paused = {
            project
                .place(paused, tasks::PAUSED, None, Stage::Paused)
                .await;
            remaining(&project.pool, project.id)
                .await
                .expect("the count")
        };
        project
            .place(paused, tasks::CANCELLED, None, Stage::Paused)
            .await;
        let all_cancelled = remaining(&project.pool, project.id)
            .await
            .expect("the count");
        project.remove().await;

        assert_eq!(with_paused, 1);
        assert_eq!(all_cancelled, 0);
    }

    #[tokio::test]
    async fn a_project_is_not_finished_while_an_agentic_task_has_work_left() {
        let project = Project::new(2).await;
        let [merged, open] = project.tasks[..] else {
            panic!("two tasks");
        };
        project
            .place(merged, tasks::COMPLETE, None, Stage::Merged)
            .await;

        let with_work_left = complete(&project.pool, project.id)
            .await
            .expect("the completion");
        project.set_status(open, tasks::CANCELLED).await;
        let with_none = complete(&project.pool, project.id)
            .await
            .expect("the completion");
        let again = complete(&project.pool, project.id)
            .await
            .expect("the completion");
        let after = automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        project.remove().await;

        assert_eq!(
            (with_work_left, with_none, again),
            (false, true, false),
            "only a project with nothing left to do is finished, and once"
        );
        assert!(after.completed_at.is_some());
    }

    #[tokio::test]
    async fn a_project_is_not_paused_as_stalled_while_a_task_is_in_flight_or_can_start() {
        const STALLED: &str = "nothing can start";
        let project = Project::new(2).await;
        let [blocker, dependent] = project.tasks[..] else {
            panic!("two tasks");
        };
        tasks::set_dependencies(&project.pool, dependent, &[blocker])
            .await
            .expect("the dependency");

        let run = tasks::create_task_run(&project.pool, blocker)
            .await
            .expect("a run a person started");
        let in_flight = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause");
        tasks::complete_task_run(&project.pool, run.id, tasks::FAILED, None, None)
            .await
            .expect("the run ends");
        let ended = project.status(blocker).await;
        project.set_status(blocker, tasks::CREATED).await;
        let runnable = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause");
        project.set_status(blocker, tasks::BLOCKED).await;
        let stalled = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause");
        let after = automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        project.remove().await;

        assert_eq!(ended, tasks::BLOCKED);
        assert_eq!(
            (in_flight, runnable, stalled),
            (false, false, true),
            "a project is stalled only when nothing is in flight and nothing can start"
        );
        assert_eq!(after.paused_reason.as_deref(), Some(STALLED));
    }

    #[tokio::test]
    async fn a_project_is_not_paused_as_stalled_over_tasks_a_person_paused_or_cancelled() {
        const STALLED: &str = "nothing can start";
        let project = Project::new(2).await;
        let [first, second] = project.tasks[..] else {
            panic!("two tasks");
        };
        project.set_status(first, tasks::CANCELLED).await;

        project.set_status(second, tasks::PAUSED).await;
        let held = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause");
        project.set_status(second, tasks::CANCELLED).await;
        let finished = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause");
        project.set_status(second, tasks::BLOCKED).await;
        let stalled = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause");
        project.remove().await;

        assert_eq!(
            (held, finished, stalled),
            (false, false, true),
            "a project waiting on a person, or with nothing left, is not one that stalled"
        );
    }

    #[tokio::test]
    async fn giving_a_task_back_lifts_a_pause_for_stalling_and_no_other() {
        const STALLED: &str = "nothing can start";
        const WITHOUT_ACCESS: &str = "no write access";
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project.set_status(task, tasks::BLOCKED).await;

        let stalled = pause_stalled(&project.pool, project.id, STALLED)
            .await
            .expect("the pause for stalling");
        reopen(&project.pool, task).await.expect("the reopen");
        let lifted = automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        let refused = pause(&project.pool, project.id, WITHOUT_ACCESS)
            .await
            .expect("the pause for access");
        sqlx::query("UPDATE projects SET auto_completed_at = NOW() WHERE id = $1")
            .bind(project.id)
            .execute(&project.pool)
            .await
            .expect("the project finishing");
        reopen(&project.pool, task).await.expect("the reopen");
        let kept = automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        project.remove().await;

        assert!(stalled && refused, "both pauses apply");
        assert_eq!(
            lifted.paused_reason, None,
            "a project that stalled starts again once a task is given back"
        );
        assert_eq!(
            kept.paused_reason.as_deref(),
            Some(WITHOUT_ACCESS),
            "a pause no task caused waits for a person to resume the project"
        );
        assert_eq!(
            kept.completed_at, None,
            "the project is still open again for the task"
        );
    }

    #[tokio::test]
    async fn giving_a_task_back_undoes_a_stop_the_driver_is_still_writing() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project.set_status(task, tasks::CANCELLED).await;
        let mut stop = project.hold().await;
        sqlx::query("UPDATE projects SET auto_completed_at = NOW() WHERE id = $1")
            .bind(project.id)
            .execute(&mut *stop.transaction)
            .await
            .expect("the driver's stop, written and not yet committed");
        project.set_status(task, tasks::CREATED).await;

        let (reopened, ()) = {
            let project = &project;
            tokio::join!(reopen(&project.pool, task), async move {
                project.queued_behind(&stop).await;
                stop.release().await;
            })
        };
        let after = automation(&project.pool, project.id)
            .await
            .expect("the project's automation")
            .expect("an automated project");
        project.remove().await;

        reopened.expect("the reopen");
        assert_eq!(
            after.completed_at, None,
            "the reopen waits for the stop and then undoes it"
        );
    }

    #[tokio::test]
    async fn no_unattended_run_is_admitted_for_a_paused_or_cancelled_task() {
        let project = Project::new(3).await;
        let [paused, cancelled, created] = project.tasks[..] else {
            panic!("three tasks");
        };
        project
            .place(paused, tasks::PAUSED, None, Stage::Idle)
            .await;
        project
            .place(cancelled, tasks::CANCELLED, None, Stage::Idle)
            .await;

        let mut refused = Vec::new();
        for task in [paused, cancelled] {
            refused.push(
                tasks::create_unattended_task_run(&project.pool, task, project.actor)
                    .await
                    .expect("the admission"),
            );
        }
        let admitted = tasks::create_unattended_task_run(&project.pool, created, project.actor)
            .await
            .expect("the admission");
        let runs = (
            project.runs(paused).await,
            project.runs(cancelled).await,
            project.runs(created).await,
        );
        let statuses = (
            project.status(paused).await,
            project.status(cancelled).await,
        );
        project.remove().await;

        assert!(
            refused
                .iter()
                .all(|outcome| matches!(outcome, tasks::Unattended::Withheld)),
            "{refused:?}"
        );
        assert!(matches!(admitted, tasks::Unattended::Created(_)));
        assert_eq!(runs, (0, 0, 1));
        assert_eq!(
            statuses,
            (tasks::PAUSED.to_string(), tasks::CANCELLED.to_string()),
            "a refusal leaves the status a person chose"
        );
    }

    const FIX: &str = "checks failed on 1a2b3c4: lint. Fix the cause on this branch.";

    #[tokio::test]
    async fn a_parked_row_gets_back_the_stage_and_reason_it_was_parked_with() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::REVIEW, Some(PULL), Stage::Idle)
            .await;
        set_stage(&project.pool, task, project.id, Stage::Fixing, Some(FIX))
            .await
            .expect("the fix the pipeline asked for");

        project.set_status(task, tasks::CANCELLED).await;
        park(&project.pool, project.id).await.expect("the park");
        let cancelled = project.automation(task).await;
        project.set_status(task, tasks::PAUSED).await;
        park(&project.pool, project.id)
            .await
            .expect("a park of a row parked already");
        let paused = project.automation(task).await;
        let saved = project.parked(task).await;
        project.set_status(task, tasks::REVIEW).await;
        unpark(&project.pool, project.id).await.expect("the unpark");
        let after = project.automation(task).await;
        let cleared = project.parked(task).await;
        project.remove().await;

        assert_eq!(cancelled.stage(), Stage::Paused);
        assert_eq!(cancelled.reason.as_deref(), Some(CANCELLED_BY_PERSON));
        assert_eq!(paused.stage(), Stage::Paused);
        assert_eq!(
            paused.reason.as_deref(),
            Some(PAUSED_BY_PERSON),
            "the row says what the task is now"
        );
        assert_eq!(
            saved,
            (
                Some(Stage::Fixing.as_str().to_string()),
                Some(FIX.to_string())
            ),
            "a second park keeps what the first one saved"
        );
        assert_eq!(after.stage(), Stage::Fixing);
        assert_eq!(
            after.reason.as_deref(),
            Some(FIX),
            "the next run is told what to fix"
        );
        assert_eq!(cleared, (None, None));
    }

    #[tokio::test]
    async fn an_un_paused_task_without_a_pull_request_is_idle_again_and_admitted() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project.place(task, tasks::PAUSED, None, Stage::Idle).await;
        park(&project.pool, project.id).await.expect("the park");
        let while_paused = next_runnable(&project.pool, project.id)
            .await
            .expect("the next runnable task");

        project.set_status(task, tasks::CREATED).await;
        unpark(&project.pool, project.id).await.expect("the unpark");
        let after = project.automation(task).await;
        let runnable = next_runnable(&project.pool, project.id)
            .await
            .expect("the next runnable task");
        let admitted = tasks::create_unattended_task_run(&project.pool, task, project.actor)
            .await
            .expect("the admission");
        project.remove().await;

        assert_eq!(while_paused, None);
        assert_eq!(after.stage(), Stage::Idle);
        assert_eq!(after.reason, None);
        assert_eq!(runnable, Some(task));
        assert!(
            matches!(admitted, tasks::Unattended::Created(_)),
            "{admitted:?}"
        );
    }

    #[tokio::test]
    async fn a_task_a_person_started_a_run_on_stays_parked_until_the_run_ends() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project.place(task, tasks::PAUSED, None, Stage::Idle).await;
        park(&project.pool, project.id).await.expect("the park");

        let run = tasks::create_task_run(&project.pool, task)
            .await
            .expect("a run a person started");
        unpark(&project.pool, project.id)
            .await
            .expect("an unpark during the run");
        let during = project.automation(task).await;
        let kept = project.parked(task).await.0;
        tasks::complete_task_run(&project.pool, run.id, "completed", None, None)
            .await
            .expect("the run ends");
        sqlx::query("UPDATE tasks SET pr_url = $2 WHERE id = $1")
            .bind(task)
            .bind(PULL)
            .execute(&project.pool)
            .await
            .expect("the run's pull request");
        unpark(&project.pool, project.id)
            .await
            .expect("an unpark after the run");
        let after = project.automation(task).await;
        project.remove().await;

        assert_eq!(during.stage(), Stage::Paused, "the run is the person's");
        assert_eq!(kept, Some(Stage::Idle.as_str().to_string()));
        assert_eq!(
            after.stage(),
            Stage::AwaitingChecks,
            "a pull request the pipeline was not driving is checked from the start"
        );
        assert_eq!(after.reason, None);
    }

    #[tokio::test]
    async fn a_row_automation_paused_for_its_own_reasons_is_not_released_by_a_person() {
        const SPENT: &str = "task \"Task 0\" used its 3 runs; the last run failed: boom";
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project.place(task, tasks::BLOCKED, None, Stage::Idle).await;
        set_stage(&project.pool, task, project.id, Stage::Paused, Some(SPENT))
            .await
            .expect("automation gives up on the task");

        project.set_status(task, tasks::PAUSED).await;
        park(&project.pool, project.id).await.expect("the park");
        let while_paused = project.automation(task).await;
        let saved = project.parked(task).await;
        project.set_status(task, tasks::BLOCKED).await;
        unpark(&project.pool, project.id).await.expect("the unpark");
        let after = project.automation(task).await;
        project.remove().await;

        assert_eq!(while_paused.reason.as_deref(), Some(SPENT));
        assert_eq!(saved, (None, None), "the row is automation's, not parked");
        assert_eq!(after.stage(), Stage::Paused);
        assert_eq!(
            after.reason.as_deref(),
            Some(SPENT),
            "it still needs the person it was waiting for"
        );
    }

    #[tokio::test]
    async fn a_task_a_person_completed_while_it_was_parked_has_nothing_left_to_drive() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::PAUSED, Some(PULL), Stage::AwaitingChecks)
            .await;
        park(&project.pool, project.id).await.expect("the park");

        project.set_status(task, tasks::COMPLETE).await;
        unpark(&project.pool, project.id).await.expect("the unpark");
        let after = project.automation(task).await;
        let changes = pipeline(&project.pool, project.id)
            .await
            .expect("the pipeline");
        project.remove().await;

        assert_eq!(after.stage(), Stage::Idle);
        assert_eq!(after.reason, None);
        assert!(changes.is_empty(), "{changes:?}");
    }

    #[tokio::test]
    async fn a_row_moved_on_by_automation_carries_nothing_a_later_pause_could_restore() {
        let project = Project::new(1).await;
        let task = project.tasks[0];
        project
            .place(task, tasks::PAUSED, Some(PULL), Stage::Fixing)
            .await;
        park(&project.pool, project.id).await.expect("the park");

        set_stage(
            &project.pool,
            task,
            project.id,
            Stage::Paused,
            Some("GitHub refused the merge"),
        )
        .await
        .expect("a step that was already under way pauses the task");
        let saved = project.parked(task).await;
        project.set_status(task, tasks::REVIEW).await;
        unpark(&project.pool, project.id).await.expect("the unpark");
        let after = project.automation(task).await;
        project.remove().await;

        assert_eq!(saved, (None, None));
        assert_eq!(
            after.stage(),
            Stage::Paused,
            "a pause automation made is not one a person's move lifts"
        );
    }

    #[tokio::test]
    async fn a_cancelled_dependency_is_met_and_a_paused_one_is_not() {
        let project = Project::new(2).await;
        let [blocker, dependent] = project.tasks[..] else {
            panic!("two tasks");
        };
        tasks::set_dependencies(&project.pool, dependent, &[blocker])
            .await
            .expect("the dependency");

        project.set_status(blocker, tasks::PAUSED).await;
        let behind_paused = next_runnable(&project.pool, project.id)
            .await
            .expect("the next runnable task");
        project.set_status(blocker, tasks::CANCELLED).await;
        let behind_cancelled = next_runnable(&project.pool, project.id)
            .await
            .expect("the next runnable task");
        project.remove().await;

        assert_eq!(behind_paused, None, "a paused task is still to be done");
        assert_eq!(
            behind_cancelled,
            Some(dependent),
            "a cancelled task is out of the plan, so nothing waits on it"
        );
    }

    #[tokio::test]
    async fn a_cancelled_continuous_integration_task_is_not_the_projects() {
        let project = Project::new(1).await;
        let added = insert_auto_task(
            &project.pool,
            project.id,
            project.workspace,
            project.actor,
            Kind::Ci,
            "Add continuous integration",
            "Nothing runs on a pull request.",
            None,
        )
        .await
        .expect("the continuous-integration task");

        let live = ci_task(&project.pool, project.id).await.expect("the read");
        project.set_status(added, tasks::CANCELLED).await;
        let cancelled = ci_task(&project.pool, project.id).await.expect("the read");
        project.remove().await;

        assert_eq!(live, Some((added, tasks::CREATED.to_string())));
        assert_eq!(cancelled, None);
    }

    #[tokio::test]
    async fn continuous_integration_goes_ahead_of_a_paused_task_as_of_one_yet_to_start() {
        let project = Project::new(3).await;
        let [waiting, paused, working] = project.tasks[..] else {
            panic!("three tasks");
        };
        project.set_status(paused, tasks::PAUSED).await;
        project.set_status(working, tasks::IN_PROGRESS).await;

        let added = insert_auto_task(
            &project.pool,
            project.id,
            project.workspace,
            project.actor,
            Kind::Ci,
            "Add continuous integration",
            "Nothing runs on a pull request.",
            None,
        )
        .await
        .expect("the continuous-integration task");
        let mut waits = Vec::new();
        for task in [waiting, paused, working] {
            let dependencies: serde_json::Value =
                sqlx::query_scalar("SELECT dependencies FROM tasks WHERE id = $1")
                    .bind(task)
                    .fetch_one(&project.pool)
                    .await
                    .expect("the task's dependencies");
            waits.push(dependencies == json!([added.to_string()]));
        }
        project.remove().await;

        assert_eq!(
            waits,
            vec![true, true, false],
            "a paused task starts once it is un-paused, so it waits too"
        );
    }
}
