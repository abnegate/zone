//! The pull requests a chat opened: noticing them as its turns open them, and
//! reading and changing them on GitHub with the workspace's own source.

mod auto_merge;
mod automation;
mod cache;
mod checks;
mod credential;
mod detection;
mod error;
mod github;
mod merge_method;
mod mergeable;
mod opened;
mod recorder;
mod state;
mod status;
mod tracked;

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{Excerpt, MergeableState, PullRequestState};
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
pub use recorder::{Recorder, record};
pub use state::State;
pub use status::Status;
pub use tracked::Tracked;

use automation::Automation;
use github::Github;

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
pub async fn track(state: &AppState, workspace_id: Uuid, row: PullRequestRow) -> DbResult<Tracked> {
    let Some(token) = token(state, workspace_id, &row).await? else {
        return Ok(Tracked {
            row,
            status: None,
            credentialed: false,
        });
    };
    let status = match state.pull_request_statuses().get(row.id) {
        Some(cached) => Some(cached),
        None => match observe(state, &row, &token).await {
            Ok(status) => {
                remember(state, &row, &status).await;
                Some(status)
            }
            Err(error) => {
                tracing::warn!(pull_request = %row.id, %error, "could not read a chat's pull request from GitHub");
                None
            }
        },
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
    let token = required_token(state, workspace_id, row).await?;
    let status = observe(state, row, &token).await?;
    if !status.open() {
        return Err(Error::Refused(AutoMerge::SETTLED.to_string()));
    }
    if status.draft != draft {
        Github::new(state.config())?
            .set_draft(&token, &status.node_id, draft)
            .await?;
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
    let token = required_token(state, workspace_id, row).await?;
    let status = observe(state, row, &token).await?;
    if status.auto_merge.enabled != enabled {
        if let Some(reason) = &status.auto_merge.unavailable {
            return Err(Error::Refused(reason.clone()));
        }
        let method = match enabled {
            true => Some(
                status
                    .merge_method
                    .ok_or_else(|| Error::Refused(AutoMerge::NOT_PERMITTED.to_string()))?,
            ),
            false => None,
        };
        Github::new(state.config())?
            .set_auto_merge(&token, &status.node_id, method)
            .await?;
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

async fn observe(
    state: &AppState,
    row: &PullRequestRow,
    token: &SecretValue,
) -> Result<Status, Error> {
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    let repository = reference.repository();
    let github = Github::new(state.config())?;
    let (detail, automation) = tokio::join!(
        service.fetch_pull(&reference, token),
        github.automation(
            token,
            repository.owner(),
            repository.name(),
            reference.number().get()
        ),
    );
    let detail = detail?;
    let lifecycle = match (detail.merged, detail.state) {
        (true, _) => State::Merged,
        (false, PullRequestState::Closed) => State::Closed,
        (false, _) => State::Open,
    };
    let checks = match lifecycle {
        State::Open => match service
            .fetch_checks(repository, token, &detail.head_sha)
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
    let merge_method = automation
        .as_ref()
        .ok()
        .and_then(|automation| automation.method);
    let auto_merge = automatic(lifecycle, detail.draft, detail.mergeable_state, automation);
    Ok(Status {
        title: detail.title,
        state: lifecycle,
        draft: detail.draft,
        additions: detail.additions,
        deletions: detail.deletions,
        changed_files: detail.changed_files,
        checks,
        mergeable: Mergeable::from(detail.mergeable_state),
        auto_merge,
        observed_at: chrono::Utc::now(),
        node_id: detail.node_id,
        merge_method,
    })
}

/// Whether auto-merge is on, and when it cannot be turned on, why not.
fn automatic(
    lifecycle: State,
    draft: bool,
    mergeable: MergeableState,
    automation: Result<Automation, Error>,
) -> AutoMerge {
    if lifecycle != State::Open {
        return AutoMerge::unavailable(AutoMerge::SETTLED);
    }
    let Ok(automation) = automation else {
        return AutoMerge::unavailable(AutoMerge::UNREAD);
    };
    if automation.enabled {
        return AutoMerge {
            enabled: true,
            unavailable: (!automation.changeable).then(|| AutoMerge::NOT_PERMITTED.to_string()),
        };
    }
    let reason = if !automation.allowed {
        Some(AutoMerge::OFF_FOR_REPOSITORY)
    } else if draft {
        Some(AutoMerge::DRAFT)
    } else if matches!(mergeable, MergeableState::Clean | MergeableState::HasHooks) {
        Some(AutoMerge::READY)
    } else if !automation.changeable || automation.method.is_none() {
        Some(AutoMerge::NOT_PERMITTED)
    } else {
        None
    };
    AutoMerge {
        enabled: false,
        unavailable: reason.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use merge_method::MergeMethod;

    fn automation(allowed: bool, enabled: bool, changeable: bool) -> Result<Automation, Error> {
        Ok(Automation {
            allowed,
            enabled,
            changeable,
            method: Some(MergeMethod::Squash),
        })
    }

    fn reason(auto_merge: &AutoMerge) -> Option<&str> {
        auto_merge.unavailable.as_deref()
    }

    #[test]
    fn auto_merge_can_be_turned_on_only_while_something_is_still_pending() {
        let blocked = MergeableState::Blocked;
        assert_eq!(
            automatic(State::Open, false, blocked, automation(true, false, true)),
            AutoMerge {
                enabled: false,
                unavailable: None
            }
        );
        for (lifecycle, draft, mergeable, read, expected) in [
            (
                State::Merged,
                false,
                blocked,
                automation(true, false, true),
                AutoMerge::SETTLED,
            ),
            (
                State::Open,
                false,
                blocked,
                Err(Error::Unreachable),
                AutoMerge::UNREAD,
            ),
            (
                State::Open,
                false,
                blocked,
                automation(false, false, true),
                AutoMerge::OFF_FOR_REPOSITORY,
            ),
            (
                State::Open,
                true,
                blocked,
                automation(true, false, true),
                AutoMerge::DRAFT,
            ),
            (
                State::Open,
                false,
                MergeableState::Clean,
                automation(true, false, false),
                AutoMerge::READY,
            ),
            (
                State::Open,
                false,
                blocked,
                automation(true, false, false),
                AutoMerge::NOT_PERMITTED,
            ),
        ] {
            let auto_merge = automatic(lifecycle, draft, mergeable, read);
            assert_eq!(reason(&auto_merge), Some(expected));
            assert!(!auto_merge.enabled);
        }
    }

    #[test]
    fn auto_merge_that_is_on_says_so_and_whether_it_can_be_turned_off() {
        let on = automatic(
            State::Open,
            false,
            MergeableState::Blocked,
            automation(true, true, true),
        );
        assert!(on.enabled);
        assert_eq!(reason(&on), None);

        let stuck = automatic(
            State::Open,
            false,
            MergeableState::Blocked,
            automation(true, true, false),
        );
        assert!(stuck.enabled);
        assert_eq!(reason(&stuck), Some(AutoMerge::NOT_PERMITTED));
    }
}
