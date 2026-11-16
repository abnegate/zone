//! Guard for routes that act on the whole server rather than a tenant.
//!
//! The admin flag in an access token stays as it was when the token was
//! issued, so the guard reads the user's row on every request: an admin who is
//! demoted or deactivated loses access at once, not when their token refreshes.

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, request::Parts},
};
use sqlx::PgPool;
use uuid::Uuid;

use super::middleware::{AuthError, AuthUser};
use crate::db::users::{self, UserRow};
use crate::state::AppState;

pub const REQUIRED: &str = "This needs a server admin.";

const UNCHECKED: &str = "Could not check server admin access";

#[derive(Debug, Clone)]
pub struct ServerAdmin(pub UserRow);

fn permits(user: &UserRow) -> bool {
    user.is_admin == Some(true) && user.is_active != Some(false)
}

fn refused() -> AuthError {
    AuthError {
        status: StatusCode::FORBIDDEN,
        message: REQUIRED.to_string(),
    }
}

impl<S> FromRequestParts<S> for ServerAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;
        let id = claims.user_id().map_err(|_| refused())?;
        admin(AppState::from_ref(state).db(), id).await.map(Self)
    }
}

async fn admin(pool: &PgPool, id: Uuid) -> Result<UserRow, AuthError> {
    users::get_user_by_id(pool, id)
        .await
        .map_err(|error| {
            tracing::error!(%error, user_id = %id, "Failed to load the user for a server admin check");
            AuthError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: UNCHECKED.to_string(),
            }
        })?
        .filter(permits)
        .ok_or_else(refused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::unreachable_pool;

    fn user(is_admin: Option<bool>, is_active: Option<bool>) -> UserRow {
        UserRow {
            id: Uuid::new_v4(),
            email: "admin@example.com".into(),
            display_name: None,
            is_active,
            is_admin,
            email_verified: true,
            created_at: None,
            updated_at: None,
            last_login_at: None,
        }
    }

    #[test]
    fn an_active_admin_is_permitted() {
        assert!(permits(&user(Some(true), Some(true))));
    }

    #[test]
    fn an_admin_with_no_recorded_activity_flag_is_permitted() {
        assert!(permits(&user(Some(true), None)));
    }

    #[test]
    fn a_deactivated_admin_is_refused() {
        assert!(!permits(&user(Some(true), Some(false))));
    }

    #[test]
    fn a_user_who_is_not_an_admin_is_refused() {
        assert!(!permits(&user(Some(false), Some(true))));
        assert!(!permits(&user(None, Some(true))));
    }

    #[test]
    fn a_refusal_is_forbidden_and_names_the_requirement() {
        let error = refused();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert_eq!(error.message, REQUIRED);
    }

    #[tokio::test]
    async fn a_database_failure_is_a_server_error_rather_than_a_refusal() {
        let error = admin(&unreachable_pool(), Uuid::new_v4())
            .await
            .unwrap_err();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.message, UNCHECKED);
    }
}
