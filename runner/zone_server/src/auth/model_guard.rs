//! Guards for routes that change the models installed on this server.
//!
//! Models live on the machine, not in an organization or workspace, so these
//! check global permissions rather than any membership role.

use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};

use super::middleware::{AuthError, AuthUser, require_permission};
use super::permission::Permission;
use crate::state::AppState;

/// Requires `models:create`: install, cancel an install, or train.
#[derive(Debug, Clone)]
pub struct ModelInstaller(pub AuthUser);

/// Requires `models:delete`: remove an installed model.
#[derive(Debug, Clone)]
pub struct ModelRemover(pub AuthUser);

async fn permitted<S>(
    parts: &mut Parts,
    state: &S,
    permission: Permission,
) -> Result<AuthUser, AuthError>
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    let user = AuthUser::from_request_parts(parts, state).await?;
    require_permission(&user.0, permission)?;
    Ok(user)
}

impl<S> FromRequestParts<S> for ModelInstaller
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        permitted(parts, state, Permission::ModelsCreate)
            .await
            .map(Self)
    }
}

impl<S> FromRequestParts<S> for ModelRemover
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        permitted(parts, state, Permission::ModelsDelete)
            .await
            .map(Self)
    }
}
