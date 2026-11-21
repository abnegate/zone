//! Workspace and workspace member endpoints

use std::fmt::Display;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::Feature;

use crate::auth::{
    Grant, OrganizationMember, Permission, WorkspaceMember, WorkspacePermit, entitlement,
};
use crate::db::audit::{actions, resources};
use crate::db::custom_roles::{self, Assignment, Summary};
use crate::db::workspace_members::{self, RoleChange, WorkspaceRole};
use crate::db::workspaces;
use crate::state::AppState;
use crate::sync::hook::Removal;

use super::common::{AuditEvent, ErrorResponse, Timestamps, audit};

const WORKSPACE_NOT_FOUND: &str = "Workspace not found";
const MEMBER_NOT_FOUND: &str = "Member not found";
const ROLE_NOT_FOUND: &str = "Role not found";
const PROMOTION_REFUSED: &str = "Only workspace owners can promote users to admin or owner";
const EXCEEDS_REFUSAL: &str = "This role grants permissions you do not hold";

fn refusal(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorResponse::new(message))).into_response()
}

fn exceeds() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse::with_code(
            EXCEEDS_REFUSAL,
            custom_roles::EXCEEDS,
        )),
    )
        .into_response()
}

fn internal(error: impl Display) -> Response {
    tracing::error!(%error, "Database error");
    refusal(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
}

/// What a membership holds, as the API reports it: `role` is the highest
/// built-in role the grant fully covers, not the stored column.
#[derive(Debug, Serialize)]
struct Standing {
    role: &'static str,
    custom_role: Option<Summary>,
    permissions: Grant,
}

impl Standing {
    fn new(custom_role: Option<Summary>, grant: Grant) -> Self {
        Self {
            role: grant.workspace_rank().as_str(),
            custom_role,
            permissions: grant,
        }
    }

    fn built_in(role: WorkspaceRole) -> Self {
        Self::new(None, Grant::workspace(role))
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: bool,
    #[serde(flatten)]
    standing: Option<Standing>,
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
            standing: None,
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl From<workspace_members::UserWorkspace> for WorkspaceResponse {
    fn from(membership: workspace_members::UserWorkspace) -> Self {
        Self::from(membership.workspace)
            .held(Standing::new(membership.custom_role, membership.grant))
    }
}

impl WorkspaceResponse {
    fn held(mut self, standing: Standing) -> Self {
        self.standing = Some(standing);
        self
    }
}

#[derive(Debug, Serialize)]
struct SingleWorkspaceResponse {
    workspace: WorkspaceResponse,
}

#[derive(Debug, Serialize)]
struct WorkspacesListResponse {
    workspaces: Vec<WorkspaceResponse>,
}

/// GET /api/organizations/:org_id/workspaces - List the caller's workspaces
pub async fn list_accessible_workspaces(
    State(state): State<AppState>,
    member: OrganizationMember,
) -> Response {
    match workspace_members::list_user_workspaces_in_org(
        state.db(),
        member.user_id,
        member.organization_id,
    )
    .await
    {
        Ok(workspaces) => Json(WorkspacesListResponse {
            workspaces: workspaces
                .into_iter()
                .map(WorkspaceResponse::from)
                .collect(),
        })
        .into_response(),
        Err(error) => internal(error),
    }
}

/// GET /api/workspaces/:workspace_id
pub async fn get_workspace(State(state): State<AppState>, member: WorkspaceMember) -> Response {
    match workspaces::get_workspace(state.db(), member.workspace_id).await {
        Ok(Some(workspace)) => Json(SingleWorkspaceResponse {
            workspace: WorkspaceResponse::from(workspace)
                .held(Standing::new(member.custom_role, member.grant)),
        })
        .into_response(),
        Ok(None) => refusal(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND),
        Err(error) => internal(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// PATCH /api/workspaces/:workspace_id
pub async fn update_workspace(
    State(state): State<AppState>,
    WorkspacePermit(member): WorkspacePermit<{ Permission::WorkspaceUpdate.index() }>,
    Json(request): Json<UpdateWorkspaceRequest>,
) -> Response {
    match workspaces::update_workspace(
        state.db(),
        member.workspace_id,
        request.name.as_deref(),
        request.description.as_deref(),
        request.is_active,
    )
    .await
    {
        Ok(Some(workspace)) => {
            let response = WorkspaceResponse::from(workspace);
            audit(
                state.db(),
                AuditEvent {
                    organization_id: Some(response.organization_id),
                    workspace_id: Some(member.workspace_id),
                    actor_id: member.user_id,
                    actor_email: &member.email,
                    action: actions::WORKSPACE_UPDATED,
                    resource_type: resources::WORKSPACE,
                    resource_id: Some(member.workspace_id),
                    old_values: None,
                    new_values: serde_json::to_value(&response).ok(),
                },
            )
            .await;
            Json(SingleWorkspaceResponse {
                workspace: response,
            })
            .into_response()
        }
        Ok(None) => refusal(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND),
        Err(error) => internal(error),
    }
}

/// DELETE /api/workspaces/:workspace_id
///
/// Deleting the workspace unseats every member at once, so it needs
/// `workspace:delete`, which only built-in owners hold and no custom role can
/// carry.
pub async fn delete_workspace(
    State(state): State<AppState>,
    WorkspacePermit(member): WorkspacePermit<{ Permission::WorkspaceDelete.index() }>,
) -> Response {
    let organization_id = match workspaces::get_workspace(state.db(), member.workspace_id).await {
        Ok(workspace) => workspace.map(|workspace| workspace.organization_id),
        Err(error) => return internal(error),
    };

    let removal = match Removal::of_workspace(&state, member.workspace_id).await {
        Ok(removal) => removal,
        Err(error) => {
            tracing::error!(%error, "Failed to read the syncs whose webhooks the delete removes");
            return refusal(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };
    match workspaces::delete_workspace(state.db(), member.workspace_id).await {
        Ok(true) => {
            removal.run(&state).await;
            audit(
                state.db(),
                AuditEvent {
                    organization_id,
                    workspace_id: Some(member.workspace_id),
                    actor_id: member.user_id,
                    actor_email: &member.email,
                    action: actions::WORKSPACE_DELETED,
                    resource_type: resources::WORKSPACE,
                    resource_id: Some(member.workspace_id),
                    old_values: None,
                    new_values: None,
                },
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => refusal(StatusCode::NOT_FOUND, WORKSPACE_NOT_FOUND),
        Err(error) => internal(error),
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceMemberResponse {
    id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    #[serde(flatten)]
    standing: Standing,
    is_active: bool,
    invited_by: Option<Uuid>,
    email: Option<String>,
    display_name: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl WorkspaceMemberResponse {
    fn new(row: workspace_members::WorkspaceMemberRow, standing: Standing) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            user_id: row.user_id,
            standing,
            is_active: row.is_active,
            invited_by: row.invited_by,
            email: None,
            display_name: None,
            timestamps: Timestamps::from_naive(Some(row.created_at), Some(row.updated_at)),
        }
    }

    fn audited(&self) -> Value {
        json!({
            "user_id": self.user_id,
            "role": self.standing.role,
            "custom_role": self.standing.custom_role,
        })
    }
}

impl From<workspace_members::MemberWithUser> for WorkspaceMemberResponse {
    fn from(member: workspace_members::MemberWithUser) -> Self {
        Self {
            email: Some(member.email),
            display_name: member.display_name,
            ..Self::new(
                member.member,
                Standing::new(member.custom_role, member.grant),
            )
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AddWorkspaceMemberRequest {
    user_id: Uuid,
    role: String,
}

/// A built-in `role` or a `custom_role_id`, exactly one.
#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceMemberRoleRequest {
    role: Option<String>,
    custom_role_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct WorkspaceMembersListResponse {
    members: Vec<WorkspaceMemberResponse>,
}

/// GET /api/workspaces/:workspace_id/members
pub async fn list_members(State(state): State<AppState>, member: WorkspaceMember) -> Response {
    match workspace_members::list_members_with_users(state.db(), member.workspace_id).await {
        Ok(members) => Json(WorkspaceMembersListResponse {
            members: members
                .into_iter()
                .map(WorkspaceMemberResponse::from)
                .collect(),
        })
        .into_response(),
        Err(error) => internal(error),
    }
}

async fn audit_member(
    state: &AppState,
    actor: &WorkspaceMember,
    action: &str,
    user_id: Uuid,
    new_values: Option<Value>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: Some(actor.workspace_id),
            actor_id: actor.user_id,
            actor_email: &actor.email,
            action,
            resource_type: resources::MEMBER,
            resource_id: Some(user_id),
            old_values: None,
            new_values,
        },
    )
    .await;
}

/// Why a member route refused before or while seating a member.
#[derive(Debug)]
enum Refusal {
    Promotion,
    Exceeds,
    Seated,
    Vanished,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Promotion => refusal(StatusCode::FORBIDDEN, PROMOTION_REFUSED),
            Self::Exceeds => exceeds(),
            Self::Seated => refusal(
                StatusCode::CONFLICT,
                "User is already an active member of this workspace",
            ),
            Self::Vanished => refusal(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve the added member",
            ),
            Self::Database(error) => internal(error),
        }
    }
}

fn promotable(member: &WorkspaceMember, role: WorkspaceRole) -> Result<(), Refusal> {
    if role >= WorkspaceRole::Admin && !member.allows(Permission::WorkspaceAdministratorsUpdate) {
        return Err(Refusal::Promotion);
    }
    Ok(())
}

fn seatable(member: &WorkspaceMember, role: WorkspaceRole) -> Result<(), Refusal> {
    promotable(member, role)?;
    if !member.grant.covers(&Grant::workspace(role)) {
        return Err(Refusal::Exceeds);
    }
    Ok(())
}

/// POST /api/workspaces/:workspace_id/members
///
/// A direct add seats a built-in role only, and never one that grants more
/// than the caller holds.
pub async fn add_member(
    State(state): State<AppState>,
    WorkspacePermit(member): WorkspacePermit<{ Permission::WorkspaceMembersCreate.index() }>,
    Json(request): Json<AddWorkspaceMemberRequest>,
) -> Response {
    let Ok(role) = request.role.parse::<WorkspaceRole>() else {
        return refusal(
            StatusCode::BAD_REQUEST,
            "Invalid role. Must be one of: viewer, member, admin",
        );
    };
    let seated = match seatable(&member, role) {
        Ok(()) => seat(&state, &member, request.user_id, role).await,
        Err(refused) => Err(refused),
    };
    match seated {
        Ok((status, response)) => (status, Json(response)).into_response(),
        Err(refused) => refused.into_response(),
    }
}

/// Adds the user, or reactivates them if they were removed; an active member
/// is a conflict.
async fn seat(
    state: &AppState,
    member: &WorkspaceMember,
    user_id: Uuid,
    role: WorkspaceRole,
) -> Result<(StatusCode, WorkspaceMemberResponse), Refusal> {
    let status =
        match workspace_members::get_member(state.db(), member.workspace_id, user_id).await? {
            Some(existing) if existing.is_active => return Err(Refusal::Seated),
            Some(_) => {
                workspace_members::reactivate_member(
                    state.db(),
                    member.workspace_id,
                    user_id,
                    role,
                    Some(member.user_id),
                )
                .await?;
                StatusCode::OK
            }
            None => {
                workspace_members::add_member(
                    state.db(),
                    member.workspace_id,
                    user_id,
                    role,
                    Some(member.user_id),
                )
                .await?;
                StatusCode::CREATED
            }
        };

    let row = workspace_members::get_member(state.db(), member.workspace_id, user_id)
        .await?
        .ok_or(Refusal::Vanished)?;
    let response = WorkspaceMemberResponse::new(row, Standing::built_in(role));
    audit_member(
        state,
        member,
        actions::MEMBER_ADDED,
        user_id,
        Some(response.audited()),
    )
    .await;
    Ok((status, response))
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceMemberPath {
    workspace_id: Uuid,
    user_id: Uuid,
}

/// PATCH /api/workspaces/:workspace_id/members/:user_id
///
/// Assigning a custom role needs the Enterprise license; assigning a built-in
/// role never does.
pub async fn update_member_role(
    State(state): State<AppState>,
    WorkspacePermit(member): WorkspacePermit<{ Permission::WorkspaceMembersUpdate.index() }>,
    Path(path): Path<WorkspaceMemberPath>,
    Json(request): Json<UpdateWorkspaceMemberRoleRequest>,
) -> Response {
    let assignment =
        match Assignment::<WorkspaceRole>::parse(request.role.as_deref(), request.custom_role_id) {
            Ok(assignment) => assignment,
            Err(message) => return refusal(StatusCode::BAD_REQUEST, message),
        };
    match assignment {
        Assignment::BuiltIn(role) => {
            if let Err(refused) = promotable(&member, role) {
                return refused.into_response();
            }
        }
        Assignment::Custom(_) => {
            if let Err(unentitled) = entitlement::require(&state, Feature::CustomRoles).await {
                return unentitled.into_response();
            }
        }
    }

    let change = match workspace_members::change_role(
        state.db(),
        member.workspace_id,
        path.user_id,
        assignment,
        &member.grant,
    )
    .await
    {
        Ok(change) => change,
        Err(error) => return internal(error),
    };
    let row = match change {
        RoleChange::Applied(row) => *row,
        RoleChange::Forbidden => {
            return refusal(
                StatusCode::FORBIDDEN,
                "Only workspace owners can change the role of an admin or owner",
            );
        }
        RoleChange::Missing => return refusal(StatusCode::NOT_FOUND, MEMBER_NOT_FOUND),
        RoleChange::LastOwner => {
            return refusal(
                StatusCode::FORBIDDEN,
                "Cannot demote the last owner of the workspace",
            );
        }
        RoleChange::LastAdmin => {
            return refusal(
                StatusCode::FORBIDDEN,
                "Cannot demote the last admin of the workspace",
            );
        }
        RoleChange::Exceeds => return exceeds(),
        RoleChange::RoleMissing => return refusal(StatusCode::NOT_FOUND, ROLE_NOT_FOUND),
    };

    let standing = match assignment {
        Assignment::BuiltIn(role) => Standing::built_in(role),
        Assignment::Custom(_) => {
            match workspace_members::membership(state.db(), member.workspace_id, path.user_id).await
            {
                Ok(Some(held)) => Standing::new(held.custom_role, held.grant),
                Ok(None) => Standing::built_in(row.role),
                Err(error) => return internal(error),
            }
        }
    };
    let response = WorkspaceMemberResponse::new(row, standing);
    audit_member(
        &state,
        &member,
        actions::MEMBER_ROLE_CHANGED,
        response.user_id,
        Some(response.audited()),
    )
    .await;
    Json(response).into_response()
}

/// DELETE /api/workspaces/:workspace_id/members/:user_id
pub async fn remove_member(
    State(state): State<AppState>,
    WorkspacePermit(member): WorkspacePermit<{ Permission::WorkspaceMembersDelete.index() }>,
    Path(path): Path<WorkspaceMemberPath>,
) -> Response {
    match workspace_members::remove_guarded(
        state.db(),
        member.workspace_id,
        path.user_id,
        &member.grant,
    )
    .await
    {
        Ok(workspace_members::Removal::Removed) => {
            audit_member(&state, &member, actions::MEMBER_REMOVED, path.user_id, None).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(workspace_members::Removal::Forbidden) => refusal(
            StatusCode::FORBIDDEN,
            "Only workspace owners can remove admins or owners",
        ),
        Ok(workspace_members::Removal::LastOwner) => refusal(
            StatusCode::FORBIDDEN,
            "Cannot remove the last owner of the workspace",
        ),
        Ok(workspace_members::Removal::LastAdmin) => refusal(
            StatusCode::FORBIDDEN,
            "Cannot remove the last admin from the workspace",
        ),
        Ok(workspace_members::Removal::Missing) => refusal(StatusCode::NOT_FOUND, MEMBER_NOT_FOUND),
        Err(error) => internal(error),
    }
}

#[cfg(test)]
mod tests;
