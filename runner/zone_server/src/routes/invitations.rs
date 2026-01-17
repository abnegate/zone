//! Invitation endpoints
//!
//! Routes for creating, accepting, and managing organization invitations.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AuthUser, OrgAdmin};
use crate::db::{invitations, organization_members, organizations};
use crate::state::AppState;

use super::common::Timestamps;

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
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
    expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organization_name: Option<String>,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl InvitationResponse {
    fn from_invitation(inv: invitations::Invitation, token: Option<String>) -> Self {
        Self {
            id: inv.id,
            email: inv.email,
            organization_id: inv.organization_id,
            workspace_ids: inv.workspace_ids,
            org_role: inv.org_role,
            workspace_role: inv.workspace_role,
            invited_by: inv.invited_by,
            expires_at: inv.expires_at.to_rfc3339(),
            token,
            organization_name: None,
            timestamps: Timestamps::from_utc(inv.created_at, inv.created_at),
        }
    }

    fn with_org_name(mut self, name: String) -> Self {
        self.organization_name = Some(name);
        self
    }
}

/// Create invitation request
#[derive(Debug, Deserialize)]
pub struct CreateInvitationRequest {
    email: String,
    workspace_ids: Vec<Uuid>,
    org_role: String,
    workspace_role: String,
}

/// Validates email format with proper checks
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
/// Requires: Organization Admin or Owner
pub async fn create_invitation(
    State(state): State<AppState>,
    OrgAdmin {
        org_id, user_id, ..
    }: OrgAdmin,
    Json(req): Json<CreateInvitationRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // Validate email format
    if !is_valid_email(&req.email) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid email address")),
        ));
    }

    // Validate roles
    let valid_org_roles = ["member", "admin", "owner"];
    let valid_workspace_roles = ["viewer", "member", "admin", "owner"];

    if !valid_org_roles.contains(&req.org_role.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Invalid org_role. Must be one of: {}",
                valid_org_roles.join(", ")
            ))),
        ));
    }

    if !valid_workspace_roles.contains(&req.workspace_role.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Invalid workspace_role. Must be one of: {}",
                valid_workspace_roles.join(", ")
            ))),
        ));
    }

    // Check if a user with this email already exists
    if let Ok(Some(existing_user)) =
        crate::db::users::get_user_by_email(state.db(), &req.email).await
    {
        // Check if this user is already a member of the organization
        let existing_member =
            organization_members::get_member(state.db(), org_id, existing_user.id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new(format!("Database error: {}", e))),
                    )
                })?;

        if let Some(member) = existing_member
            && member.is_active
        {
            return Err((
                StatusCode::CONFLICT,
                Json(ErrorResponse::new(
                    "User is already a member of this organization",
                )),
            ));
        }
    }

    // Check if there's already a pending invitation
    let existing_invitation =
        invitations::get_pending_invitation_for_email(state.db(), &req.email, org_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(format!("Database error: {}", e))),
                )
            })?;

    if existing_invitation.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse::new(
                "Pending invitation already exists for this email",
            )),
        ));
    }

    // Create invitation
    let (invitation, token) = invitations::create_invitation(
        state.db(),
        &req.email,
        org_id,
        req.workspace_ids,
        &req.org_role,
        &req.workspace_role,
        user_id,
    )
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") || e.to_string().contains("unique constraint") {
            (
                StatusCode::CONFLICT,
                Json(ErrorResponse::new("Invitation already exists")),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!(
                    "Failed to create invitation: {}",
                    e
                ))),
            )
        }
    })?;

    // Return invitation with token (for email sending)
    let response = InvitationResponse::from_invitation(invitation, Some(token));

    Ok((StatusCode::CREATED, Json(response)))
}

/// List pending invitations for an organization
///
/// GET /api/organizations/:org_id/invitations
///
/// Requires: Organization Admin or Owner
pub async fn list_invitations(
    State(state): State<AppState>,
    OrgAdmin { org_id, .. }: OrgAdmin,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let invitations = invitations::list_pending_invitations(state.db(), org_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!("Database error: {}", e))),
            )
        })?;

    let response: Vec<InvitationResponse> = invitations
        .into_iter()
        .map(|inv| InvitationResponse::from_invitation(inv, None))
        .collect();

    Ok(Json(response))
}

/// Revoke an invitation
///
/// DELETE /api/organizations/:org_id/invitations/:invitation_id
///
/// Requires: Organization Admin or Owner
pub async fn revoke_invitation(
    State(state): State<AppState>,
    OrgAdmin { org_id, .. }: OrgAdmin,
    Path((_org_id, invitation_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    // First get the invitation to verify org ownership
    let invitation = invitations::get_invitation_by_id(state.db(), invitation_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!("Database error: {}", e))),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Invitation not found")),
            )
        })?;

    // Verify invitation belongs to this organization
    if invitation.organization_id != org_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "Invitation does not belong to this organization",
            )),
        ));
    }

    // Now safe to delete
    invitations::revoke_invitation(state.db(), invitation_id)
        .await
        .map_err(|e| {
            if matches!(e, sqlx::Error::RowNotFound) {
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new("Invitation not found")),
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(format!("Database error: {}", e))),
                )
            }
        })?;

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
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let invitation = invitations::get_invitation_by_token(state.db(), &token)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!("Database error: {}", e))),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Invitation not found or expired")),
        ))?;

    // Get organization name
    let org = organizations::get_organization(state.db(), invitation.organization_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!("Database error: {}", e))),
            )
        })?
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Organization not found")),
        ))?;

    let response = InvitationResponse::from_invitation(invitation, None).with_org_name(org.name);

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
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse::new("Invalid user ID in token")),
        )
    })?;

    // Get invitation to verify it exists and get email
    let invitation = invitations::get_invitation_by_token(state.db(), &token)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!("Database error: {}", e))),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Invitation not found or expired")),
        ))?;

    // Verify the user's email matches the invitation email
    let user = crate::db::users::get_user_by_id(state.db(), user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(format!("Database error: {}", e))),
            )
        })?
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse::new("User not found")),
        ))?;

    if user.email != invitation.email {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "This invitation is for a different email address",
            )),
        ));
    }

    // Accept the invitation
    invitations::accept_invitation(state.db(), &token, user_id)
        .await
        .map_err(|e| {
            if matches!(e, sqlx::Error::RowNotFound) {
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new(
                        "Invitation not found or already accepted",
                    )),
                )
            } else if e.to_string().contains("duplicate key")
                || e.to_string().contains("unique constraint")
            {
                (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new(
                        "You are already a member of this organization",
                    )),
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(format!(
                        "Failed to accept invitation: {}",
                        e
                    ))),
                )
            }
        })?;

    Ok(Json(serde_json::json!({
        "message": "Invitation accepted successfully"
    })))
}
