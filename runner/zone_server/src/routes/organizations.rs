//! Organization endpoints

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AuthUser, OrgAdmin, OrgMember, OrgOwner, WorkspaceAdmin, WorkspaceMember};
use crate::db::workspace_members::WorkspaceRole;
use crate::db::{organization_members, organizations, workspace_members, workspaces};
use crate::state::AppState;

use super::common::{ErrorResponse, Timestamps};

/// Organization response
#[derive(Debug, Serialize)]
pub struct OrganizationResponse {
    id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<organizations::OrganizationRow> for OrganizationResponse {
    fn from(row: organizations::OrganizationRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            slug: row.slug,
            description: row.description,
            is_active: row.is_active.unwrap_or(true),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

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

/// Create organization request
#[derive(Debug, Deserialize)]
pub struct CreateOrganizationRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

/// Update organization request
#[derive(Debug, Deserialize)]
pub struct UpdateOrganizationRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// Create workspace request
#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

/// Update workspace request
#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
struct SingleOrganizationResponse {
    organization: OrganizationResponse,
}

/// Organizations list response wrapper
#[derive(Debug, Serialize)]
struct OrganizationsListResponse {
    organizations: Vec<OrganizationResponse>,
}

/// GET /api/organizations - List user's organizations
pub async fn list(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
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

    match organization_members::list_user_organizations(state.db(), user_id).await {
        Ok(orgs) => Json(OrganizationsListResponse {
            organizations: orgs.into_iter().map(OrganizationResponse::from).collect(),
        })
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

/// POST /api/organizations - Create new organization (user becomes owner)
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateOrganizationRequest>,
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

    // MAJOR-1: Wrap organization creation in a transaction
    let mut tx = match state.db().begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Create organization
    let org = match organizations::create_organization_tx(
        &mut tx,
        &req.name,
        &req.slug,
        req.description.as_deref(),
    )
    .await
    {
        Ok(org) => org,
        Err(e) => {
            tracing::error!("Database error creating organization: {}", e);
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Add creator as owner
    if let Err(e) = organization_members::add_member_tx(
        &mut tx,
        org.id,
        user_id,
        organization_members::OrgRole::Owner,
        None,
    )
    .await
    {
        tracing::error!("Failed to add organization owner: {}", e);
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(
                "Failed to create organization membership",
            )),
        )
            .into_response();
    }

    // Commit transaction
    if let Err(e) = tx.commit().await {
        tracing::error!("Failed to commit transaction: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Internal server error")),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(SingleOrganizationResponse {
            organization: OrganizationResponse::from(org),
        }),
    )
        .into_response()
}

/// GET /api/organizations/:org_id - Get org details (requires membership)
pub async fn get(State(state): State<AppState>, member: OrgMember) -> impl IntoResponse {
    match organizations::get_organization(state.db(), member.org_id).await {
        Ok(Some(org)) => Json(SingleOrganizationResponse {
            organization: OrganizationResponse::from(org),
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
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

/// PATCH /api/organizations/:org_id - Update org (requires admin)
pub async fn update(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Json(req): Json<UpdateOrganizationRequest>,
) -> impl IntoResponse {
    match organizations::update_organization(
        state.db(),
        admin.org_id,
        req.name.as_deref(),
        req.description.as_deref(),
        req.is_active,
    )
    .await
    {
        Ok(Some(org)) => Json(SingleOrganizationResponse {
            organization: OrganizationResponse::from(org),
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
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

/// DELETE /api/organizations/:org_id - Delete org (requires owner)
pub async fn delete(State(state): State<AppState>, owner: OrgOwner) -> impl IntoResponse {
    match organizations::delete_organization(state.db(), owner.org_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
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

#[derive(Debug, Serialize)]
struct SingleWorkspaceResponse {
    workspace: WorkspaceResponse,
}

/// Workspaces list response wrapper
#[derive(Debug, Serialize)]
struct WorkspacesListResponse {
    workspaces: Vec<WorkspaceResponse>,
}

/// GET /api/organizations/:org_id/workspaces
pub async fn list_workspaces(
    State(state): State<AppState>,
    member: OrgMember,
) -> impl IntoResponse {
    match workspaces::list_workspaces(state.db(), member.org_id).await {
        Ok(ws) => Json(WorkspacesListResponse {
            workspaces: ws.into_iter().map(WorkspaceResponse::from).collect(),
        })
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

/// POST /api/organizations/:org_id/workspaces
pub async fn create_workspace(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    // Create the workspace
    let ws = match workspaces::create_workspace(
        state.db(),
        admin.org_id,
        &req.name,
        &req.slug,
        req.description.as_deref(),
    )
    .await
    {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("Database error creating workspace: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Add the creator as a workspace admin
    if let Err(e) =
        workspace_members::add_member(state.db(), ws.id, admin.user_id, WorkspaceRole::Admin, None)
            .await
    {
        tracing::error!("Failed to add creator as workspace member: {}", e);
        // Still return success since workspace was created - the user can add themselves later
    }

    (
        StatusCode::CREATED,
        Json(SingleWorkspaceResponse {
            workspace: WorkspaceResponse::from(ws),
        }),
    )
        .into_response()
}

/// GET /api/workspaces/:id
pub async fn get_workspace(
    State(state): State<AppState>,
    member: WorkspaceMember,
) -> impl IntoResponse {
    match workspaces::get_workspace(state.db(), member.workspace_id).await {
        Ok(Some(ws)) => Json(SingleWorkspaceResponse {
            workspace: WorkspaceResponse::from(ws),
        })
        .into_response(),
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

/// PUT /api/workspaces/:id
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
        Ok(Some(ws)) => Json(SingleWorkspaceResponse {
            workspace: WorkspaceResponse::from(ws),
        })
        .into_response(),
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

/// DELETE /api/workspaces/:id
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
// Organization Member Management
// ============================================================================

/// Organization member response
#[derive(Debug, Serialize)]
pub struct OrganizationMemberResponse {
    id: Uuid,
    organization_id: Uuid,
    user_id: Uuid,
    role: String,
    is_active: bool,
    invited_by: Option<Uuid>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<organization_members::OrganizationMemberRow> for OrganizationMemberResponse {
    fn from(row: organization_members::OrganizationMemberRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            user_id: row.user_id,
            role: row.role.as_str().to_string(),
            is_active: row.is_active,
            invited_by: row.invited_by,
            timestamps: Timestamps::from_naive(Some(row.created_at), Some(row.updated_at)),
        }
    }
}

/// Add member request
#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    user_id: Uuid,
    role: String,
}

/// Update member role request
#[derive(Debug, Deserialize)]
pub struct UpdateMemberRoleRequest {
    role: String,
}

#[derive(Debug, Serialize)]
struct OrganizationMembersListResponse {
    members: Vec<OrganizationMemberResponse>,
}

/// GET /api/organizations/:org_id/members - List members (requires member)
pub async fn list_members(State(state): State<AppState>, member: OrgMember) -> impl IntoResponse {
    match organization_members::list_members(state.db(), member.org_id).await {
        Ok(members) => Json(OrganizationMembersListResponse {
            members: members
                .into_iter()
                .map(OrganizationMemberResponse::from)
                .collect(),
        })
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

/// POST /api/organizations/:org_id/members - Add member (requires admin)
pub async fn add_member(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    let role = match req.role.parse::<organization_members::OrgRole>() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid role. Must be one of: member, admin, owner",
                )),
            )
                .into_response();
        }
    };

    // CRITICAL-7: Check if member already exists (active or inactive)
    match organization_members::get_member(state.db(), admin.org_id, req.user_id).await {
        Ok(Some(existing)) => {
            if existing.is_active {
                return (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new(
                        "User is already a member of this organization",
                    )),
                )
                    .into_response();
            } else {
                // Member exists but is inactive - use reactivate instead
                match organization_members::reactivate_member(
                    state.db(),
                    admin.org_id,
                    req.user_id,
                    role,
                    Some(admin.user_id),
                )
                .await
                {
                    Ok(member) => {
                        return (
                            StatusCode::OK,
                            Json(OrganizationMemberResponse::from(member)),
                        )
                            .into_response();
                    }
                    Err(e) => {
                        tracing::error!("Database error reactivating member: {}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse::new("Internal server error")),
                        )
                            .into_response();
                    }
                }
            }
        }
        Ok(None) => {
            // Member doesn't exist, proceed with add
        }
        Err(e) => {
            tracing::error!("Database error checking existing member: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    match organization_members::add_member(
        state.db(),
        admin.org_id,
        req.user_id,
        role,
        Some(admin.user_id),
    )
    .await
    {
        Ok(member) => (
            StatusCode::CREATED,
            Json(OrganizationMemberResponse::from(member)),
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
pub struct MemberPath {
    org_id: Uuid,
    user_id: Uuid,
}

/// PATCH /api/organizations/:org_id/members/:user_id - Update role (requires admin)
pub async fn update_member_role(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Path(path): Path<MemberPath>,
    Json(req): Json<UpdateMemberRoleRequest>,
) -> impl IntoResponse {
    let role = match req.role.parse::<organization_members::OrgRole>() {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid role. Must be one of: member, admin, owner",
                )),
            )
                .into_response();
        }
    };

    // CRITICAL-4: Prevent privilege escalation - only owners can grant owner role
    if role == organization_members::OrgRole::Owner
        && admin.role != organization_members::OrgRole::Owner
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Only owners can grant owner role")),
        )
            .into_response();
    }

    match organization_members::update_member_role(state.db(), admin.org_id, path.user_id, role)
        .await
    {
        Ok(member) => Json(OrganizationMemberResponse::from(member)).into_response(),
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

/// DELETE /api/organizations/:org_id/members/:user_id - Remove member (requires admin)
pub async fn remove_member(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Path(path): Path<MemberPath>,
) -> impl IntoResponse {
    // CRITICAL-6: Get the target member's role to check permissions
    let target_member =
        match organization_members::get_member(state.db(), admin.org_id, path.user_id).await {
            Ok(Some(member)) => member,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new("Member not found")),
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

    // CRITICAL-6: Check role hierarchy - can't remove someone with higher or equal role
    // Owner > Admin > Member
    if admin.role != organization_members::OrgRole::Owner {
        // Non-owners cannot remove admins or owners
        if target_member.role >= organization_members::OrgRole::Admin {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new(
                    "Only owners can remove admins or owners",
                )),
            )
                .into_response();
        }
    }

    // CRITICAL-6: Prevent removal of last owner
    if target_member.role == organization_members::OrgRole::Owner {
        match organization_members::count_owners(state.db(), admin.org_id).await {
            Ok(count) if count <= 1 => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse::new(
                        "Cannot remove the last owner of the organization",
                    )),
                )
                    .into_response();
            }
            Ok(_) => {
                // More than one owner, proceed
            }
            Err(e) => {
                tracing::error!("Database error counting owners: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    match organization_members::remove_member(state.db(), admin.org_id, path.user_id).await {
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
