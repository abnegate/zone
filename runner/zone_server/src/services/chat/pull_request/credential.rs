//! Which of a workspace's sources may act on a pull request, and with what.

use abnegate_secret::SecretValue;
use serde_json::Value;
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::chat_pull_requests::PullRequestRow;
use crate::db::sources::{self, SourceRow};
use crate::state::AppState;

const GITHUB: &str = "github";

/// More GitHub sources than any workspace connects to one repository.
const SOURCE_PAGE: i64 = 200;

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
    {
        return Ok(Some(source));
    }
    repository_source(state, workspace_id, &pull_request.owner, &pull_request.repo).await
}

/// The workspace's active GitHub source for `owner/repo`.
pub async fn repository_source(
    state: &AppState,
    workspace_id: Uuid,
    owner: &str,
    repo: &str,
) -> DbResult<Option<SourceRow>> {
    let candidates = sources::list_sources(
        state.db(),
        workspace_id,
        Some(GITHUB),
        Some(true),
        SOURCE_PAGE,
        0,
    )
    .await?;
    Ok(candidates.into_iter().find(|source| {
        usable(source)
            && named(&source.config, "owner", owner)
            && named(&source.config, "repo", repo)
    }))
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

fn named(config: &Value, key: &str, expected: &str) -> bool {
    config[key]
        .as_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}
