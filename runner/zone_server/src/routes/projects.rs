//! Project endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::projects::{self, NewProject, ProjectRow};
use crate::db::{auto_projects, chats, sources, workspace_members};
use crate::error::ServerError;
use crate::state::AppState;

use super::common::Timestamps;

/// The statuses `projects.status` accepts.
pub const PROJECT_STATUSES: [&str; 3] = ["active", "on_hold", "cancelled"];

fn not_found() -> ServerError {
    ServerError::NotFound("Project not found".to_string())
}

fn user_id(auth: &AuthUser) -> Result<Uuid, ServerError> {
    Uuid::parse_str(&auth.0.sub)
        .map_err(|_| ServerError::Unauthorized("Invalid user ID in token".to_string()))
}

/// A project and the workspace it belongs to.
///
/// A project without a workspace is treated as absent: nothing can be
/// authorised against it.
async fn load(state: &AppState, id: Uuid) -> Result<(ProjectRow, Uuid), ServerError> {
    let project = projects::get_project(state.db(), id)
        .await?
        .ok_or_else(not_found)?;
    let workspace_id = project.workspace_id.ok_or_else(not_found)?;
    Ok((project, workspace_id))
}

/// The project `id` names, for a caller who may read its workspace.
async fn readable(state: &AppState, auth: &AuthUser, id: Uuid) -> Result<ProjectRow, ServerError> {
    let user_id = user_id(auth)?;
    let (project, workspace_id) = load(state, id).await?;
    if workspace_members::is_member(state.db(), user_id, workspace_id).await? {
        Ok(project)
    } else {
        Err(not_found())
    }
}

/// The project `id` names and its workspace, for a caller who may change it.
async fn writable(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
) -> Result<(ProjectRow, Uuid), ServerError> {
    let user_id = user_id(auth)?;
    let (project, workspace_id) = load(state, id).await?;
    refuse_unless_writable(state, workspace_id, user_id).await?;
    Ok((project, workspace_id))
}

/// Refuse a write to `workspace_id` unless the caller may make it.
///
/// A caller who is not a member is told the project does not exist, so the
/// id-addressed routes cannot be used to enumerate ids across tenants -- the
/// rule `tasks.rs` already applies to tasks and runs. A member who only reads
/// is told they need write access, because they can already see the project.
async fn refuse_unless_writable(
    state: &AppState,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<(), ServerError> {
    if !workspace_members::is_member(state.db(), user_id, workspace_id).await? {
        return Err(not_found());
    }
    if !workspace_members::can_write(state.db(), workspace_id, user_id).await? {
        return Err(ServerError::Forbidden(
            "Workspace write access required".to_string(),
        ));
    }
    Ok(())
}

/// A source id the caller gave, once it is confirmed to live in `workspace_id`.
async fn source_in_workspace(
    state: &AppState,
    source_id: Uuid,
    workspace_id: Uuid,
) -> Result<Uuid, ServerError> {
    sources::get_source(state.db(), source_id, workspace_id)
        .await?
        .map(|_| source_id)
        .ok_or_else(|| ServerError::BadRequest("Source not found in this workspace".to_string()))
}

fn known_status(status: &str) -> Result<&str, ServerError> {
    if PROJECT_STATUSES.contains(&status) {
        Ok(status)
    } else {
        Err(ServerError::BadRequest(format!(
            "Invalid status \"{}\". Must be one of: {}",
            status,
            PROJECT_STATUSES.join(", ")
        )))
    }
}

/// The conflict a request to start, enable or resume automation is told when
/// this server runs no driver: enabling a project nothing will pick up would
/// only look like progress.
fn refuse_unless_driven(state: &AppState) -> Result<(), ServerError> {
    if state.config().auto.enabled {
        return Ok(());
    }
    Err(ServerError::Conflict(
        "Automation is disabled on this server (ZONE_AUTO_ENABLED=false); enable the \
         driver before starting, enabling or resuming an auto project"
            .to_string(),
    ))
}

/// One project's response, with what automation knows about it.
async fn describe(state: &AppState, row: ProjectRow) -> ProjectResponse {
    let automation = auto_projects::automation(state.db(), row.id)
        .await
        .ok()
        .flatten();
    ProjectResponse {
        project: ProjectData::from(row).with_automation(automation.as_ref()),
    }
}

async fn respond(state: &AppState, project: Option<ProjectRow>) -> Result<Response, ServerError> {
    let project = project.ok_or_else(not_found)?;
    Ok(Json(describe(state, project).await).into_response())
}

/// Project data
#[derive(Debug, Serialize)]
pub struct ProjectData {
    id: Uuid,
    workspace_id: Option<Uuid>,
    source_id: Option<Uuid>,
    name: String,
    description: Option<String>,
    status: String,
    github_repo_url: Option<String>,
    /// Whether the project runs itself, and why it stopped or when it finished.
    auto: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_paused_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_completed_at: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl ProjectData {
    /// Carry the project's automation state into the response.
    fn with_automation(mut self, automation: Option<&auto_projects::ProjectAutomation>) -> Self {
        if let Some(automation) = automation {
            self.auto = automation.auto;
            self.auto_paused_reason = automation.paused_reason.clone();
            self.auto_completed_at = automation.completed_at.map(|at| at.to_rfc3339());
        }
        self
    }
}

/// Single project response
#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    project: ProjectData,
}

/// Projects list response
#[derive(Debug, Serialize)]
pub struct ProjectsListResponse {
    projects: Vec<ProjectData>,
}

impl From<ProjectRow> for ProjectData {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            source_id: row.source_id,
            name: row.name,
            description: row.description,
            status: row.status,
            github_repo_url: row.github_repo_url,
            auto: false,
            auto_paused_reason: None,
            auto_completed_at: None,
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

/// Query parameters for listing projects
#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    workspace_id: Uuid,
    status: Option<String>,
}

/// Create project request
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    name: String,
    description: Option<String>,
    workspace_id: Uuid,
    source_id: Option<Uuid>,
    status: Option<String>,
}

/// Update project request; every field is optional and only the given ones change
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
    /// Turn automation on or off: on, every agentic task of the project is
    /// run, reviewed and merged without anyone in the loop.
    auto: Option<bool>,
}

/// Link source request
#[derive(Debug, Deserialize)]
pub struct LinkSourceRequest {
    source_id: Uuid,
}

/// Link GitHub request
#[derive(Debug, Deserialize)]
pub struct LinkGitHubRequest {
    repo_url: String,
    access_token: Option<String>,
}

/// Start an auto project: the brief the interview opens with.
#[derive(Debug, Deserialize)]
pub struct AutoProjectRequest {
    brief: String,
    model_name: Option<String>,
}

/// Where the interview happens.
#[derive(Debug, Serialize)]
pub struct AutoProjectResponse {
    chat_id: Uuid,
}

/// Characters a brief may carry.
const BRIEF_CHARS: usize = 8_000;
/// Characters of the brief that name the planner chat.
const BRIEF_TITLE_CHARS: usize = 72;

/// POST /api/workspaces/{workspace_id}/projects/auto
///
/// Opens a planner chat with the brief as its first message and runs the
/// first turn, which asks the first card. The person carries on in the chat;
/// the project appears when the interview ends.
pub async fn start_auto(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(req): Json<AutoProjectRequest>,
) -> Result<Response, ServerError> {
    let user_id = user_id(&auth)?;
    refuse_unless_writable(&state, workspace_id, user_id).await?;
    refuse_unless_driven(&state)?;
    let brief = req.brief.trim();
    if brief.is_empty() {
        return Err(ServerError::BadRequest("A brief is required".to_string()));
    }
    if brief.chars().count() > BRIEF_CHARS {
        return Err(ServerError::BadRequest("The brief is too long".to_string()));
    }
    let model = req
        .model_name
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(crate::services::stages::AUTO)
        .to_string();
    if crate::services::model::Model::completion(&state.config().ollama_host, &model).await
        == Some(false)
    {
        return Err(ServerError::BadRequest(
            crate::services::model::UNSUPPORTED.to_string(),
        ));
    }
    let first_line: String = brief
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(BRIEF_TITLE_CHARS)
        .collect();
    let chat = chats::create_purposed_chat(
        state.db(),
        workspace_id,
        &format!("Plan: {}", first_line.trim()),
        &model,
        chats::ChatPurpose::ProjectPlanner,
        None,
        true,
    )
    .await?;
    let chat_id = chat.id;
    let brief = brief.to_string();
    let turn_state = state.clone();
    tokio::spawn(async move {
        crate::ws::chat::run_turn(
            &turn_state,
            chat_id,
            workspace_id,
            user_id,
            &brief,
            Some(serde_json::json!({"source": "auto_project", "actor_id": user_id})),
        )
        .await;
    });
    Ok((StatusCode::ACCEPTED, Json(AutoProjectResponse { chat_id })).into_response())
}

/// GET /api/projects?workspace_id=xxx
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListProjectsQuery>,
) -> Result<Response, ServerError> {
    let user_id = user_id(&auth)?;
    if !workspace_members::is_member(state.db(), user_id, query.workspace_id).await? {
        return Err(ServerError::NotFound("Workspace not found".to_string()));
    }

    let rows =
        projects::list_projects(state.db(), query.workspace_id, query.status.as_deref()).await?;
    let automation = auto_projects::automation_for_workspace(state.db(), query.workspace_id)
        .await
        .unwrap_or_default();
    Ok(Json(ProjectsListResponse {
        projects: rows
            .into_iter()
            .map(|row| {
                let known = automation.iter().find(|entry| entry.project_id == row.id);
                ProjectData::from(row).with_automation(known)
            })
            .collect(),
    })
    .into_response())
}

/// POST /api/projects
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Response, ServerError> {
    let user_id = user_id(&auth)?;
    if !workspace_members::can_write(state.db(), req.workspace_id, user_id).await? {
        return Err(ServerError::Forbidden(
            "Workspace write access required".to_string(),
        ));
    }

    let status = req.status.as_deref().map(known_status).transpose()?;
    let source_id = match req.source_id {
        Some(source_id) => Some(source_in_workspace(&state, source_id, req.workspace_id).await?),
        None => None,
    };

    let project = projects::create(
        state.db(),
        NewProject {
            name: &req.name,
            description: req.description.as_deref(),
            workspace_id: Some(req.workspace_id),
            source_id,
            status,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(describe(&state, project).await)).into_response())
}

/// GET /api/projects/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    let project = readable(&state, &auth, id).await?;
    Ok(Json(describe(&state, project).await).into_response())
}

/// PUT|PATCH /api/projects/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Response, ServerError> {
    let (_, workspace_id) = writable(&state, &auth, id).await?;
    let user_id = user_id(&auth)?;
    let status = req.status.as_deref().map(known_status).transpose()?;

    if let Some(auto) = req.auto {
        if auto {
            refuse_unless_driven(&state)?;
        }
        let automation = auto_projects::set_auto(state.db(), id, auto, user_id)
            .await?
            .ok_or_else(not_found)?;
        if auto {
            if let Err(error) = chats::ensure_project_chat(
                state.db(),
                workspace_id,
                id,
                &automation.name,
                chats::ChatPurpose::ProjectUpdates,
            )
            .await
            {
                tracing::warn!(project_id = %id, %error, "Could not open the project's updates chat");
            }
            crate::workers::auto_project::poke(id);
        }
    }

    respond(
        &state,
        projects::update_project(
            state.db(),
            id,
            req.name.as_deref(),
            req.description.as_deref(),
            status,
        )
        .await?,
    )
    .await
}

/// DELETE /api/projects/:id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    if projects::delete_project(state.db(), id).await? {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(not_found())
    }
}

/// PUT /api/projects/:id/source
pub async fn link_source(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<LinkSourceRequest>,
) -> Result<Response, ServerError> {
    let (_, workspace_id) = writable(&state, &auth, id).await?;
    let source_id = source_in_workspace(&state, req.source_id, workspace_id).await?;

    respond(
        &state,
        projects::set_source(state.db(), id, Some(source_id)).await?,
    )
    .await
}

/// DELETE /api/projects/:id/source
pub async fn unlink_source(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    respond(&state, projects::set_source(state.db(), id, None).await?).await
}

/// POST /api/projects/:id/github
pub async fn link_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<LinkGitHubRequest>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    // The token is stored the way source credentials are: encrypted at rest,
    // opened by the checkout and pull request code that uses it.
    let sealed = req
        .access_token
        .as_deref()
        .map(|token| crate::crypto::encrypt(state.encryption_key(), token))
        .transpose()
        .map_err(|error| {
            tracing::error!(%error, "Could not encrypt the repository token");
            ServerError::Internal("Internal server error".to_string())
        })?;
    respond(
        &state,
        projects::link_github(state.db(), id, &req.repo_url, sealed.as_deref()).await?,
    )
    .await
}

/// DELETE /api/projects/:id/github
pub async fn unlink_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    respond(&state, projects::unlink_github(state.db(), id).await?).await
}

/// One task of a project as automation sees it.
#[derive(Debug, Serialize)]
pub struct AutomationTask {
    task_id: Uuid,
    title: String,
    status: String,
    is_agentic: bool,
    kind: Option<String>,
    stage: Option<String>,
    reason: Option<String>,
    runs: i32,
    review_rounds: i32,
    reviewers: Option<String>,
    pr_url: Option<String>,
    head: Option<String>,
    checks: Option<String>,
    merge_sha: Option<String>,
    auto_created: bool,
}

#[derive(Debug, Serialize)]
pub struct AutomationCounts {
    total: usize,
    agentic: usize,
    complete: usize,
    in_flight: usize,
    paused: usize,
}

#[derive(Debug, Serialize)]
pub struct AutomationResponse {
    project_id: Uuid,
    auto: bool,
    actor_id: Option<Uuid>,
    paused_reason: Option<String>,
    completed_at: Option<String>,
    parallelism: u64,
    planner_chat_id: Option<Uuid>,
    updates_chat_id: Option<Uuid>,
    counts: AutomationCounts,
    tasks: Vec<AutomationTask>,
}

/// GET /api/projects/:id/automation
pub async fn automation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    readable(&state, &auth, id).await?;
    let (automation, tasks, planner, updates) = tokio::join!(
        auto_projects::automation(state.db(), id),
        auto_projects::project_tasks(state.db(), id),
        chats::project_chat(state.db(), id, chats::ChatPurpose::ProjectPlanner),
        chats::project_chat(state.db(), id, chats::ChatPurpose::ProjectUpdates),
    );
    let automation = automation?.ok_or_else(not_found)?;
    let listed: Vec<AutomationTask> = tasks?
        .into_iter()
        .map(|task| AutomationTask {
            task_id: task.task_id,
            title: task.title,
            status: task.status,
            is_agentic: task.is_agentic,
            kind: task.kind,
            stage: task.stage,
            reason: task.reason,
            runs: task.runs.unwrap_or(0),
            review_rounds: task.review_rounds.unwrap_or(0),
            reviewers: task.reviewers,
            pr_url: task.pr_url,
            head: task.head,
            checks: task.checks,
            merge_sha: task.merge_sha,
            auto_created: task.auto_created.unwrap_or(false),
        })
        .collect();
    let counts = AutomationCounts {
        total: listed.len(),
        agentic: listed.iter().filter(|task| task.is_agentic).count(),
        complete: listed
            .iter()
            .filter(|task| task.status == "complete")
            .count(),
        in_flight: listed
            .iter()
            .filter(|task| {
                task.stage
                    .as_deref()
                    .and_then(auto_projects::Stage::parse)
                    .is_some_and(auto_projects::Stage::in_flight)
            })
            .count(),
        paused: listed
            .iter()
            .filter(|task| task.stage.as_deref() == Some("paused"))
            .count(),
    };
    Ok(Json(AutomationResponse {
        project_id: id,
        auto: automation.auto,
        actor_id: automation.actor_id,
        paused_reason: automation.paused_reason,
        completed_at: automation.completed_at.map(|at| at.to_rfc3339()),
        parallelism: state.config().auto.parallel_tasks,
        planner_chat_id: planner.ok().flatten(),
        updates_chat_id: updates.ok().flatten(),
        counts,
        tasks: listed,
    })
    .into_response())
}

/// POST /api/projects/:id/automation/resume
///
/// Clears a pause -- the project's, and every task's -- and pokes the driver.
pub async fn resume_automation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;
    refuse_unless_driven(&state)?;
    auto_projects::resume(state.db(), id).await?;
    auto_projects::resume_paused_tasks(state.db(), id).await?;
    crate::workers::auto_project::poke(id);
    respond(&state, projects::get_project(state.db(), id).await?).await
}
