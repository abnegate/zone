//! Workspace member management endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{OrgMember, WorkspaceAdmin, WorkspaceMember};
use crate::db::{workspace_members, workspaces};
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

// ============================================================================
// Workspace Endpoints
// ============================================================================

/// Workspace response
#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<workspaces::WorkspaceRow> for WorkspaceResponse {
    fn from(row: workspaces::WorkspaceRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            name: row.name,
            slug: row.slug,
            description: row.description,
            is_active: row.is_active.unwrap_or(true),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct OrgIdPath {
    org_id: Uuid,
}

/// GET /api/organizations/:org_id/workspaces - List accessible workspaces
pub async fn list_accessible_workspaces(
    State(state): State<AppState>,
    member: OrgMember,
) -> impl IntoResponse {
    match workspace_members::list_user_workspaces_in_org(state.db(), member.user_id, member.org_id)
        .await
    {
        Ok(workspaces) => Json(
            workspaces
                .into_iter()
                .map(WorkspaceResponse::from)
                .collect::<Vec<_>>(),
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

/// GET /api/workspaces/:workspace_id - Get workspace (requires member)
pub async fn get_workspace(
    State(state): State<AppState>,
    member: WorkspaceMember,
) -> impl IntoResponse {
    match workspaces::get_workspace(state.db(), member.workspace_id).await {
        Ok(Some(ws)) => Json(WorkspaceResponse::from(ws)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Workspace not found")),
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

#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// PATCH /api/workspaces/:workspace_id - Update workspace (requires admin)
pub async fn update_workspace(
    State(state): State<AppState>,
    admin: WorkspaceAdmin,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> impl IntoResponse {
    match workspaces::update_workspace(
        state.db(),
        admin.workspace_id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.is_active,
    )
    .await
    {
        Ok(Some(ws)) => Json(WorkspaceResponse::from(ws)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Workspace not found")),
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

/// DELETE /api/workspaces/:workspace_id - Delete workspace (requires admin)
pub async fn delete_workspace(
    State(state): State<AppState>,
    admin: WorkspaceAdmin,
) -> impl IntoResponse {
    match workspaces::delete_workspace(state.db(), admin.workspace_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Workspace not found")),
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

// ============================================================================
// Workspace Member Management
// ============================================================================

/// Workspace member response
#[derive(Debug, Serialize)]
pub struct WorkspaceMemberResponse {
    id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    role: String,
    is_active: bool,
    invited_by: Option<Uuid>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<workspace_members::WorkspaceMemberRow> for WorkspaceMemberResponse {
    fn from(row: workspace_members::WorkspaceMemberRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            user_id: row.user_id,
            role: row.role.as_str().to_string(),
            is_active: row.is_active,
            invited_by: row.invited_by,
            timestamps: Timestamps::from_naive(Some(row.created_at), Some(row.updated_at)),
        }
    }
}

/// Add workspace member request
#[derive(Debug, Deserialize)]
pub struct AddWorkspaceMemberRequest {
    user_id: Uuid,
    role: String,
}

/// Update workspace member role request
#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceMemberRoleRequest {
    role: String,
}

/// GET /api/workspaces/:workspace_id/members - List members (requires member)
pub async fn list_members(
    State(state): State<AppState>,
    member: WorkspaceMember,
) -> impl IntoResponse {
    match workspace_members::list_members(state.db(), member.workspace_id).await {
        Ok(members) => Json(
            members
                .into_iter()
                .map(WorkspaceMemberResponse::from)
                .collect::<Vec<_>>(),
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

/// POST /api/workspaces/:workspace_id/members - Add member (requires admin)
pub async fn add_member(
    State(state): State<AppState>,
    admin: WorkspaceAdmin,
    Json(req): Json<AddWorkspaceMemberRequest>,
) -> impl IntoResponse {
    let role = match req.role.parse::<workspace_members::WorkspaceRole>() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid role. Must be one of: viewer, member, admin",
                )),
            )
                .into_response();
        }
    };

    // CRITICAL-7: Check if member already exists (active or inactive)
    match workspace_members::get_member(state.db(), admin.workspace_id, req.user_id).await {
        Ok(Some(existing_member)) => {
            if existing_member.is_active {
                // Member is already active - return 409 CONFLICT
                (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new(
                        "User is already an active member of this workspace",
                    )),
                )
                    .into_response()
            } else {
                // Member exists but is inactive - reactivate
                match workspace_members::reactivate_member(
                    state.db(),
                    admin.workspace_id,
                    req.user_id,
                    role,
                    Some(admin.user_id),
                )
                .await
                {
                    Ok(_) => {
                        // Fetch the reactivated member to return
                        match workspace_members::get_member(
                            state.db(),
                            admin.workspace_id,
                            req.user_id,
                        )
                        .await
                        {
                            Ok(Some(member)) => {
                                (StatusCode::OK, Json(WorkspaceMemberResponse::from(member)))
                                    .into_response()
                            }
                            Ok(None) => (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ErrorResponse::new("Failed to retrieve reactivated member")),
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
                    Err(e) => {
                        tracing::error!("Database error reactivating member: {}", e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse::new("Internal server error")),
                        )
                            .into_response()
                    }
                }
            }
        }
        Ok(None) => {
            // Member doesn't exist - add new member
            match workspace_members::add_member(
                state.db(),
                admin.workspace_id,
                req.user_id,
                role,
                Some(admin.user_id),
            )
            .await
            {
                Ok(_member_id) => {
                    // Fetch the created member to return
                    match workspace_members::get_member(state.db(), admin.workspace_id, req.user_id)
                        .await
                    {
                        Ok(Some(member)) => (
                            StatusCode::CREATED,
                            Json(WorkspaceMemberResponse::from(member)),
                        )
                            .into_response(),
                        Ok(None) => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse::new("Failed to retrieve created member")),
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
                Err(e) => {
                    tracing::error!("Database error adding member: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Internal server error")),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Database error checking member: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceMemberPath {
    workspace_id: Uuid,
    user_id: Uuid,
}

/// PATCH /api/workspaces/:workspace_id/members/:user_id - Update role (requires admin)
pub async fn update_member_role(
    State(state): State<AppState>,
    admin: WorkspaceAdmin,
    Path(path): Path<WorkspaceMemberPath>,
    Json(req): Json<UpdateWorkspaceMemberRoleRequest>,
) -> impl IntoResponse {
    let role = match req.role.parse::<workspace_members::WorkspaceRole>() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid role. Must be one of: viewer, member, admin, owner",
                )),
            )
                .into_response();
        }
    };

    // NEW-MAJOR-1: Prevent privilege escalation - only owners can grant admin or owner role
    if role >= workspace_members::WorkspaceRole::Admin
        && admin.role != workspace_members::WorkspaceRole::Owner
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Only workspace owners can promote users to admin or owner",
            )),
        )
            .into_response();
    }

    match workspace_members::update_member_role(state.db(), admin.workspace_id, path.user_id, role)
        .await
    {
        Ok(member) => Json(WorkspaceMemberResponse::from(member)).into_response(),
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

/// DELETE /api/workspaces/:workspace_id/members/:user_id - Remove member (requires admin)
pub async fn remove_member(
    State(state): State<AppState>,
    admin: WorkspaceAdmin,
    Path(path): Path<WorkspaceMemberPath>,
) -> impl IntoResponse {
    // NEW-MAJOR-2: Get the target member to check permissions
    let target_member =
        match workspace_members::get_member(state.db(), admin.workspace_id, path.user_id).await {
            Ok(Some(member)) => member,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new("Member not found")),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("Database error fetching member: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        };

    // NEW-MAJOR-2: Check role hierarchy - only owners can remove admins/owners
    if target_member.role >= workspace_members::WorkspaceRole::Admin {
        if admin.role != workspace_members::WorkspaceRole::Owner {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new(
                    "Only workspace owners can remove admins or owners",
                )),
            )
                .into_response();
        }

        // NEW-MAJOR-2: Prevent removal of last admin
        match workspace_members::count_admins(state.db(), admin.workspace_id).await {
            Ok(count) if count <= 1 => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse::new(
                        "Cannot remove the last admin from the workspace",
                    )),
                )
                    .into_response();
            }
            Ok(_) => {
                // More than one admin, proceed
            }
            Err(e) => {
                tracing::error!("Database error counting admins: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    match workspace_members::remove_member(state.db(), admin.workspace_id, path.user_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Member not found")),
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
