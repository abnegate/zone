//! Which of a workspace's sources may act on a pull request, and with what.

use abnegate_secret::SecretValue;
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::chat_pull_requests::{GITHUB, PullRequestRow};
use crate::db::sources::{self, SourceRow};
use crate::state::AppState;

/// The source the pull request was opened through, or else the workspace's
/// active GitHub source for its repository.
pub async fn source(
    state: &AppState,
    workspace_id: Uuid,
    pull_request: &PullRequestRow,
) -> DbResult<Option<SourceRow>> {
    if let Some(id) = pull_request.source_id
        && let Some(source) = sources::get_source(state.db(), id, workspace_id).await?
        && usable(&source)
        && source.credentials_encrypted.is_some()
    {
        return Ok(Some(source));
    }
    repository_source(state, workspace_id, &pull_request.owner, &pull_request.repo).await
}

pub async fn repository_source(
    state: &AppState,
    workspace_id: Uuid,
    owner: &str,
    repo: &str,
) -> DbResult<Option<SourceRow>> {
    sources::get_repository_source(state.db(), workspace_id, GITHUB, owner, repo).await
}

/// The token `source` authenticates with. A source without one, or one whose
/// stored credential cannot be opened, cannot act.
pub fn token(state: &AppState, source: &SourceRow) -> Option<SecretValue> {
    let encrypted = source.credentials_encrypted.as_deref()?;
    match crate::crypto::decrypt(state.encryption_key(), encrypted) {
        Ok(token) => Some(SecretValue::new(token)),
        Err(_) => {
            tracing::warn!(source = %source.id, "a GitHub source's credential could not be decrypted");
            None
        }
    }
}

fn usable(source: &SourceRow) -> bool {
    source.source_type == GITHUB && source.is_active.unwrap_or(true)
}
