//! Which of a workspace's sources may act on a pull request, and with what.

use uuid::Uuid;

use crate::db::DbResult;
use crate::db::chat_pull_requests::{GITHUB, PullRequestRow};
use crate::db::sources::{self, SourceRow};
use crate::services::github_app::{self, Credential};
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
        && connected(&source)
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

/// What `source` authenticates with: a token minted for its GitHub App
/// installation, or else its stored personal access token. A source whose
/// installation was removed or suspended fails rather than falling back.
pub async fn credential(
    state: &AppState,
    source: &SourceRow,
) -> Result<Option<Credential>, github_app::Error> {
    state.github().source(state.db(), source).await
}

fn usable(source: &SourceRow) -> bool {
    source.source_type == GITHUB && source.is_active.unwrap_or(true)
}

fn connected(source: &SourceRow) -> bool {
    source.credentials_encrypted.is_some() || source.github_installation_id.is_some()
}
