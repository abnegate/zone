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
use crate::db::{auto_projects, chats, projects, workspace_members};
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

/// Refuse a write to `workspace_id`, or `None` when the caller may make it.
///
/// A caller who is not a member is told the project does not exist, so the
/// id-addressed routes cannot be used to enumerate ids across tenants -- the
/// rule `tasks.rs` already applies to tasks and runs. A member who only reads
/// is told they need write access, because they can already see the project.
async fn refuse_unless_writable(
    state: &AppState,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Option<Response> {
    match workspace_members::is_member(state.db(), user_id, workspace_id).await {
        Ok(true) => {}
        Ok(false) => {
            return Some(
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new("Project not found")),
                )
                    .into_response(),
            );
        }
        Err(e) => {
            tracing::error!("Database error checking membership: {}", e);
            return Some(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response(),
            );
        }
    }

    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => None,
        Ok(false) => Some(
            (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response(),
        ),
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            Some(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response(),
            )
        }
    }
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

/// One project's response, with what automation knows about it.
async fn respond(state: &AppState, row: projects::ProjectRow) -> ProjectResponse {
    let automation = auto_projects::automation(state.db(), row.id)
        .await
        .ok()
        .flatten();
    ProjectResponse {
        project: ProjectData::from(row).with_automation(automation.as_ref()),
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

impl From<projects::ProjectRow> for ProjectData {
    fn from(row: projects::ProjectRow) -> Self {
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

impl From<projects::ProjectRow> for ProjectResponse {
    fn from(row: projects::ProjectRow) -> Self {
        Self {
            project: ProjectData::from(row),
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
}

/// Update project request
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
    /// Turn automation on or off: on, every agentic task of the project is
    /// run, reviewed and merged without anyone in the loop.
    auto: Option<bool>,
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
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };
    if let Some(refusal) = refuse_unless_writable(&state, workspace_id, user_id).await {
        return refusal;
    }
    if let Some(refusal) = refuse_unless_driven(&state) {
        return refusal;
    }
    let brief = req.brief.trim();
    if brief.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("A brief is required")),
        )
            .into_response();
    }
    if brief.chars().count() > BRIEF_CHARS {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("The brief is too long")),
        )
            .into_response();
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
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(crate::services::model::UNSUPPORTED)),
        )
            .into_response();
    }
    let first_line: String = brief
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(BRIEF_TITLE_CHARS)
        .collect();
    let chat = match chats::create_purposed_chat(
        state.db(),
        workspace_id,
        &format!("Plan: {}", first_line.trim()),
        &model,
        chats::ChatPurpose::ProjectPlanner,
        None,
        true,
    )
    .await
    {
        Ok(chat) => chat,
        Err(error) => {
            tracing::error!("Database error: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };
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
    (StatusCode::ACCEPTED, Json(AutoProjectResponse { chat_id })).into_response()
}

/// GET /api/projects?workspace_id=xxx
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListProjectsQuery>,
) -> impl IntoResponse {
    // Verify workspace membership
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Check if user is a member of the workspace
    match workspace_members::is_member(state.db(), user_id, query.workspace_id).await {
        Ok(true) => {
            // User is a member, proceed
        }
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Workspace not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking membership: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    match projects::list_projects(state.db(), query.workspace_id, query.status.as_deref()).await {
        Ok(projs) => {
            let automation =
                auto_projects::automation_for_workspace(state.db(), query.workspace_id)
                    .await
                    .unwrap_or_default();
            Json(ProjectsListResponse {
                projects: projs
                    .into_iter()
                    .map(|row| {
                        let known = automation.iter().find(|entry| entry.project_id == row.id);
                        ProjectData::from(row).with_automation(known)
                    })
                    .collect(),
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/projects
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    // Verify workspace membership (must be writer or higher)
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Check if user can write to the workspace
    match workspace_members::can_write(state.db(), req.workspace_id, user_id).await {
        Ok(true) => {
            // User can write, proceed
        }
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Workspace write access required")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking permissions: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    match projects::create_project(
        state.db(),
        &req.name,
        req.description.as_deref(),
        Some(req.workspace_id),
    )
    .await
    {
        Ok(proj) => (StatusCode::CREATED, Json(respond(&state, proj).await)).into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/projects/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user has access to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
    };

    match workspace_members::is_member(state.db(), user_id, workspace_id).await {
        Ok(true) => {
            // User is a member, proceed
        }
        Ok(false) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking membership: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    Json(respond(&state, proj).await).into_response()
}

/// PUT /api/projects/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
    };

    if let Some(refusal) = refuse_unless_writable(&state, workspace_id, user_id).await {
        return refusal;
    }

    if let Some(auto) = req.auto {
        if auto && let Some(refusal) = refuse_unless_driven(&state) {
            return refusal;
        }
        match auto_projects::set_auto(state.db(), id, auto, user_id).await {
            Ok(Some(automation)) => {
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
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new("Project not found")),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("Database error: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    match projects::update_project(
        state.db(),
        id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.status.as_deref(),
    )
    .await
    {
        Ok(Some(proj)) => Json(respond(&state, proj).await).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// DELETE /api/projects/:id
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace (delete requires write access)
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
    };

    if let Some(refusal) = refuse_unless_writable(&state, workspace_id, user_id).await {
        return refusal;
    }

    match projects::delete_project(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/projects/:id/github
pub async fn link_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<LinkGitHubRequest>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
    };

    if let Some(refusal) = refuse_unless_writable(&state, workspace_id, user_id).await {
        return refusal;
    }

    // The token is stored the way source credentials are: encrypted at rest,
    // opened by the checkout and pull request code that uses it.
    let sealed = match req.access_token.as_deref() {
        Some(token) => match crate::crypto::encrypt(state.encryption_key(), token) {
            Ok(sealed) => Some(sealed),
            Err(error) => {
                tracing::error!(%error, "Could not encrypt the repository token");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        },
        None => None,
    };
    match projects::link_github(state.db(), id, &req.repo_url, sealed.as_deref()).await {
        Ok(Some(proj)) => Json(respond(&state, proj).await).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// DELETE /api/projects/:id/github
pub async fn unlink_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };

    // Get the project first to check its workspace
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // NEW-CRITICAL-1: Verify user can write to the project's workspace
    // If workspace_id is None, deny access (project should always have a workspace)
    let workspace_id = match proj.workspace_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response();
        }
    };

    if let Some(refusal) = refuse_unless_writable(&state, workspace_id, user_id).await {
        return refusal;
    }

    match projects::unlink_github(state.db(), id).await {
        Ok(Some(proj)) => Json(respond(&state, proj).await).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
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

/// The conflict a request to start, enable or resume automation is told when
/// this server runs no driver: enabling a project nothing will pick up would
/// only look like progress.
fn refuse_unless_driven(state: &AppState) -> Option<Response> {
    if state.config().auto.enabled {
        return None;
    }
    Some(
        (
            StatusCode::CONFLICT,
            Json(ErrorResponse::new(
                "Automation is disabled on this server (ZONE_AUTO_ENABLED=false); enable the \
                 driver before starting, enabling or resuming an auto project",
            )),
        )
            .into_response(),
    )
}

/// The project's workspace, or the not-found a non-member is told.
async fn project_workspace(
    state: &AppState,
    id: Uuid,
    user_id: Uuid,
) -> Result<(projects::ProjectRow, Uuid), Box<Response>> {
    let proj = match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => proj,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Project not found")),
            )
                .into_response()
                .into());
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
                .into());
        }
    };
    let Some(workspace_id) = proj.workspace_id else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response()
            .into());
    };
    match workspace_members::is_member(state.db(), user_id, workspace_id).await {
        Ok(true) => Ok((proj, workspace_id)),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response()
            .into()),
        Err(e) => {
            tracing::error!("Database error checking membership: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
                .into())
        }
    }
}

/// GET /api/projects/:id/automation
pub async fn automation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };
    if let Err(refusal) = project_workspace(&state, id, user_id).await {
        return *refusal;
    }
    let (automation, tasks, planner, updates) = tokio::join!(
        auto_projects::automation(state.db(), id),
        auto_projects::project_tasks(state.db(), id),
        chats::project_chat(state.db(), id, chats::ChatPurpose::ProjectPlanner),
        chats::project_chat(state.db(), id, chats::ChatPurpose::ProjectUpdates),
    );
    let (Ok(Some(automation)), Ok(tasks)) = (automation, tasks) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    };
    let listed: Vec<AutomationTask> = tasks
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
    Json(AutomationResponse {
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
    .into_response()
}

/// POST /api/projects/:id/automation/resume
///
/// Clears a pause -- the project's, and every task's -- and pokes the driver.
pub async fn resume_automation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&auth.0.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Invalid user ID in token")),
            )
                .into_response();
        }
    };
    let (_, workspace_id) = match project_workspace(&state, id, user_id).await {
        Ok(found) => found,
        Err(refusal) => return *refusal,
    };
    if let Some(refusal) = refuse_unless_writable(&state, workspace_id, user_id).await {
        return refusal;
    }
    if let Some(refusal) = refuse_unless_driven(&state) {
        return refusal;
    }
    if let Err(e) = auto_projects::resume(state.db(), id).await {
        tracing::error!("Database error: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    }
    if let Err(e) = auto_projects::resume_paused_tasks(state.db(), id).await {
        tracing::error!("Database error: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    }
    crate::workers::auto_project::poke(id);
    match projects::get_project(state.db(), id).await {
        Ok(Some(proj)) => Json(respond(&state, proj).await).into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Project not found")),
        )
            .into_response(),
    }
}
