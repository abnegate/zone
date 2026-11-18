//! The pull requests a chat opened: noticing them as its turns open them, and
//! reading and changing them on GitHub through abnegate-vcs with the
//! workspace's own source.

mod auto_fix;
mod auto_merge;
mod cache;
mod changes;
mod checks;
mod connection;
mod credential;
mod detection;
mod error;
mod merge;
mod mergeable;
mod observation;
mod open;
mod opened;
mod push;
mod reading;
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
use crate::db::sources::SourceRow;
use crate::services::github_app::{self, Credential};
use crate::state::AppState;

pub use auto_merge::AutoMerge;
pub use cache::StatusCache;
pub use checks::Checks;
pub use connection::Connection;
pub use error::Error;
pub use merge::Merge;
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
use reading::Reading;

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
/// credential lookup. An offline chat's pull request is never read, which
/// also keeps auto-fix, which only acts on a status read here, off it.
pub async fn track(
    state: &AppState,
    workspace_id: Uuid,
    mut row: PullRequestRow,
) -> DbResult<Tracked> {
    if row.offline {
        return Ok(Tracked {
            row,
            status: None,
            connection: Connection::Offline,
        });
    }
    if let Some(cached) = state.pull_request_statuses().get(row.id) {
        return Ok(Tracked {
            row,
            status: Some(cached),
            connection: Connection::Credentialed,
        });
    }
    let credential = match credential(state, workspace_id, &row).await? {
        Ok(credential) => credential,
        Err(connection) => {
            return Ok(Tracked {
                row,
                status: None,
                connection,
            });
        }
    };
    let status = match observed(state, &row, credential).await {
        Ok(Observation {
            status,
            branch,
            head,
            ..
        }) => {
            remember(state, &row, &status).await;
            auto_fix::consider(state, workspace_id, &mut row, &status, &branch, &head).await;
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
        connection: Connection::Credentialed,
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

/// Merges the pull request on GitHub, at the head it was last read at.
pub async fn merge(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> Result<(), Error> {
    let open = open(state, workspace_id, row).await?;
    let merged = changes::merge(&open).await;
    state.pull_request_statuses().forget(row.id);
    merged
}

/// Turns auto-fix on, acting as `user`, or off. Unlike draft and auto-merge
/// this is zone's own preference rather than something GitHub holds, so it
/// only touches the row; what acts on it is the next status read that finds
/// something to fix, which turning it on makes the very next one.
pub async fn set_auto_fix(
    state: &AppState,
    chat_id: Uuid,
    row: &PullRequestRow,
    enabled: bool,
    user: Uuid,
) -> Result<PullRequestRow, Error> {
    let updated = chat_pull_requests::set_auto_fix(state.db(), chat_id, row.id, enabled, user)
        .await
        .map_err(|error| {
            tracing::error!(pull_request = %row.id, %error, "could not change auto-fix");
            Error::Unsaved
        })?
        .ok_or(Error::Unsaved)?;
    if enabled {
        state.pull_request_statuses().forget(row.id);
    }
    Ok(updated)
}

/// The start of the pull request's unified diff.
pub async fn diff(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> Result<Excerpt, Error> {
    let credential = required(state, workspace_id, row).await?;
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    let (service, reference) = (&service, &reference);
    state
        .github()
        .retrying(state.db(), credential, |token| async move {
            Ok(service.fetch_diff(reference, &token, DIFF_BYTES).await?)
        })
        .await
}

async fn open(state: &AppState, workspace_id: Uuid, row: &PullRequestRow) -> Result<Open, Error> {
    let credential = required(state, workspace_id, row).await?;
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    let (
        Observation {
            status,
            node_id,
            head,
            methods,
            ..
        },
        token,
    ) = {
        let (service, reference) = (&service, &reference);
        state
            .github()
            .retrying(state.db(), credential, |token| async move {
                let observation = observe(service, reference, &token, row, Reading::Change).await?;
                Ok::<_, Error>((observation, token))
            })
            .await?
    };
    if !status.open() {
        return Err(Error::Refused(AutoMerge::SETTLED.to_string()));
    }
    Ok(Open {
        service,
        reference,
        token,
        status,
        node_id,
        head,
        methods,
    })
}

/// The credential of the source that can act on `row`, or else why the
/// workspace cannot reach GitHub for it.
async fn credential(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> DbResult<Result<Credential, Connection>> {
    let Some(source) = credential::source(state, workspace_id, row).await? else {
        return Ok(Err(Connection::Uncredentialed));
    };
    Ok(match resolved(state, row, &source).await {
        Ok(Some(credential)) => Ok(credential),
        Ok(None) => Err(Connection::Uncredentialed),
        Err(error) => Err(Connection::from(error)),
    })
}

/// What every change and diff of `row` reads GitHub with.
async fn required(
    state: &AppState,
    workspace_id: Uuid,
    row: &PullRequestRow,
) -> Result<Credential, Error> {
    if row.offline {
        return Err(Error::Offline);
    }
    let source = credential::source(state, workspace_id, row)
        .await
        .map_err(|error| {
            tracing::error!(pull_request = %row.id, %error, "could not read a pull request's source");
            Error::Storage
        })?
        .ok_or(Error::Uncredentialed)?;
    resolved(state, row, &source)
        .await?
        .ok_or(Error::Uncredentialed)
}

async fn resolved(
    state: &AppState,
    row: &PullRequestRow,
    source: &SourceRow,
) -> Result<Option<Credential>, github_app::Error> {
    credential::credential(state, source).await.inspect_err(|error| {
        tracing::warn!(pull_request = %row.id, source = %source.id, %error, "could not resolve a pull request source's GitHub credential");
    })
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
    credential: Credential,
) -> Result<Observation, Error> {
    let service = state.config().pull_requests()?;
    let reference = service.pull_request(&row.url)?;
    let (service, reference) = (&service, &reference);
    state
        .github()
        .retrying(state.db(), credential, |token| async move {
            observe(service, reference, &token, row, Reading::Listing).await
        })
        .await
}

async fn observe(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    row: &PullRequestRow,
    reading: Reading,
) -> Result<Observation, Error> {
    let detail = service.fetch_pull(reference, token).await?;
    let lifecycle = match (detail.merged, detail.state) {
        (true, _) => State::Merged,
        (false, PullRequestState::Closed) => State::Closed,
        (false, _) => State::Open,
    };
    let mergeable = Mergeable::from(detail.mergeable_state);
    let (checks, (auto_merge, methods)) = match lifecycle {
        State::Open => {
            tokio::join!(
                checks(service, reference, token, &detail.head_sha, row),
                automation(service, reference, token, mergeable, row),
            )
        }
        State::Closed | State::Merged => (
            Checks::None,
            (AutoMerge::unavailable(AutoMerge::SETTLED), Vec::new()),
        ),
    };
    let (threads, comments) = tokio::join!(
        threads(service, reference, token, row, reading),
        comments(service, reference, token, row, reading),
    );
    let draft_unavailable = match lifecycle {
        State::Open => None,
        State::Closed | State::Merged => Some(AutoMerge::SETTLED.to_string()),
    };
    let merge_unavailable = Merge::unavailable(lifecycle, detail.draft, mergeable, &methods);
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
            merge_unavailable,
            threads,
            comments,
            observed_at: chrono::Utc::now(),
        },
        node_id: detail.node_id,
        head: detail.head_sha,
        branch: detail.head,
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
    reading: Reading,
) -> Option<Threads> {
    if reading == Reading::Change {
        return None;
    }
    match service.fetch_review_thread_counts(reference, token).await {
        Ok(count) => Threads::counted(count),
        Err(error) => {
            tracing::debug!(pull_request = %row.id, %error, "could not read a pull request's review threads");
            None
        }
    }
}

/// How many comments the pull request's conversation carries, which is where
/// a review bot like CodeRabbit posts its walkthrough rather than as an
/// inline review comment.
async fn comments(
    service: &PullRequestService,
    reference: &PullRequestReference,
    token: &SecretValue,
    row: &PullRequestRow,
    reading: Reading,
) -> Option<u32> {
    if reading == Reading::Change {
        return None;
    }
    match service.fetch_issue_comments(reference, token).await {
        Ok(comments) => Some(comments.len() as u32),
        Err(error) => {
            tracing::debug!(pull_request = %row.id, %error, "could not read a pull request's conversation comments");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::github::testing::connected::Connected;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::any;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    #[tokio::test]
    async fn nothing_reads_or_changes_an_offline_chats_pull_request_on_github() {
        let connected = Connected::new().await;
        connected.reads_only(None).await;
        let row = connected.chat_pull_request(true, 12).await;
        let (state, workspace) = (&connected.state, connected.workspace);

        let tracked = track(state, workspace, row.clone())
            .await
            .expect("an offline chat's pull request");
        assert_eq!(tracked.connection, Connection::Offline);
        assert_eq!(tracked.status, None);
        assert_eq!(
            set_draft(state, workspace, &row, true).await,
            Err(Error::Offline)
        );
        assert_eq!(
            set_auto_merge(state, workspace, &row, true).await,
            Err(Error::Offline)
        );
        assert_eq!(
            diff(state, workspace, &row)
                .await
                .map(|excerpt| excerpt.text),
            Err(Error::Offline)
        );

        connected.github.verify().await;
    }

    fn stub_row() -> PullRequestRow {
        PullRequestRow {
            id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            provider: "github".to_string(),
            owner: "acme".to_string(),
            repo: "shop".to_string(),
            number: 7,
            url: "https://github.com/acme/shop/pull/7".to_string(),
            title: None,
            source_id: None,
            dismissed_at: None,
            created_at: chrono::Utc::now(),
            auto_fix: false,
            auto_fix_run_id: None,
            auto_fix_by: None,
            auto_fix_head: None,
            auto_fixing: false,
            offline: false,
        }
    }

    fn stub_reference() -> PullRequestReference {
        PullRequestService::new()
            .expect("GitHub's own origin")
            .pull_request("https://github.com/acme/shop/pull/7")
            .expect("a pull request URL")
    }

    fn stub_token() -> abnegate_secret::SecretValue {
        abnegate_secret::SecretValue::new("token")
    }

    #[tokio::test]
    async fn a_reading_of_change_asks_github_nothing_for_conversation_comments() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let service =
            PullRequestService::standing_in_for("github.com", &server.uri()).expect("a stand-in");

        let count = comments(
            &service,
            &stub_reference(),
            &stub_token(),
            &stub_row(),
            Reading::Change,
        )
        .await;

        assert_eq!(count, None);
    }

    #[tokio::test]
    async fn an_unreadable_comment_list_is_none_rather_than_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/shop/issues/7/comments"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let service =
            PullRequestService::standing_in_for("github.com", &server.uri()).expect("a stand-in");

        let count = comments(
            &service,
            &stub_reference(),
            &stub_token(),
            &stub_row(),
            Reading::Listing,
        )
        .await;

        assert_eq!(count, None);
    }

    #[tokio::test]
    async fn the_conversation_comments_github_lists_are_counted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/shop/issues/7/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": 1,
                    "body": "Walkthrough",
                    "html_url": "https://github.com/acme/shop/pull/7#issuecomment-1",
                    "created_at": "2026-09-05T00:00:00Z",
                    "user": {"login": "coderabbitai[bot]"},
                },
                {
                    "id": 2,
                    "body": "Updated",
                    "html_url": "https://github.com/acme/shop/pull/7#issuecomment-2",
                    "created_at": "2026-09-05T00:05:00Z",
                    "user": {"login": "coderabbitai[bot]"},
                },
            ])))
            .mount(&server)
            .await;
        let service =
            PullRequestService::standing_in_for("github.com", &server.uri()).expect("a stand-in");

        let count = comments(
            &service,
            &stub_reference(),
            &stub_token(),
            &stub_row(),
            Reading::Listing,
        )
        .await;

        assert_eq!(count, Some(2));
    }

    /// A merged or closed pull request used to carry no review threads at
    /// all, because 63b40398 stopped reading them there for cost. Both
    /// threads and the conversation comment count have to survive that
    /// branch now.
    #[tokio::test]
    async fn a_settled_pull_request_still_carries_its_threads_and_comments() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/shop/pulls/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "node_id": "PR_7",
                "number": 7,
                "title": "Add a cart",
                "state": "closed",
                "head": {"ref": "widgets", "sha": "a".repeat(40)},
                "base": {"ref": "main", "sha": "b".repeat(40)},
                "html_url": "https://github.com/acme/shop/pull/7",
                "merged": true,
                "merge_commit_sha": "c".repeat(40),
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/shop/issues/7/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": 1,
                    "body": "Walkthrough",
                    "html_url": "https://github.com/acme/shop/pull/7#issuecomment-1",
                    "created_at": "2026-09-05T00:00:00Z",
                    "user": {"login": "coderabbitai[bot]"},
                },
            ])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"repository": {"pullRequest": {
                    "number": 7,
                    "reviewThreads": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": [{"id": "PRRT_1", "isResolved": true}],
                    },
                }}},
            })))
            .mount(&server)
            .await;
        let service =
            PullRequestService::standing_in_for("github.com", &server.uri()).expect("a stand-in");

        let observation = observe(
            &service,
            &stub_reference(),
            &stub_token(),
            &stub_row(),
            Reading::Listing,
        )
        .await
        .expect("a merged pull request reads back");

        assert_eq!(observation.status.state, State::Merged);
        assert_eq!(
            observation.status.threads,
            Some(Threads {
                open: 0,
                resolved: 1
            })
        );
        assert_eq!(observation.status.comments, Some(1));
    }
}
