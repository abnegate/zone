use axum::extract::FromRef;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::middleware::AuthError;
use super::middleware::AuthUser;
use super::middleware::require_permission;
use super::permission::Permission;
use super::workspace_guard::WorkspaceWriter;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct PullRequestMerger(pub WorkspaceWriter);

#[derive(Debug, Clone)]
pub struct ReleaseCreator(pub WorkspaceWriter);

async fn permitted<S>(
    parts: &mut Parts,
    state: &S,
    permission: Permission,
) -> Result<WorkspaceWriter, AuthError>
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    let writer = WorkspaceWriter::from_request_parts(parts, state).await?;
    let user = AuthUser::from_request_parts(parts, state).await?;
    require_permission(&user.0, permission)?;
    Ok(writer)
}

impl<S> FromRequestParts<S> for PullRequestMerger
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        permitted(parts, state, Permission::PullRequestsUpdate)
            .await
            .map(Self)
    }
}

impl<S> FromRequestParts<S> for ReleaseCreator
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        permitted(parts, state, Permission::ReleasesCreate)
            .await
            .map(Self)
    }
}
