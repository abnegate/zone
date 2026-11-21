//! Invitation endpoints
//!
//! Routes for creating, accepting, and managing organization invitations.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AuthUser, Grant, OrganizationPermit, Permission};
use crate::db::audit::{actions, resources};
use crate::db::custom_roles;
use crate::db::organization_members::OrgRole;
use crate::db::workspace_members::WorkspaceRole;
use crate::db::{invitations, organization_members, organizations, users, workspaces};
use crate::state::AppState;

use super::common::{AuditEvent, ErrorResponse, Timestamps, audit};

const INVALID_ORGANIZATION_ROLE: &str = "Invalid org_role. Must be one of: member, admin, owner";
const INVALID_WORKSPACE_ROLE: &str =
    "Invalid workspace_role. Must be one of: viewer, member, admin, owner";

type Refusal = (StatusCode, Json<ErrorResponse>);

fn refusal(status: StatusCode, message: impl Into<String>) -> Refusal {
    (status, Json(ErrorResponse::new(message)))
}

fn database(error: sqlx::Error) -> Refusal {
    refusal(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Database error: {error}"),
    )
}

/// Invitation response
#[derive(Debug, Serialize)]
pub struct InvitationResponse {
    id: Uuid,
    email: String,
    organization_id: Uuid,
    workspace_ids: Vec<Uuid>,
    org_role: String,
    workspace_role: String,
    invited_by: Uuid,
    expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organization_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invited_by_email: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl InvitationResponse {
    fn from_invitation(invitation: invitations::Invitation, token: Option<String>) -> Self {
        Self {
            id: invitation.id,
            email: invitation.email,
            organization_id: invitation.organization_id,
            workspace_ids: invitation.workspace_ids,
            org_role: invitation.org_role,
            workspace_role: invitation.workspace_role,
            invited_by: invitation.invited_by,
            expires_at: invitation.expires_at,
            token,
            organization_name: None,
            workspace_name: None,
            invited_by_email: None,
            timestamps: Timestamps::from_utc(invitation.created_at, invitation.created_at),
        }
    }

    fn with_org_name(mut self, name: String) -> Self {
        self.organization_name = Some(name);
        self
    }

    /// Name the first workspace the invitation seats and the inviter, the
    /// details the acceptance page shows before the invitee signs in.
    async fn with_details(mut self, state: &AppState) -> Self {
        if let Some(workspace_id) = self.workspace_ids.first() {
            match workspaces::get_workspace(state.db(), *workspace_id).await {
                Ok(workspace) => self.workspace_name = workspace.map(|workspace| workspace.name),
                Err(error) => {
                    tracing::warn!(%error, %workspace_id, "Failed to name the invited workspace")
                }
            }
        }
        match users::get_user_by_id(state.db(), self.invited_by).await {
            Ok(inviter) => self.invited_by_email = inviter.map(|inviter| inviter.email),
            Err(error) => {
                tracing::warn!(%error, inviter = %self.invited_by, "Failed to name the inviter")
            }
        }
        self
    }
}

/// Create invitation request. The console's form names one workspace, the
/// API's own callers name a list, so either identifies the workspaces and at
/// most one form may be given. Naming none invites into the organization alone.
#[derive(Debug, Deserialize)]
pub struct CreateInvitationRequest {
    email: String,
    #[serde(default)]
    workspace_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    workspace_id: Option<Uuid>,
    org_role: String,
    #[serde(default = "default_workspace_role")]
    workspace_role: String,
}

fn default_workspace_role() -> String {
    WorkspaceRole::Member.as_str().to_string()
}

fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let local = parts[0];
    let domain = parts[1];

    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

/// Create a new invitation
///
/// POST /api/organizations/:org_id/invitations
///
/// Requires: invitations:create, plus administrators:update to invite an
/// organization owner or a workspace admin or owner
pub async fn create_invitation(
    State(state): State<AppState>,
    OrganizationPermit(inviter): OrganizationPermit<{ Permission::InvitationsCreate.index() }>,
    Json(request): Json<CreateInvitationRequest>,
) -> Result<impl IntoResponse, Refusal> {
    let organization_id = inviter.organization_id;
    if !is_valid_email(&request.email) {
        return Err(refusal(StatusCode::BAD_REQUEST, "Invalid email address"));
    }

    let workspace_ids = match (request.workspace_ids, request.workspace_id) {
        (Some(ids), None) => ids,
        (None, Some(id)) => vec![id],
        (None, None) => Vec::new(),
        (Some(_), Some(_)) => {
            return Err(refusal(
                StatusCode::BAD_REQUEST,
                "Name the workspaces by either workspace_id or workspace_ids",
            ));
        }
    };

    let organization_role: OrgRole = request
        .org_role
        .parse()
        .map_err(|_| refusal(StatusCode::BAD_REQUEST, INVALID_ORGANIZATION_ROLE))?;
    let workspace_role: WorkspaceRole = request
        .workspace_role
        .parse()
        .map_err(|_| refusal(StatusCode::BAD_REQUEST, INVALID_WORKSPACE_ROLE))?;

    if (organization_role == OrgRole::Owner || workspace_role >= WorkspaceRole::Admin)
        && !inviter.allows(Permission::AdministratorsUpdate)
    {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "Only owners can invite owners or workspace admins",
        ));
    }
    if !inviter
        .grant
        .covers(&Grant::organization(organization_role))
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::with_code(
                custom_roles::EXCEEDS_MESSAGE,
                custom_roles::EXCEEDS,
            )),
        ));
    }

    for workspace_id in &workspace_ids {
        let workspace = workspaces::get_workspace(state.db(), *workspace_id)
            .await
            .map_err(database)?;

        if workspace.is_none_or(|workspace| workspace.organization_id != organization_id) {
            return Err(refusal(
                StatusCode::BAD_REQUEST,
                "Workspace does not belong to this organization",
            ));
        }
    }

    if let Ok(Some(existing_user)) = users::get_user_by_email(state.db(), &request.email).await {
        let existing_member =
            organization_members::get_member(state.db(), organization_id, existing_user.id)
                .await
                .map_err(database)?;

        if existing_member.is_some_and(|member| member.is_active) {
            return Err(refusal(
                StatusCode::CONFLICT,
                "User is already a member of this organization",
            ));
        }
    }

    let existing_invitation =
        invitations::get_pending_invitation_for_email(state.db(), &request.email, organization_id)
            .await
            .map_err(database)?;

    if existing_invitation.is_some() {
        return Err(refusal(
            StatusCode::CONFLICT,
            "Pending invitation already exists for this email",
        ));
    }

    invitations::delete_expired_invitations_for_email(state.db(), &request.email, organization_id)
        .await
        .map_err(database)?;

    let (invitation, token) = invitations::create_invitation(
        state.db(),
        &request.email,
        organization_id,
        workspace_ids,
        organization_role.as_str(),
        workspace_role.as_str(),
        inviter.user_id,
    )
    .await
    .map_err(|error| {
        let message = error.to_string();
        if message.contains("duplicate key") || message.contains("unique constraint") {
            refusal(StatusCode::CONFLICT, "Invitation already exists")
        } else {
            refusal(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create invitation: {message}"),
            )
        }
    })?;

    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(organization_id),
            workspace_id: None,
            actor_id: inviter.user_id,
            actor_email: &inviter.email,
            action: actions::INVITATION_SENT,
            resource_type: resources::INVITATION,
            resource_id: Some(invitation.id),
            old_values: None,
            new_values: Some(serde_json::json!({
                "email": invitation.email,
                "org_role": invitation.org_role,
                "workspace_ids": invitation.workspace_ids,
                "workspace_role": invitation.workspace_role,
            })),
        },
    )
    .await;

    let response = InvitationResponse::from_invitation(invitation, Some(token));

    Ok((StatusCode::CREATED, Json(response)))
}

/// List pending invitations for an organization
///
/// GET /api/organizations/:org_id/invitations
///
/// Requires: invitations:read
pub async fn list_invitations(
    State(state): State<AppState>,
    OrganizationPermit(reader): OrganizationPermit<{ Permission::InvitationsRead.index() }>,
) -> Result<impl IntoResponse, Refusal> {
    let invitations = invitations::list_pending_invitations(state.db(), reader.organization_id)
        .await
        .map_err(database)?;

    let response: Vec<InvitationResponse> = invitations
        .into_iter()
        .map(|invitation| InvitationResponse::from_invitation(invitation, None))
        .collect();

    Ok(Json(response))
}

/// Revoke an invitation
///
/// DELETE /api/organizations/:org_id/invitations/:invitation_id
///
/// Requires: invitations:delete
pub async fn revoke_invitation(
    State(state): State<AppState>,
    OrganizationPermit(revoker): OrganizationPermit<{ Permission::InvitationsDelete.index() }>,
    Path((_organization_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, Refusal> {
    let invitation = invitations::get_invitation_by_id(state.db(), invitation_id)
        .await
        .map_err(database)?
        .ok_or_else(|| refusal(StatusCode::NOT_FOUND, "Invitation not found"))?;

    if invitation.organization_id != revoker.organization_id {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "Invitation does not belong to this organization",
        ));
    }

    invitations::revoke_invitation(state.db(), invitation_id)
        .await
        .map_err(|error| {
            if matches!(error, sqlx::Error::RowNotFound) {
                refusal(StatusCode::NOT_FOUND, "Invitation not found")
            } else {
                database(error)
            }
        })?;

    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(revoker.organization_id),
            workspace_id: None,
            actor_id: revoker.user_id,
            actor_email: &revoker.email,
            action: actions::INVITATION_REVOKED,
            resource_type: resources::INVITATION,
            resource_id: Some(invitation_id),
            old_values: Some(serde_json::json!({ "email": invitation.email })),
            new_values: None,
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// Get invitation details by token
///
/// GET /api/invitations/:token
///
/// Public route - no authentication required
pub async fn get_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, Refusal> {
    let invitation = invitations::get_invitation_by_token(state.db(), &token)
        .await
        .map_err(database)?
        .ok_or_else(|| refusal(StatusCode::NOT_FOUND, "Invitation not found or expired"))?;

    let organization = organizations::get_organization(state.db(), invitation.organization_id)
        .await
        .map_err(database)?
        .ok_or_else(|| refusal(StatusCode::INTERNAL_SERVER_ERROR, "Organization not found"))?;

    let response = InvitationResponse::from_invitation(invitation, None)
        .with_org_name(organization.name)
        .with_details(&state)
        .await;

    Ok(Json(response))
}

/// Accept an invitation
///
/// POST /api/invitations/:token/accept
///
/// Requires: Authentication (user must be logged in)
pub async fn accept_invitation(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, Refusal> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| refusal(StatusCode::UNAUTHORIZED, "Invalid user ID in token"))?;

    let invitation = invitations::get_invitation_by_token(state.db(), &token)
        .await
        .map_err(database)?
        .ok_or_else(|| refusal(StatusCode::NOT_FOUND, "Invitation not found or expired"))?;

    let user = users::get_user_by_id(state.db(), user_id)
        .await
        .map_err(database)?
        .ok_or_else(|| refusal(StatusCode::UNAUTHORIZED, "User not found"))?;

    if user.email != invitation.email {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "This invitation is for a different email address",
        ));
    }

    invitations::accept_invitation(state.db(), &token, user_id)
        .await
        .map_err(|error| {
            let message = error.to_string();
            if matches!(error, sqlx::Error::RowNotFound) {
                refusal(
                    StatusCode::NOT_FOUND,
                    "Invitation not found or already accepted",
                )
            } else if message.contains("duplicate key") || message.contains("unique constraint") {
                refusal(
                    StatusCode::CONFLICT,
                    "You are already a member of this organization",
                )
            } else {
                refusal(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to accept invitation: {message}"),
                )
            }
        })?;

    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(invitation.organization_id),
            workspace_id: None,
            actor_id: user_id,
            actor_email: &user.email,
            action: actions::INVITATION_ACCEPTED,
            resource_type: resources::INVITATION,
            resource_id: Some(invitation.id),
            old_values: None,
            new_values: Some(serde_json::json!({
                "email": invitation.email,
                "org_role": invitation.org_role,
                "workspace_ids": invitation.workspace_ids,
            })),
        },
    )
    .await;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Invitation accepted successfully"
    })))
}
