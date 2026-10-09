//! Host folder mounts: instance mapping and per-workspace directory lists.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthUser, WorkspaceMember, WorkspaceWriter};
use crate::db::workspace_host_directories;
use crate::error::ServerError;
use crate::host_mounts::{HostMounts, MapError};
use crate::state::AppState;

use super::common::ErrorResponse;

#[derive(Debug, Serialize)]
pub struct HostMountsResponse {
    pub in_container: bool,
    pub host_root: Option<String>,
    pub container_root: Option<&'static str>,
    pub ready: bool,
    pub hint: String,
}

impl From<&HostMounts> for HostMountsResponse {
    fn from(mounts: &HostMounts) -> Self {
        Self {
            in_container: mounts.in_container,
            host_root: mounts
                .host_root
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            container_root: mounts.container_root(),
            ready: mounts.ready(),
            hint: mounts.hint(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MappedFolder {
    pub host: String,
    pub mapped: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Serialize)]
pub struct HostDirectoriesResponse {
    pub directories: Vec<String>,
    pub folders: Vec<MappedFolder>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateHostDirectoriesRequest {
    pub directories: Vec<String>,
}

fn folders(mounts: &HostMounts, directories: Vec<String>) -> HostDirectoriesResponse {
    let folders = directories
        .iter()
        .map(|host| match mounts.map(host) {
            Ok(mapped) => MappedFolder {
                host: host.clone(),
                mapped: Some(mapped.to_string_lossy().into_owned()),
                exists: mapped.is_dir(),
            },
            Err(_) => MappedFolder {
                host: host.clone(),
                mapped: None,
                exists: false,
            },
        })
        .collect();
    HostDirectoriesResponse {
        directories,
        folders,
    }
}

fn refusal(error: MapError, mounts: &HostMounts, host: &str) -> impl IntoResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::with_code(
            error.message(mounts.host_root.as_deref(), host),
            error.code(),
        )),
    )
}

/// GET /api/host-mounts
pub async fn instance(State(state): State<AppState>, _auth: AuthUser) -> Json<HostMountsResponse> {
    Json(HostMountsResponse::from(&state.config().host_mounts))
}

/// GET /api/workspaces/:workspace_id/host-directories
pub async fn get(
    State(state): State<AppState>,
    member: WorkspaceMember,
) -> Result<Json<HostDirectoriesResponse>, ServerError> {
    let directories = workspace_host_directories::list(state.db(), member.workspace_id).await?;
    Ok(Json(folders(&state.config().host_mounts, directories)))
}

/// PUT /api/workspaces/:workspace_id/host-directories
pub async fn put(
    State(state): State<AppState>,
    writer: WorkspaceWriter,
    body: Result<Json<UpdateHostDirectoriesRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ServerError> {
    let Json(request) = body.map_err(|error| ServerError::BadRequest(error.to_string()))?;
    let mounts = &state.config().host_mounts;
    let directories = match mounts.validate(&request.directories) {
        Ok(directories) => directories,
        Err((error, host)) => return Ok(refusal(error, mounts, &host).into_response()),
    };
    let directories =
        workspace_host_directories::upsert(state.db(), writer.workspace_id, &directories).await?;
    Ok(Json(folders(mounts, directories)).into_response())
}
