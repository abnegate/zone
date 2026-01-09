//! Organization membership guards for authorization
//!
//! Provides extractors that verify organization membership and roles.

use axum::{
    extract::{FromRef, FromRequestParts, Path},
    http::{StatusCode, request::Parts},
};
use uuid::Uuid;

use super::middleware::{AuthError, AuthUser};
use crate::{db::organization_members, state::AppState};

/// Guard that requires organization membership
#[derive(Debug, Clone)]
pub struct OrgMember {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub role: organization_members::OrgRole,
}

impl<S> FromRequestParts<S> for OrgMember
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

        // Extract org_id from path using Axum's Path extractor
        // Try to extract as a single UUID first (e.g., /api/organizations/:org_id)
        let org_id = match Path::<Uuid>::from_request_parts(parts, state).await {
            Ok(Path(id)) => id,
            Err(_) => {
                // If that fails, try extracting as a named parameter map
                #[derive(serde::Deserialize)]
                struct OrgPath {
                    #[serde(alias = "organization_id")]
                    org_id: Uuid,
                }

                match Path::<OrgPath>::from_request_parts(parts, state).await {
                    Ok(Path(path)) => path.org_id,
                    Err(_) => {
                        return Err(AuthError {
                            status: StatusCode::BAD_REQUEST,
                            message: "Missing org_id in path".to_string(),
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
        let member = organization_members::get_member(app_state.db(), org_id, user_id)
            .await
            .map_err(|e| AuthError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Database error: {}", e),
            })?
            .ok_or(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Not a member of this organization".to_string(),
            })?;

        // CRITICAL-5: Check if membership is active
        if !member.is_active {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Membership inactive".to_string(),
            });
        }

        Ok(OrgMember {
            org_id,
            user_id,
            role: member.role,
        })
    }
}

/// Guard that requires organization admin role
#[derive(Debug, Clone)]
pub struct OrgAdmin {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub role: organization_members::OrgRole,
}

impl<S> FromRequestParts<S> for OrgAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Get organization member first
        let member = OrgMember::from_request_parts(parts, state).await?;

        // Check if user is admin or owner
        if member.role < organization_members::OrgRole::Admin {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Organization admin access required".to_string(),
            });
        }

        Ok(OrgAdmin {
            org_id: member.org_id,
            user_id: member.user_id,
            role: member.role,
        })
    }
}

/// Guard that requires organization owner role
#[derive(Debug, Clone)]
pub struct OrgOwner {
    pub org_id: Uuid,
    pub user_id: Uuid,
}

impl<S> FromRequestParts<S> for OrgOwner
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Get organization member first
        let member = OrgMember::from_request_parts(parts, state).await?;

        // Check if user is owner
        if member.role != organization_members::OrgRole::Owner {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "Organization owner access required".to_string(),
            });
        }

        Ok(OrgOwner {
            org_id: member.org_id,
            user_id: member.user_id,
        })
    }
}
