//! Organization membership guards for authorization
//!
//! Provides extractors that verify organization membership and permissions.

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
    db::{
        custom_roles::Summary,
        organization_members::{self, OrgRole},
    },
    state::AppState,
};

const INACTIVE: &str = "Membership inactive";

fn database(error: sqlx::Error) -> AuthError {
    AuthError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("Database error: {error}"),
    }
}

/// The organization in the path and the signed-in user asking about it.
async fn identify<S>(
    parts: &mut Parts,
    state: &S,
    auth_user: &AuthUser,
) -> Result<(Uuid, Uuid), AuthError>
where
    S: Send + Sync,
{
    let organization_id = match Path::<Uuid>::from_request_parts(parts, state).await {
        Ok(Path(id)) => id,
        Err(_) => {
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

    Ok((organization_id, user_id))
}

/// An active member of the organization in the path, with what they hold.
#[derive(Debug, Clone)]
pub struct OrganizationMember {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub role: OrgRole,
    pub custom_role: Option<Summary>,
    pub grant: Grant,
}

impl OrganizationMember {
    pub fn allows(&self, permission: Permission) -> bool {
        self.grant.allows(permission)
    }
}

impl<S> FromRequestParts<S> for OrganizationMember
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state).await?;
        let app_state = AppState::from_ref(state);
        let (organization_id, user_id) = identify(parts, state, &auth_user).await?;

        let refusal = |message: String| AuthError {
            status: StatusCode::FORBIDDEN,
            message,
        };
        let membership = organization_members::membership(app_state.db(), organization_id, user_id)
            .await
            .map_err(database)?;
        let Some(membership) = membership else {
            let inactive =
                organization_members::get_member(app_state.db(), organization_id, user_id)
                    .await
                    .map_err(database)?
                    .is_some_and(|member| !member.is_active);
            return Err(refusal(if inactive {
                INACTIVE.to_string()
            } else {
                Permission::OrganizationRead.refusal()
            }));
        };
        if !membership.grant.allows(Permission::OrganizationRead) {
            return Err(refusal(Permission::OrganizationRead.refusal()));
        }

        Ok(Self {
            organization_id,
            user_id,
            email: auth_user.0.email,
            role: membership.role,
            custom_role: membership.custom_role,
            grant: membership.grant,
        })
    }
}

/// An organization member who holds the permission at index `PERMISSION`:
/// `OrganizationPermit<{ Permission::MembersCreate.index() }>`.
#[derive(Debug, Clone)]
pub struct OrganizationPermit<const PERMISSION: u16>(pub OrganizationMember);

impl<const PERMISSION: u16> OrganizationPermit<PERMISSION> {
    const PERMISSION: Permission = {
        let permission = Permission::at(PERMISSION);
        assert!(
            matches!(permission.scope(), Scope::Organization),
            "an organization permit needs an organization permission"
        );
        permission
    };
}

impl<S, const PERMISSION: u16> FromRequestParts<S> for OrganizationPermit<PERMISSION>
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let member = OrganizationMember::from_request_parts(parts, state).await?;
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
