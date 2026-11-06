//! Project endpoints

use std::collections::HashMap;
use std::fmt;

use abnegate_secret::REDACTED;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::github_installations::{self, InstallationStatus, InstallationSummary};
use crate::db::projects::{self, LinkError, NewProject, ProjectRow, Reorder};
use crate::db::{auto_projects, chats, sources, sync_config, workspace_members};
use crate::error::ServerError;
use crate::services::model::Model;
use crate::services::route::Route;
use crate::state::AppState;
use crate::sync::hook::Removal;

use super::common::Timestamps;

/// The statuses `projects.status` accepts.
pub const PROJECT_STATUSES: [&str; 3] = ["active", "on_hold", "cancelled"];

const INTERNAL_ERROR: &str = "Internal server error";

const CONFLICTING_CREDENTIALS: &str =
    "Send either access_token or github_installation_id, not both.";

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

/// An installation id the caller gave, once it is confirmed to belong to
/// `workspace_id`'s organization and to be neither revoked nor suspended.
async fn attachable_installation(
    state: &AppState,
    workspace_id: Uuid,
    installation_id: Uuid,
) -> Result<Uuid, ServerError> {
    github_installations::attachable(state.db(), workspace_id, installation_id)
        .await?
        .map(|installation| installation.id)
        .ok_or_else(unattachable)
}

fn unattachable() -> ServerError {
    ServerError::BadRequest(github_installations::UNATTACHABLE.to_string())
}

/// The installations `ids` name, keyed by id, in one query.
async fn installations(
    state: &AppState,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, InstallationSummary>, ServerError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(github_installations::summaries(state.db(), ids)
        .await?
        .into_iter()
        .map(|summary| (summary.id, summary))
        .collect())
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

/// One project's response, with what automation and GitHub know about it.
async fn describe(state: &AppState, row: ProjectRow) -> Result<ProjectResponse, ServerError> {
    let installation_ids: Vec<Uuid> = row.github_installation_id.into_iter().collect();
    let (automation, installations) = tokio::join!(
        auto_projects::automation(state.db(), row.id),
        installations(state, &installation_ids),
    );
    let installations = installations?;
    let installation = row
        .github_installation_id
        .and_then(|id| installations.get(&id));
    Ok(ProjectResponse {
        project: ProjectData::from(row)
            .with_automation(automation.ok().flatten().as_ref())
            .with_installation(installation),
    })
}

async fn respond(state: &AppState, project: Option<ProjectRow>) -> Result<Response, ServerError> {
    let project = project.ok_or_else(not_found)?;
    Ok(Json(describe(state, project).await?).into_response())
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
    github_installation_id: Option<Uuid>,
    github_installation_status: Option<InstallationStatus>,
    github_account_login: Option<String>,
    position: i32,
    /// Listed only, and only once a member has typed in or added to the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_used_at: Option<String>,
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

    /// Carry the status and account of the GitHub installation behind the
    /// project's repository into the response.
    fn with_installation(mut self, installation: Option<&InstallationSummary>) -> Self {
        if let Some(installation) = installation {
            self.github_installation_status = Some(installation.status());
            self.github_account_login = Some(installation.account_login.clone());
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
            github_installation_id: row.github_installation_id,
            github_installation_status: None,
            github_account_login: None,
            position: row.position,
            last_used_at: None,
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

#[derive(Debug, Deserialize)]
pub struct ReorderProjectsRequest {
    project_ids: Vec<Uuid>,
}

/// Link source request
#[derive(Debug, Deserialize)]
pub struct LinkSourceRequest {
    source_id: Uuid,
}

/// Link GitHub request
#[derive(Deserialize)]
pub struct LinkGitHubRequest {
    repo_url: String,
    access_token: Option<String>,
    github_installation_id: Option<Uuid>,
}

impl fmt::Debug for LinkGitHubRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LinkGitHubRequest")
            .field("repo_url", &self.repo_url)
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| REDACTED),
            )
            .field("github_installation_id", &self.github_installation_id)
            .finish()
    }
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
    Json(request): Json<AutoProjectRequest>,
) -> Result<Response, ServerError> {
    let user_id = user_id(&auth)?;
    refuse_unless_writable(&state, workspace_id, user_id).await?;
    refuse_unless_driven(&state)?;
    let brief = request.brief.trim();
    if brief.is_empty() {
        return Err(ServerError::BadRequest("A brief is required".to_string()));
    }
    if brief.chars().count() > BRIEF_CHARS {
        return Err(ServerError::BadRequest("The brief is too long".to_string()));
    }
    let model = request
        .model_name
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(crate::services::stages::AUTO)
        .to_string();
    let endpoint = Route::for_workspace(&state, workspace_id)
        .await
        .into_endpoint()
        .map_err(|unusable| ServerError::Conflict(unusable.to_string()))?;
    if Model::profile_on(endpoint.origin(), &state.config().ollama_host, &model)
        .await
        .completion
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

    let (rows, automation, used) = tokio::join!(
        projects::list_projects(state.db(), query.workspace_id, query.status.as_deref()),
        auto_projects::automation_for_workspace(state.db(), query.workspace_id),
        projects::last_used(state.db(), query.workspace_id),
    );
    let rows = rows?;
    let used = used?;
    let automation: HashMap<Uuid, auto_projects::ProjectAutomation> = automation
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.project_id, entry))
        .collect();
    let mut installation_ids: Vec<Uuid> = rows
        .iter()
        .filter_map(|row| row.github_installation_id)
        .collect();
    installation_ids.sort_unstable();
    installation_ids.dedup();
    let installations = installations(&state, &installation_ids).await?;
    Ok(Json(ProjectsListResponse {
        projects: rows
            .into_iter()
            .map(|row| {
                let known = automation.get(&row.id);
                let installation = row
                    .github_installation_id
                    .and_then(|id| installations.get(&id));
                let last_used_at = used.get(&row.id).map(|at| at.and_utc().to_rfc3339());
                ProjectData {
                    last_used_at,
                    ..ProjectData::from(row)
                        .with_automation(known)
                        .with_installation(installation)
                }
            })
            .collect(),
    })
    .into_response())
}

/// PUT /api/workspaces/{workspace_id}/projects/order
pub async fn reorder(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<ReorderProjectsRequest>,
) -> Result<Response, ServerError> {
    let user_id = user_id(&auth)?;
    if !workspace_members::is_member(state.db(), user_id, workspace_id).await? {
        return Err(ServerError::NotFound("Workspace not found".to_string()));
    }
    if !workspace_members::can_write(state.db(), workspace_id, user_id).await? {
        return Err(ServerError::Forbidden(
            "Workspace write access required".to_string(),
        ));
    }
    match projects::reorder(state.db(), workspace_id, &request.project_ids).await? {
        Reorder::Reordered => Ok(StatusCode::NO_CONTENT.into_response()),
        Reorder::Duplicated => Err(ServerError::BadRequest(
            "The order must name each project at most once".to_string(),
        )),
    }
}

/// POST /api/projects
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(request): Json<CreateProjectRequest>,
) -> Result<Response, ServerError> {
    let user_id = user_id(&auth)?;
    if !workspace_members::can_write(state.db(), request.workspace_id, user_id).await? {
        return Err(ServerError::Forbidden(
            "Workspace write access required".to_string(),
        ));
    }

    let status = request.status.as_deref().map(known_status).transpose()?;
    let source_id = match request.source_id {
        Some(source_id) => {
            Some(source_in_workspace(&state, source_id, request.workspace_id).await?)
        }
        None => None,
    };

    let project = projects::create(
        state.db(),
        NewProject {
            name: &request.name,
            description: request.description.as_deref(),
            workspace_id: Some(request.workspace_id),
            source_id,
            status,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(describe(&state, project).await?)).into_response())
}

/// GET /api/projects/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    let project = readable(&state, &auth, id).await?;
    Ok(Json(describe(&state, project).await?).into_response())
}

/// PUT|PATCH /api/projects/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateProjectRequest>,
) -> Result<Response, ServerError> {
    let (_, workspace_id) = writable(&state, &auth, id).await?;
    let user_id = user_id(&auth)?;
    let status = request.status.as_deref().map(known_status).transpose()?;

    if let Some(auto) = request.auto {
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
            request.name.as_deref(),
            request.description.as_deref(),
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

    let removal = hooks(&state, id).await?;
    if projects::delete_project(state.db(), id).await? {
        removal.run(&state).await;
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
    Json(request): Json<LinkSourceRequest>,
) -> Result<Response, ServerError> {
    let (_, workspace_id) = writable(&state, &auth, id).await?;
    let source_id = source_in_workspace(&state, request.source_id, workspace_id).await?;

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
    Json(request): Json<LinkGitHubRequest>,
) -> Result<Response, ServerError> {
    let (_, workspace_id) = writable(&state, &auth, id).await?;
    if request.access_token.is_some() && request.github_installation_id.is_some() {
        return Err(ServerError::BadRequest(CONFLICTING_CREDENTIALS.to_string()));
    }
    let installation = match request.github_installation_id {
        Some(installation_id) => {
            Some(attachable_installation(&state, workspace_id, installation_id).await?)
        }
        None => None,
    };

    let sealed = request
        .access_token
        .as_deref()
        .map(|token| encrypt_value(&SecretValue::new(token), state.master_key()))
        .transpose()
        .map_err(|error| {
            tracing::error!(%error, "Could not encrypt the repository token");
            ServerError::Internal(INTERNAL_ERROR.to_string())
        })?;
    let linked = projects::link_github(
        state.db(),
        id,
        &request.repo_url,
        sealed.as_deref(),
        installation,
    )
    .await
    .map_err(link_refusal)?;
    respond(&state, linked).await
}

fn link_refusal(error: LinkError) -> ServerError {
    match error {
        LinkError::Unattachable => unattachable(),
        LinkError::Database(error) => ServerError::from(error),
    }
}

/// DELETE /api/projects/:id/github
pub async fn unlink_github(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ServerError> {
    writable(&state, &auth, id).await?;

    let removal = hooks(&state, id).await?;
    let unlinked = projects::unlink_github(state.db(), id).await?;
    if unlinked.is_some() {
        removal.run(&state).await;
    }
    respond(&state, unlinked).await
}

/// The webhooks Zone registered for project `id`'s syncs, resolved while the
/// project still holds the token to remove them with.
async fn hooks(state: &AppState, id: Uuid) -> Result<Removal, ServerError> {
    let rows = sync_config::list_sync_configs(state.db(), id).await?;
    Ok(Removal::of(state, &rows).await)
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn an_installation_refused_at_the_write_is_a_bad_request() {
        assert!(matches!(
            link_refusal(LinkError::Unattachable),
            ServerError::BadRequest(message) if message == github_installations::UNATTACHABLE
        ));
        assert!(!matches!(
            link_refusal(LinkError::Database(sqlx::Error::PoolTimedOut)),
            ServerError::BadRequest(_)
        ));
    }

    fn row(github_installation_id: Option<Uuid>) -> ProjectRow {
        ProjectRow {
            id: Uuid::new_v4(),
            workspace_id: Some(Uuid::new_v4()),
            source_id: None,
            name: "Zone".to_string(),
            description: None,
            status: "active".to_string(),
            github_repo_url: Some("https://github.com/abnegate/zone".to_string()),
            github_access_token: Some("ENC[v1:sealed]".to_string()),
            created_at: None,
            updated_at: None,
            github_installation_id,
            position: 0,
        }
    }

    fn summary(id: Uuid, revoked: bool) -> InstallationSummary {
        InstallationSummary {
            id,
            account_login: "abnegate".to_string(),
            suspended_at: Some(Utc::now()),
            revoked_at: revoked.then(Utc::now),
        }
    }

    fn serialized(data: ProjectData) -> Value {
        serde_json::to_value(data).unwrap()
    }

    #[test]
    fn a_token_project_reports_no_installation() {
        let body = serialized(ProjectData::from(row(None)).with_installation(None));

        assert_eq!(body["github_installation_id"], Value::Null);
        assert_eq!(body["github_installation_status"], Value::Null);
        assert_eq!(body["github_account_login"], Value::Null);
        assert!(
            body.get("github_access_token").is_none(),
            "the sealed token must never reach a response"
        );
    }

    #[test]
    fn an_installation_project_reports_its_status_and_account() {
        let installation_id = Uuid::new_v4();
        let installation = summary(installation_id, false);

        let body = serialized(
            ProjectData::from(row(Some(installation_id))).with_installation(Some(&installation)),
        );

        assert_eq!(body["github_installation_id"], json!(installation_id));
        assert_eq!(body["github_installation_status"], json!("suspended"));
        assert_eq!(body["github_account_login"], json!("abnegate"));
    }

    #[test]
    fn revocation_outranks_suspension_in_the_response() {
        let installation_id = Uuid::new_v4();
        let installation = summary(installation_id, true);

        let body = serialized(
            ProjectData::from(row(Some(installation_id))).with_installation(Some(&installation)),
        );

        assert_eq!(body["github_installation_status"], json!("revoked"));
    }

    #[test]
    fn a_link_request_accepts_an_installation_without_a_token() {
        let installation_id = Uuid::new_v4();

        let request: LinkGitHubRequest = serde_json::from_value(json!({
            "repo_url": "https://github.com/abnegate/zone",
            "github_installation_id": installation_id,
        }))
        .unwrap();

        assert_eq!(request.github_installation_id, Some(installation_id));
        assert_eq!(request.access_token, None);
    }

    #[test]
    fn debug_redacts_the_access_token() {
        let request: LinkGitHubRequest = serde_json::from_value(json!({
            "repo_url": "https://github.com/abnegate/zone",
            "access_token": "ghp_plaintext_token",
        }))
        .unwrap();

        let rendered = format!("{request:?}");

        assert!(!rendered.contains("ghp_plaintext_token"), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
        assert!(rendered.contains("abnegate/zone"), "{rendered}");
    }
}
