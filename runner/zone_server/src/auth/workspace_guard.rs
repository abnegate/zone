//! Workspace membership guards for authorization
//!
//! Provides extractors that verify workspace membership and permissions.

use axum::{
    extract::{FromRef, FromRequestParts, Path},
    http::{StatusCode, request::Parts},
};
use uuid::Uuid;

use super::{
    grant::Grant,
    middleware::{AuthError, AuthUser},
    permission::Permission,
    scope::Scope,
};
use crate::{
    db::{custom_roles::Summary, workspace_members},
    state::AppState,
};

/// An active member of the workspace in the path, with what they hold.
/// `role` is the stored built-in column.
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub role: workspace_members::WorkspaceRole,
    pub custom_role: Option<Summary>,
    pub grant: Grant,
}

impl WorkspaceMember {
    pub fn allows(&self, permission: Permission) -> bool {
        self.grant.allows(permission)
    }
}

/// A workspace member who holds the permission at index `PERMISSION`:
/// `WorkspacePermit<{ Permission::WorkspaceWrite.index() }>`.
#[derive(Debug, Clone)]
pub struct WorkspacePermit<const PERMISSION: u16>(pub WorkspaceMember);

impl<const PERMISSION: u16> WorkspacePermit<PERMISSION> {
    const PERMISSION: Permission = {
        let permission = Permission::at(PERMISSION);
        assert!(
            matches!(permission.scope(), Scope::Workspace),
            "a workspace permit needs a workspace permission"
        );
        permission
    };
}

impl<S, const PERMISSION: u16> FromRequestParts<S> for WorkspacePermit<PERMISSION>
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let member = WorkspaceMember::from_request_parts(parts, state).await?;
        let permission = Self::PERMISSION;
        if !member.allows(permission) {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: permission.refusal(),
            });
        }
        Ok(Self(member))
    }
}

impl<S> FromRequestParts<S> for WorkspaceMember
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state).await?;

        let app_state = AppState::from_ref(state);

        let workspace_id = match Path::<Uuid>::from_request_parts(parts, state).await {
            Ok(Path(id)) => id,
            Err(_) => {
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

        let refusal = || AuthError {
            status: StatusCode::FORBIDDEN,
            message: Permission::WorkspaceRead.refusal(),
        };
        let membership = workspace_members::membership(app_state.db(), workspace_id, user_id)
            .await
            .map_err(|error| AuthError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Database error: {error}"),
            })?
            .ok_or_else(refusal)?;
        if !membership.grant.allows(Permission::WorkspaceRead) {
            return Err(refusal());
        }

        Ok(WorkspaceMember {
            workspace_id,
            user_id,
            email: auth_user.0.email,
            role: membership.role,
            custom_role: membership.custom_role,
            grant: membership.grant,
        })
    }
}
