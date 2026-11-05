//! The pull requests a chat opened: noticing them as its turns open them, and
//! reading and changing them on GitHub through abnegate-vcs with the
//! workspace's own source.

mod auto_merge;
mod cache;
mod changes;
mod checks;
mod credential;
mod detection;
mod error;
mod mergeable;
mod open;
mod opened;
mod push;
mod recorder;
mod state;
mod status;
mod tracked;

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{Excerpt, PullRequestState};
use abnegate_vcs::{PullRequestReference, PullRequestService};
use futures::{StreamExt, stream};
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::chat_pull_requests::{self, PullRequestRow};
use crate::state::AppState;

pub use auto_merge::AutoMerge;
pub use cache::StatusCache;
pub use changes::PENDING_RELEASE;
pub use checks::Checks;
pub use error::Error;
pub use mergeable::Mergeable;
pub use opened::Opened;
pub use push::push;
pub use recorder::{Recorder, record};
pub use state::State;
pub use status::Status;
pub use tracked::Tracked;

use open::Open;

/// The most of a pull request's diff read for one view.
const DIFF_BYTES: usize = 1024 * 1024;

/// How many pull requests are read from GitHub at once.
const CONCURRENT_READS: usize = 4;

/// The pull requests `chat_id` opened and still shows, with their status.
pub async fn list(state: &AppState, workspace_id: Uuid, chat_id: Uuid) -> DbResult<Vec<Tracked>> {
    let rows = chat_pull_requests::list(state.db(), chat_id).await?;
    stream::iter(rows)
        .map(|row| track(state, workspace_id, row))
        .buffered(CONCURRENT_READS)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect()
}

/// `row` with what GitHub last said of it, asking again once that is stale.
/// A fresh status was read with a credential, so only a stale one costs a
/// credential lookup.
pub async fn track(state: &AppState, workspace_id: Uuid, row: PullRequestRow) -> DbResult<Tracked> {
    if let Some(cached) = state.pull_request_statuses().get(row.id) {
        return Ok(Tracked {
            row,
            status: Some(cached),
            credentialed: true,
        });
    }
    let Some(token) = token(state, workspace_id, &row).await? else {
        return Ok(Tracked {
            row,
            status: None,
            credentialed: false,
        });
    };
    let status = match observed(state, &row, &token).await {
        Ok(status) => {
            remember(state, &row, &status).await;
            Some(status)
        }
        Err(error) => {
            tracing::warn!(pull_request = %row.id, %error, "could not read a chat's pull request from GitHub");
            None
        }
    };
    Ok(Tracked {
        row,
        status,
        credentialed: true,
    })
}

/// Makes the pull request a draft, or marks it ready for review.
pub async fn set_draft(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
    draft: bool,
) -> Result<(), Error> {
    let Open {
        service,
        reference,
        token,
        status,
    } = open(state, workspace_id, row).await?;
    if status.draft != draft {
        changes::set_draft(&service, &reference, &token, draft).await?;
    }
    state.pull_request_statuses().forget(row.id);
    Ok(())
}

/// Turns GitHub's auto-merge on or off for the pull request.
pub async fn set_auto_merge(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
    enabled: bool,
) -> Result<(), Error> {
    let Open {
        service,
        reference,
        token,
        status,
    } = open(state, workspace_id, row).await?;
    if status.auto_merge.enabled != enabled {
        changes::set_auto_merge(&service, &reference, &token, enabled).await?;
    }
    state.pull_request_statuses().forget(row.id);
    Ok(())
}

/// The start of the pull request's unified diff.
pub async fn diff(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> Result<Excerpt, Error> {
    let token = required_token(state, workspace_id, row).await?;
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    Ok(service.fetch_diff(&reference, &token, DIFF_BYTES).await?)
}

async fn open(state: &AppState, workspace_id: Uuid, row: &PullRequestRow) -> Result<Open, Error> {
    let token = required_token(state, workspace_id, row).await?;
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    let status = observe(&service, &reference, &token, row).await?;
    if !status.open() {
        return Err(Error::Refused(AutoMerge::SETTLED.to_string()));
    }
    Ok(Open {
        service,
        reference,
        token,
        status,
    })
}

async fn token(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> DbResult<Option<SecretValue>> {
    let source = credential::source(state, workspace_id, row).await?;
    Ok(source.and_then(|source| credential::token(state, &source)))
}

async fn required_token(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> Result<SecretValue, Error> {
    token(state, workspace_id, row)
        .await
        .map_err(|error| {
            tracing::error!(pull_request = %row.id, %error, "could not read a pull request's source");
            Error::Storage
        })?
        .ok_or(Error::Uncredentialed)
}

async fn remember(state: &AppState, row: &PullRequestRow, status: &Status) {
    state.pull_request_statuses().put(row.id, status.clone());
    if row.title.as_deref() != Some(status.title.as_str())
        && let Err(error) = chat_pull_requests::retitle(state.db(), row.id, &status.title).await
    {
        tracing::warn!(pull_request = %row.id, %error, "could not keep a pull request's title");
    }
}

async fn observed(
    state: &AppState,
    row: &PullRequestRow,
    token: &SecretValue,
) -> Result<Status, Error> {
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    observe(&service, &reference, token, row).await
}

async fn observe(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    row: &PullRequestRow,
) -> Result<Status, Error> {
    let detail = service.fetch_pull(reference, token).await?;
    let lifecycle = match (detail.merged, detail.state) {
        (true, _) => State::Merged,
        (false, PullRequestState::Closed) => State::Closed,
        (false, _) => State::Open,
    };
    let checks = match lifecycle {
        State::Open => match service
            .fetch_checks(reference.repository(), token, &detail.head_sha)
            .await
        {
            Ok(outcome) => Checks::from(&outcome),
            Err(error) => {
                tracing::debug!(pull_request = %row.id, %error, "could not read a pull request's checks");
                Checks::Unknown
            }
        },
        State::Closed | State::Merged => Checks::None,
    };
    let unavailable = match lifecycle {
        State::Open => PENDING_RELEASE,
        State::Closed | State::Merged => AutoMerge::SETTLED,
    };
    Ok(Status {
        title: detail.title,
        state: lifecycle,
        draft: detail.draft,
        additions: detail.additions,
        deletions: detail.deletions,
        changed_files: detail.changed_files,
        checks,
        mergeable: Mergeable::from(detail.mergeable_state),
        auto_merge: AutoMerge::unavailable(unavailable),
        draft_unavailable: Some(unavailable.to_string()),
        observed_at: chrono::Utc::now(),
    })
}
