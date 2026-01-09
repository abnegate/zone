//! Workspace membership guards for authorization
//!
//! Provides extractors that verify workspace membership and roles.

use axum::{
    extract::{FromRef, FromRequestParts, Path},
    http::{StatusCode, request::Parts},
};
use uuid::Uuid;

use super::middleware::{AuthError, AuthUser};
use crate::{db::workspace_members, state::AppState};

/// Guard that requires workspace membership (any role, viewer or higher)
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: workspace_members::WorkspaceRole,
}

impl<S> FromRequestParts<S> for WorkspaceMember
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // First require authentication
        let auth_user = AuthUser::from_request_parts(parts, state).await?;

        // Get app state to access database pool
        let app_state = AppState::from_ref(state);

        // Extract workspace_id from path using Axum's Path extractor
        // Try to extract as a single UUID first (e.g., /api/workspaces/:workspace_id or /api/workspaces/:id)
        let workspace_id = match Path::<Uuid>::from_request_parts(parts, state).await {
            Ok(Path(id)) => id,
            Err(_) => {
                // If that fails, try extracting as a named parameter map
                #[derive(serde::Deserialize)]
                struct WorkspacePath {
                    #[serde(alias = "id")]
                    workspace_id: Uuid,
                }

                match Path::<WorkspacePath>::from_request_parts(parts, state).await {
                    Ok(Path(path)) => path.workspace_id,
                    Err(_) => {
                        return Err(AuthError {
                            status: StatusCode::BAD_REQUEST,
                            message: "Missing workspace_id in path".to_string(),
                        });
                    }
                }
            }
        };

        let user_id = Uuid::parse_str(&auth_user.0.sub).map_err(|_| AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid user ID in token".to_string(),
        })?;

        // Check membership
        let member = workspace_members::get_member(app_state.db(), workspace_id, user_id)
            .await
            .map_err(|e| AuthError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Database error: {}", e),
            })?
            .ok_or(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Not a member of this workspace".to_string(),
            })?;

        // Check if member is active
        if !member.is_active {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Workspace membership is inactive".to_string(),
            });
        }

        Ok(WorkspaceMember {
            workspace_id,
            user_id,
            role: member.role,
        })
    }
}

/// Guard that requires workspace writer role (member or higher)
#[derive(Debug, Clone)]
pub struct WorkspaceWriter {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: workspace_members::WorkspaceRole,
}

impl<S> FromRequestParts<S> for WorkspaceWriter
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Get workspace member first
        let member = WorkspaceMember::from_request_parts(parts, state).await?;

        // Check if user can write (member or higher)
        if member.role < workspace_members::WorkspaceRole::Member {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Workspace write access required".to_string(),
            });
        }

        Ok(WorkspaceWriter {
            workspace_id: member.workspace_id,
            user_id: member.user_id,
            role: member.role,
        })
    }
}

/// Guard that requires workspace admin role
#[derive(Debug, Clone)]
pub struct WorkspaceAdmin {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: workspace_members::WorkspaceRole,
}

impl<S> FromRequestParts<S> for WorkspaceAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Get workspace member first
        let member = WorkspaceMember::from_request_parts(parts, state).await?;

        // Check if user is admin or owner
        if member.role < workspace_members::WorkspaceRole::Admin {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Workspace admin access required".to_string(),
            });
        }

        Ok(WorkspaceAdmin {
            workspace_id: member.workspace_id,
            user_id: member.user_id,
            role: member.role,
        })
    }
}
