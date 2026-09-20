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
use uuid::Uuid;

use super::DbResult;
use super::actions::invalid;
use super::tasks::{self, Create, MutationError};

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
pub async fn claim_due(pool: &PgPool, lease: Duration, limit: i64) -> DbResult<Vec<Uuid>> {
    let seconds = lease.as_secs_f64();
    let mut transaction = pool.begin().await?;
    let due: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE auto AND status = 'active' \
           AND auto_paused_reason IS NULL AND auto_completed_at IS NULL \
           AND (auto_claimed_at IS NULL OR auto_claimed_at < NOW() - make_interval(secs => $1)) \
         ORDER BY updated_at, id LIMIT $2 FOR UPDATE SKIP LOCKED",
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

/// Give up the driver's lease on a project once its pass is over.
pub async fn release(pool: &PgPool, project_id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE projects SET auto_claimed_at = NULL WHERE id = $1")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Stop driving a project and say why. Nothing already running is touched.
pub async fn pause(pool: &PgPool, project_id: Uuid, reason: &str) -> DbResult<bool> {
    Ok(sqlx::query(
        "UPDATE projects SET auto_paused_reason = $2, auto_claimed_at = NULL, updated_at = NOW() \
         WHERE id = $1 AND auto AND auto_paused_reason IS NULL",
    )
    .bind(project_id)
    .bind(reason)
    .execute(pool)
    .await?
    .rows_affected()
        == 1)
}

/// Clear a project's pause so the driver picks it up again; false when it does not exist.
pub async fn resume(pool: &PgPool, project_id: Uuid) -> DbResult<bool> {
    Ok(sqlx::query(
        "UPDATE projects SET auto_paused_reason = NULL, updated_at = NOW() \
         WHERE id = $1 AND auto_paused_reason IS NOT NULL",
    )
    .bind(project_id)
    .execute(pool)
    .await?
    .rows_affected()
        == 1)
}

/// Put every paused task of a project back where it was going: a task with a
/// pull request is checked again, one without is started again.
pub async fn resume_paused_tasks(pool: &PgPool, project_id: Uuid) -> DbResult<u64> {
    let checked = sqlx::query(
        "UPDATE task_automation a SET stage = 'awaiting_checks', reason = NULL, updated_at = NOW() \
         FROM tasks t WHERE t.id = a.task_id AND a.project_id = $1 AND a.stage = 'paused' \
           AND t.pr_url IS NOT NULL AND t.status <> 'complete'",
    )
    .bind(project_id)
    .execute(pool)
    .await?
    .rows_affected();
    // Admission only reads tasks in `created`; a task that exhausted its runs
    // sits in `blocked` (or `review` without a pull request), so it is put
    // back to `created` with its counter, or resume would change nothing.
    let restarted = sqlx::query(
        "WITH restarted AS ( \
           UPDATE task_automation a SET stage = 'idle', reason = NULL, runs = 0, updated_at = NOW() \
           FROM tasks t WHERE t.id = a.task_id AND a.project_id = $1 AND a.stage = 'paused' \
             AND t.pr_url IS NULL AND t.status <> 'complete' \
           RETURNING a.task_id) \
         UPDATE tasks SET status = 'created', updated_at = NOW() \
         WHERE id IN (SELECT task_id FROM restarted) AND active_run_id IS NULL",
    )
    .bind(project_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(checked + restarted)
}

/// Record that every agentic task merged; false when the project does not exist.
pub async fn complete(pool: &PgPool, project_id: Uuid) -> DbResult<bool> {
    Ok(sqlx::query(
        "UPDATE projects SET auto_completed_at = NOW(), auto_claimed_at = NULL, updated_at = NOW() \
         WHERE id = $1 AND auto_completed_at IS NULL",
    )
    .bind(project_id)
    .execute(pool)
    .await?
    .rows_affected()
        == 1)
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

/// Move a task to a stage, creating its row on the way if it has none.
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
         updated_at = NOW()",
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
/// depends on complete. Priority first, then age.
pub async fn next_runnable(pool: &PgPool, project_id: Uuid) -> DbResult<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT t.id FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
         WHERE tp.project_id = $1 AND t.status = 'created' AND t.is_agentic \
           AND t.active_run_id IS NULL \
           AND NOT EXISTS ( \
             SELECT 1 FROM jsonb_array_elements_text( \
               CASE WHEN jsonb_typeof(t.dependencies) = 'array' THEN t.dependencies ELSE '[]'::jsonb END \
             ) AS d(id) JOIN tasks p ON p.id::text = d.id WHERE p.status <> 'complete') \
         ORDER BY t.priority NULLS LAST, t.created_at, t.id LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

/// Tasks occupying one of the project's slots: running, or between a run and a merge.
pub async fn in_flight(pool: &PgPool, project_id: Uuid) -> DbResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
         LEFT JOIN task_automation a ON a.task_id = t.id \
         WHERE tp.project_id = $1 AND t.status <> 'complete' \
           AND (t.active_run_id IS NOT NULL \
                OR a.stage IN ('running', 'awaiting_checks', 'awaiting_reviews', 'fixing', 'merging'))",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
}

/// Agentic tasks of the project not yet complete.
pub async fn remaining(pool: &PgPool, project_id: Uuid) -> DbResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
         WHERE tp.project_id = $1 AND t.is_agentic AND t.status <> 'complete'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
}

/// Runs the driver started that are still going, across every project.
pub async fn active_unattended_runs(pool: &PgPool) -> DbResult<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_runs WHERE unattended AND status IN ('running', 'waiting')",
    )
    .fetch_one(pool)
    .await
}

/// The project's continuous-integration task, and its status.
pub async fn ci_task(pool: &PgPool, project_id: Uuid) -> DbResult<Option<(Uuid, String)>> {
    sqlx::query_as::<_, (Uuid, String)>(
        "SELECT t.id, t.status FROM task_automation a JOIN tasks t ON t.id = a.task_id \
         WHERE a.project_id = $1 AND a.kind = 'ci' ORDER BY t.created_at LIMIT 1",
    )
    .bind(project_id)
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
                (SELECT string_agg(DISTINCT r.reviewer, ', ') FROM task_reviews r WHERE r.task_id = t.id) \
                  AS reviewers \
         FROM tasks t JOIN task_projects tp ON tp.task_id = t.id \
         LEFT JOIN task_automation a ON a.task_id = t.id \
         WHERE tp.project_id = $1 \
         ORDER BY t.priority NULLS LAST, t.created_at, t.id",
    )
    .bind(project_id)
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
    Unparseable,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::RequestChanges => "request_changes",
            Self::Unparseable => "unparseable",
        }
    }

    /// The variant a stored name denotes, if any.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "approve" => Self::Approve,
            "request_changes" => Self::RequestChanges,
            "unparseable" => Self::Unparseable,
            _ => return None,
        })
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
    pub verdict: String,
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

    /// The round's verdict; unparseable when the stored name is unknown.
    pub fn verdict(&self) -> Verdict {
        Verdict::parse(&self.verdict).unwrap_or(Verdict::Unparseable)
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
/// recorded on this head: a Zone review on a different model, or any bot.
pub async fn distinct_review_on_head(pool: &PgPool, task_id: Uuid, head: &str) -> DbResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM task_reviews WHERE task_id = $1 AND head = $2 \
           AND (reviewer_kind = 'bot' OR (NOT same_model AND author_model IS NOT NULL)) \
           AND verdict <> 'unparseable')",
    )
    .bind(task_id)
    .bind(head)
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

/// Translate a task mutation refusal into the database error the transaction reports.
fn project_error(error: MutationError) -> sqlx::Error {
    match error {
        MutationError::Database(error) => error,
        MutationError::Project => invalid("Project is not available in this workspace"),
        MutationError::Source => invalid("Source is not available in this workspace"),
        MutationError::ActiveRun => invalid("Task has an active run"),
    }
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
    let project_id: Uuid = sqlx::query_scalar(
        "INSERT INTO projects (name, description, workspace_id, status, auto, auto_actor_id, brief, \
           github_repo_url, github_access_token) \
         VALUES ($1, $2, $3, 'active', TRUE, $4, $5, $6, $7) RETURNING id",
    )
    .bind(plan.name)
    .bind(plan.description)
    .bind(workspace_id)
    .bind(actor)
    .bind(plan.brief)
    .bind(plan.repository.as_ref().map(|repository| repository.url))
    .bind(plan.repository.as_ref().and_then(|repository| repository.token))
    .fetch_one(&mut *transaction)
    .await?;

    let mut task_ids: Vec<Uuid> = Vec::with_capacity(plan.tasks.len());
    for task in plan.tasks {
        let created = tasks::create_task_in(
            &mut transaction,
            &Create {
                workspace_id,
                project_ids: &[project_id],
                title: task.title.trim(),
                description: task.description.trim(),
                acceptance_criteria: task.acceptance_criteria.as_deref(),
                priority: task.priority,
                is_agentic: true,
                require_plan_approval: false,
                source_id: None,
                created_by: Some(actor),
            },
        )
        .await
        .map_err(project_error)?;
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
            title,
            description,
            acceptance_criteria,
            priority: Some(1),
            is_agentic: true,
            require_plan_approval: false,
            source_id: None,
            created_by: Some(actor),
        },
    )
    .await
    .map_err(project_error)?;
    set_kind(&mut transaction, created.id, project_id, kind, true).await?;
    if kind == Kind::Ci {
        sqlx::query(
            "UPDATE tasks t SET dependencies = \
               (CASE WHEN jsonb_typeof(t.dependencies) = 'array' THEN t.dependencies ELSE '[]'::jsonb END) \
               || jsonb_build_array($2::text), updated_at = NOW() \
             FROM task_projects tp \
             WHERE tp.task_id = t.id AND tp.project_id = $1 AND t.id <> $3 \
               AND t.is_agentic AND t.status = 'created'",
        )
        .bind(project_id)
        .bind(created.id.to_string())
        .bind(created.id)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(created.id)
}

#[cfg(test)]
mod tests {
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
            verdict: "request_changes".into(),
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
}
