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
use crate::db::DbResult;
use crate::db::audit::{actions, resources};
use crate::db::{organization_members, organizations, workspaces};
use crate::services::login::devices;
use crate::state::AppState;
use crate::sync::hook::Removal;

use super::common::{AuditEvent, ErrorResponse, Timestamps, audit};

/// Organization response
#[derive(Debug, Serialize)]
pub struct OrganizationResponse {
    id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
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
            role: None,
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl OrganizationResponse {
    fn with_role(mut self, role: organization_members::OrgRole) -> Self {
        self.role = Some(role.as_str().to_string());
        self
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

    match organization_members::list_user_organizations_with_role(state.db(), user_id).await {
        Ok(orgs) => Json(OrganizationsListResponse {
            organizations: orgs
                .into_iter()
                .map(|membership| {
                    OrganizationResponse::from(membership.organization).with_role(membership.role)
                })
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
            organization: OrganizationResponse::from(org).with_role(member.role),
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
        Ok(Some(org)) => {
            let organization = OrganizationResponse::from(org).with_role(admin.role);
            audit(
                state.db(),
                AuditEvent {
                    organization_id: Some(admin.org_id),
                    workspace_id: None,
                    actor_id: admin.user_id,
                    actor_email: &admin.email,
                    action: actions::ORGANIZATION_UPDATED,
                    resource_type: resources::ORGANIZATION,
                    resource_id: Some(admin.org_id),
                    old_values: None,
                    new_values: Some(serde_json::json!({
                        "name": organization.name,
                        "description": organization.description,
                        "is_active": organization.is_active,
                    })),
                },
            )
            .await;
            Json(SingleOrganizationResponse { organization }).into_response()
        }
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
    let removal = match Removal::of_organization(&state, owner.org_id).await {
        Ok(removal) => removal,
        Err(error) => {
            tracing::error!(%error, "Failed to read the syncs whose webhooks the delete removes");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };
    let deleted = match tokio::spawn(erase(state.clone(), owner.org_id)).await {
        Ok(deleted) => deleted,
        Err(error) => {
            tracing::error!(organization = %owner.org_id, %error, "The delete did not finish");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };
    match deleted {
        Ok(true) => {
            removal.run(&state).await;
            audit(
                state.db(),
                AuditEvent {
                    organization_id: Some(owner.org_id),
                    workspace_id: None,
                    actor_id: owner.user_id,
                    actor_email: &owner.email,
                    action: actions::ORGANIZATION_DELETED,
                    resource_type: resources::ORGANIZATION,
                    resource_id: Some(owner.org_id),
                    old_values: None,
                    new_values: None,
                },
            )
            .await;
            if let Err(error) = devices::forget(&state, owner.org_id).await {
                tracing::error!(
                    organization = %owner.org_id,
                    %error,
                    "Could not forget a deleted organization's coding agent sign-ins"
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Organization not found")),
        )
            .into_response(),
        Err(error) => {
            tracing::error!("Database error: {}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// Deletes the organization and stops its MCP servers, which hold its secrets. Run as a task of
/// its own: a caller that disconnects drops the handler wherever it waits, and a delete that
/// committed must still stop the servers on this replica.
async fn erase(state: AppState, organization: Uuid) -> DbResult<bool> {
    let deleted = organizations::delete_organization(state.db(), organization).await?;
    if deleted {
        state.mcp().forget(organization);
    }
    Ok(deleted)
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
    let ws = match workspaces::create_workspace_with_owner(
        state.db(),
        admin.org_id,
        &req.name,
        &req.slug,
        req.description.as_deref(),
        admin.user_id,
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

    let workspace = WorkspaceResponse::from(ws);
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(admin.org_id),
            workspace_id: Some(workspace.id),
            actor_id: admin.user_id,
            actor_email: &admin.email,
            action: actions::WORKSPACE_CREATED,
            resource_type: resources::WORKSPACE,
            resource_id: Some(workspace.id),
            old_values: None,
            new_values: serde_json::to_value(&workspace).ok(),
        },
    )
    .await;

    (
        StatusCode::CREATED,
        Json(SingleWorkspaceResponse { workspace }),
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
        Ok(Some(ws)) => {
            let workspace = WorkspaceResponse::from(ws);
            audit(
                state.db(),
                AuditEvent {
                    organization_id: Some(workspace.organization_id),
                    workspace_id: Some(admin.workspace_id),
                    actor_id: admin.user_id,
                    actor_email: &admin.email,
                    action: actions::WORKSPACE_UPDATED,
                    resource_type: resources::WORKSPACE,
                    resource_id: Some(admin.workspace_id),
                    old_values: None,
                    new_values: serde_json::to_value(&workspace).ok(),
                },
            )
            .await;
            Json(SingleWorkspaceResponse { workspace }).into_response()
        }
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
    let organization_id = match workspaces::get_workspace(state.db(), admin.workspace_id).await {
        Ok(workspace) => workspace.map(|workspace| workspace.organization_id),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    let removal = match Removal::of_workspace(&state, admin.workspace_id).await {
        Ok(removal) => removal,
        Err(error) => {
            tracing::error!(%error, "Failed to read the syncs whose webhooks the delete removes");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };
    match workspaces::delete_workspace(state.db(), admin.workspace_id).await {
        Ok(true) => {
            removal.run(&state).await;
            audit(
                state.db(),
                AuditEvent {
                    organization_id,
                    workspace_id: Some(admin.workspace_id),
                    actor_id: admin.user_id,
                    actor_email: &admin.email,
                    action: actions::WORKSPACE_DELETED,
                    resource_type: resources::WORKSPACE,
                    resource_id: Some(admin.workspace_id),
                    old_values: None,
                    new_values: None,
                },
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
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

/// Organization member response
#[derive(Debug, Serialize)]
pub struct OrganizationMemberResponse {
    id: Uuid,
    organization_id: Uuid,
    user_id: Uuid,
    role: String,
    is_active: bool,
    invited_by: Option<Uuid>,
    email: Option<String>,
    display_name: Option<String>,
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
            email: None,
            display_name: None,
            timestamps: Timestamps::from_naive(Some(row.created_at), Some(row.updated_at)),
        }
    }
}

impl From<organization_members::MemberWithUser> for OrganizationMemberResponse {
    fn from(member: organization_members::MemberWithUser) -> Self {
        Self {
            email: Some(member.email),
            display_name: member.display_name,
            ..Self::from(member.member)
        }
    }
}

/// The member as the console lists them: with the account's email and name
/// when the account can be read, the bare seat otherwise.
async fn describe_member(
    state: &AppState,
    row: organization_members::OrganizationMemberRow,
) -> OrganizationMemberResponse {
    match organization_members::get_member_with_user(state.db(), row.organization_id, row.user_id)
        .await
    {
        Ok(Some(member)) => OrganizationMemberResponse::from(member),
        Ok(None) => OrganizationMemberResponse::from(row),
        Err(error) => {
            tracing::warn!(%error, user_id = %row.user_id, "Failed to read the member's account");
            OrganizationMemberResponse::from(row)
        }
    }
}

async fn audit_member(
    state: &AppState,
    admin: &OrgAdmin,
    action: &str,
    user_id: Uuid,
    role: Option<&str>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(admin.org_id),
            workspace_id: None,
            actor_id: admin.user_id,
            actor_email: &admin.email,
            action,
            resource_type: resources::MEMBER,
            resource_id: Some(user_id),
            old_values: None,
            new_values: role.map(|role| serde_json::json!({ "user_id": user_id, "role": role })),
        },
    )
    .await;
}

/// Add member request. The console's organization form names the invitee by
/// email, the workspace form and the API's own callers by id, so either
/// identifies the member and exactly one must be given.
#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    user_id: Option<Uuid>,
    email: Option<String>,
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
    match organization_members::list_members_with_users(state.db(), member.org_id).await {
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

    if role >= organization_members::OrgRole::Admin
        && admin.role != organization_members::OrgRole::Owner
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Only owners can grant admin or owner role",
            )),
        )
            .into_response();
    }

    let target = match (req.user_id, req.email.as_deref()) {
        (Some(user_id), None) => user_id,
        (None, Some(email)) => match crate::db::users::get_user_by_email(state.db(), email).await {
            Ok(Some(user)) => user.id,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new(
                        "No account uses that email. Send an invitation instead.",
                    )),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("Database error resolving email: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        },
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Name the member by either user_id or email",
                )),
            )
                .into_response();
        }
    };

    // CRITICAL-7: Check if member already exists (active or inactive)
    match organization_members::get_member(state.db(), admin.org_id, target).await {
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
                    target,
                    role,
                    Some(admin.user_id),
                )
                .await
                {
                    Ok(member) => {
                        audit_member(
                            &state,
                            &admin,
                            actions::MEMBER_ADDED,
                            member.user_id,
                            Some(member.role.as_str()),
                        )
                        .await;
                        let response = describe_member(&state, member).await;
                        return (StatusCode::OK, Json(response)).into_response();
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
        target,
        role,
        Some(admin.user_id),
    )
    .await
    {
        Ok(member) => {
            audit_member(
                &state,
                &admin,
                actions::MEMBER_ADDED,
                member.user_id,
                Some(member.role.as_str()),
            )
            .await;
            let response = describe_member(&state, member).await;
            (StatusCode::CREATED, Json(response)).into_response()
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

    match organization_members::change_role(
        state.db(),
        admin.org_id,
        path.user_id,
        role,
        admin.role,
    )
    .await
    {
        Ok(organization_members::RoleChange::Applied(member)) => {
            audit_member(
                &state,
                &admin,
                actions::MEMBER_ROLE_CHANGED,
                member.user_id,
                Some(member.role.as_str()),
            )
            .await;
            let response = describe_member(&state, *member).await;
            Json(response).into_response()
        }
        Ok(organization_members::RoleChange::Forbidden) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Only owners can change the role of an admin or owner",
            )),
        )
            .into_response(),
        Ok(organization_members::RoleChange::Missing) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Member not found")),
        )
            .into_response(),
        Ok(organization_members::RoleChange::LastOwner) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Cannot demote the last owner of the organization",
            )),
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

/// DELETE /api/organizations/:org_id/members/:user_id - Remove member (requires admin)
pub async fn remove_member(
    State(state): State<AppState>,
    admin: OrgAdmin,
    Path(path): Path<MemberPath>,
) -> impl IntoResponse {
    match organization_members::remove_guarded(state.db(), admin.org_id, path.user_id, admin.role)
        .await
    {
        Ok(organization_members::Removal::Removed) => {
            audit_member(&state, &admin, actions::MEMBER_REMOVED, path.user_id, None).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(organization_members::Removal::Forbidden) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Only owners can remove admins or owners",
            )),
        )
            .into_response(),
        Ok(organization_members::Removal::LastOwner) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Cannot remove the last owner of the organization",
            )),
        )
            .into_response(),
        Ok(organization_members::Removal::Missing) => (
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Weak;
    use std::time::Duration;

    use abnegate_secret::SecretValue;
    use abnegate_secret::encrypt_value;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use super::*;
    use crate::config::Config;
    use crate::db::projects;
    use crate::db::sync_config;
    use crate::services::mcp_servers::load;
    use crate::services::mcp_servers::organization::OrganizationHub;
    use crate::services::mcp_servers::testing::Tenant;
    use crate::services::mcp_servers::testing::stub;
    use crate::state::test_config;
    use crate::sync::hook::HOOK_ID;
    use crate::sync::hook::HOOK_SIGNED;

    const REPOSITORY: &str = "http://127.0.0.1/acme/project";
    const WEBHOOK: u64 = 7;
    const POLL: Duration = Duration::from_millis(20);
    const PATIENCE: Duration = Duration::from_secs(20);
    const NEVER: Duration = Duration::from_secs(3600);
    const HELD: &str = "SELECT pg_backend_pid() FROM organizations WHERE id = $1 FOR UPDATE";
    const BLOCKING: &str = "SELECT EXISTS(SELECT 1 FROM pg_stat_activity \
         WHERE wait_event_type = 'Lock' AND $1 = ANY(pg_blocking_pids(pid)))";

    fn owner(tenant: &Tenant) -> OrgOwner {
        OrgOwner {
            org_id: tenant.organization,
            user_id: tenant.user,
            email: tenant.email.clone(),
        }
    }

    /// How many servers the organization's hub connected, and the hub, held only weakly.
    async fn serving(state: &AppState, tenant: &Tenant) -> (usize, Weak<OrganizationHub>) {
        tenant.store(state, stub("stub")).await;
        let hub = load::organization(state, tenant.organization)
            .await
            .expect("the organization's hub");
        (hub.connected(), Arc::downgrade(&hub))
    }

    /// A GitHub sync in the tenant's workspace whose webhook Zone registered.
    async fn hooked(state: &AppState, tenant: &Tenant) {
        let project =
            projects::create_project(&tenant.pool, "Hooked", None, Some(tenant.workspace))
                .await
                .expect("a project");
        let token = encrypt_value(&SecretValue::new("ghp_hooks"), state.master_key())
            .expect("a sealed token");
        projects::link_github(&tenant.pool, project.id, REPOSITORY, Some(&token), None)
            .await
            .expect("the project is connected to GitHub");
        sync_config::create_sync_config(
            &tenant.pool,
            project.id,
            "github",
            true,
            json!({ "external_repo_url": REPOSITORY, HOOK_ID: WEBHOOK, HOOK_SIGNED: true }),
            None,
        )
        .await
        .expect("a sync with a registered webhook");
    }

    async fn eventually(mut met: impl AsyncFnMut() -> bool) -> bool {
        tokio::time::timeout(PATIENCE, async {
            while !met().await {
                tokio::time::sleep(POLL).await;
            }
        })
        .await
        .is_ok()
    }

    async fn removing(github: &MockServer) -> bool {
        github
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .any(|request| request.method.as_str() == "DELETE")
    }

    async fn blocking(tenant: &Tenant, holder: i32) -> bool {
        sqlx::query_scalar(BLOCKING)
            .bind(holder)
            .fetch_one(&tenant.pool)
            .await
            .expect("the sessions the holder blocks")
    }

    async fn stored(tenant: &Tenant) -> bool {
        organizations::get_organization(&tenant.pool, tenant.organization)
            .await
            .expect("the organization is read")
            .is_some()
    }

    #[tokio::test]
    async fn deleting_an_organization_stops_its_mcp_servers() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let (connected, held) = serving(&state, &tenant).await;

        let response = delete(State(state.clone()), owner(&tenant))
            .await
            .into_response();
        let forgotten = state
            .mcp()
            .existing_organization(tenant.organization)
            .is_none();
        tenant.remove().await;

        assert_eq!(connected, 1, "the stub server has to connect");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(forgotten, "the deleted organization kept its hub");
        assert!(
            held.upgrade().is_none(),
            "the deleted organization's servers kept running"
        );
    }

    #[tokio::test]
    async fn a_caller_that_leaves_while_a_webhook_is_removed_leaves_no_mcp_server_running() {
        let github = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!("/repos/acme/project/hooks/{WEBHOOK}")))
            .respond_with(ResponseTemplate::new(204).set_delay(NEVER))
            .mount(&github)
            .await;
        let tenant = Tenant::new().await;
        let state = tenant
            .configured(Config {
                github_api_url: github.uri(),
                github_api_allows_http: true,
                mcp_commands: true,
                ..test_config()
            })
            .await;
        hooked(&state, &tenant).await;
        let (connected, held) = serving(&state, &tenant).await;

        let reached = {
            let deleting = delete(State(state.clone()), owner(&tenant));
            tokio::pin!(deleting);
            tokio::select! {
                _ = &mut deleting => false,
                reached = eventually(async || removing(&github).await) => reached,
            }
        };
        let kept = stored(&tenant).await;
        let forgotten = state
            .mcp()
            .existing_organization(tenant.organization)
            .is_none();
        tenant.remove().await;

        assert_eq!(connected, 1, "the stub server has to connect");
        assert!(reached, "the delete never came to remove the webhook");
        assert!(!kept, "the organization was not deleted");
        assert!(forgotten, "the deleted organization kept its hub");
        assert!(
            held.upgrade().is_none(),
            "the deleted organization's servers kept running"
        );
    }

    #[tokio::test]
    async fn a_caller_that_leaves_before_the_delete_commits_leaves_no_mcp_server_running() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let (connected, held) = serving(&state, &tenant).await;
        let mut holding = tenant.pool.begin().await.expect("a transaction");
        let holder: i32 = sqlx::query_scalar(HELD)
            .bind(tenant.organization)
            .fetch_one(&mut *holding)
            .await
            .expect("the organization is held");

        let waited = {
            let deleting = delete(State(state.clone()), owner(&tenant));
            tokio::pin!(deleting);
            tokio::select! {
                _ = &mut deleting => false,
                waited = eventually(async || blocking(&tenant, holder).await) => waited,
            }
        };
        holding
            .commit()
            .await
            .expect("the organization is released");
        let deleted = eventually(async || !stored(&tenant).await).await;
        let forgotten = eventually(async || {
            state
                .mcp()
                .existing_organization(tenant.organization)
                .is_none()
        })
        .await;
        tenant.remove().await;

        assert_eq!(connected, 1, "the stub server has to connect");
        assert!(waited, "the delete never waited for the organization");
        assert!(deleted, "the organization was not deleted");
        assert!(forgotten, "the deleted organization kept its hub");
        assert!(
            held.upgrade().is_none(),
            "the deleted organization's servers kept running"
        );
    }
}
