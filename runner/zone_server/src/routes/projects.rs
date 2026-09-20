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
use crate::db::{sources, workspace_members};
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

fn respond(project: Option<ProjectRow>) -> Result<Response, ServerError> {
    project
        .map(|project| Json(ProjectResponse::from(project)).into_response())
        .ok_or_else(not_found)
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
    #[serde(flatten)]
    timestamps: Timestamps,
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
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl From<ProjectRow> for ProjectResponse {
    fn from(row: ProjectRow) -> Self {
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
    source_id: Option<Uuid>,
    status: Option<String>,
}

/// Update project request; every field is optional and only the given ones change
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
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
    Ok(Json(ProjectsListResponse {
        projects: rows.into_iter().map(ProjectData::from).collect(),
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
    Ok((StatusCode::CREATED, Json(ProjectResponse::from(project))).into_response())
}

/// GET /api/projects/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    let project = readable(&state, &auth, id).await?;
    Ok(Json(ProjectResponse::from(project)).into_response())
}

/// PUT|PATCH /api/projects/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;
    let status = req.status.as_deref().map(known_status).transpose()?;

    respond(
        projects::update_project(
            state.db(),
            id,
            req.name.as_deref(),
            req.description.as_deref(),
            status,
        )
        .await?,
    )
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

    respond(projects::set_source(state.db(), id, Some(source_id)).await?)
}

/// DELETE /api/projects/:id/source
pub async fn unlink_source(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    respond(projects::set_source(state.db(), id, None).await?)
}

/// POST /api/projects/:id/github
pub async fn link_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<LinkGitHubRequest>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    respond(
        projects::link_github(state.db(), id, &req.repo_url, req.access_token.as_deref()).await?,
    )
}

/// DELETE /api/projects/:id/github
pub async fn unlink_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    respond(projects::unlink_github(state.db(), id).await?)
}
