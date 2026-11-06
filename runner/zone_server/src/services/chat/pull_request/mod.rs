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
mod observation;
mod open;
mod opened;
mod push;
mod recorder;
mod state;
mod status;
mod threads;
mod tracked;

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{Excerpt, PullRequestState};
use abnegate_vcs::{CommitSha, MergeMethod, PullRequestReference, PullRequestService};
use futures::{StreamExt, stream};
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::chat_pull_requests::{self, PullRequestRow};
use crate::state::AppState;

pub use auto_merge::AutoMerge;
pub use cache::StatusCache;
pub use checks::Checks;
pub use error::Error;
pub use mergeable::Mergeable;
pub use opened::Opened;
pub use push::push;
pub use recorder::{Recorder, record};
pub use state::State;
pub use status::Status;
pub use threads::Threads;
pub use tracked::Tracked;

use observation::Observation;
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
    let open = open(state, workspace_id, row).await?;
    if open.status.draft != draft {
        changes::set_draft(&open, draft).await?;
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
    let open = open(state, workspace_id, row).await?;
    if open.status.auto_merge.enabled != enabled {
        changes::set_auto_merge(&open, enabled).await?;
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
    let Observation {
        status,
        node_id,
        head,
        methods,
    } = observe(&service, &reference, &token, row).await?;
    if !status.open() {
        return Err(Error::Refused(AutoMerge::SETTLED.to_string()));
    }
    Ok(Open {
        service,
        token,
        status,
        node_id,
        head,
        methods,
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
    let mut status = observe(&service, &reference, token, row).await?.status;
    if status.open() {
        status.threads = threads(&service, &reference, token, row).await;
    }
    Ok(status)
}

async fn observe(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    row: &PullRequestRow,
) -> Result<Observation, Error> {
    let detail = service.fetch_pull(reference, token).await?;
    let lifecycle = match (detail.merged, detail.state) {
        (true, _) => State::Merged,
        (false, PullRequestState::Closed) => State::Closed,
        (false, _) => State::Open,
    };
    let mergeable = Mergeable::from(detail.mergeable_state);
    let (checks, (auto_merge, methods)) = match lifecycle {
        State::Open => tokio::join!(
            checks(service, reference, token, &detail.head_sha, row),
            automation(service, reference, token, mergeable, row),
        ),
        State::Closed | State::Merged => (
            Checks::None,
            (AutoMerge::unavailable(AutoMerge::SETTLED), Vec::new()),
        ),
    };
    let draft_unavailable = match lifecycle {
        State::Open => None,
        State::Closed | State::Merged => Some(AutoMerge::SETTLED.to_string()),
    };
    Ok(Observation {
        status: Status {
            title: detail.title,
            state: lifecycle,
            draft: detail.draft,
            additions: detail.additions,
            deletions: detail.deletions,
            changed_files: detail.changed_files,
            checks,
            mergeable,
            auto_merge,
            draft_unavailable,
            threads: None,
            observed_at: chrono::Utc::now(),
        },
        node_id: detail.node_id,
        head: detail.head_sha,
        methods,
    })
}

async fn checks(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    head: &CommitSha,
    row: &PullRequestRow,
) -> Checks {
    match service
        .fetch_checks(reference.repository(), token, head)
        .await
    {
        Ok(outcome) => Checks::from(&outcome),
        Err(error) => {
            tracing::debug!(pull_request = %row.id, %error, "could not read a pull request's checks");
            Checks::Unknown
        }
    }
}

async fn threads(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    row: &PullRequestRow,
) -> Option<Threads> {
    match service.fetch_review_thread_counts(reference, token).await {
        Ok(count) => Some(Threads::from(count)),
        Err(error) => {
            tracing::debug!(pull_request = %row.id, %error, "could not read a pull request's review threads");
            None
        }
    }
}

/// Whether auto-merge is set or can be, and the ways its repository lets it
/// merge, most preferred first.
async fn automation(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    mergeable: Mergeable,
    row: &PullRequestRow,
) -> (AutoMerge, Vec<MergeMethod>) {
    match service.fetch_automation(reference, token).await {
        Ok(automation) => (
            AutoMerge::read(&automation, mergeable),
            automation.settings.methods,
        ),
        Err(error) => {
            tracing::debug!(pull_request = %row.id, %error, "could not read a pull request's auto-merge");
            (AutoMerge::unavailable(AutoMerge::UNREAD), Vec::new())
        }
    }
}
