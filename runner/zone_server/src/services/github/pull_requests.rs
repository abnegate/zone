mod assessed;
mod bot_comment;
mod check_detail;
mod check_kind;
mod commit;
mod commits;
mod counts;
mod depth;
mod detail;
mod error;
mod listing;
mod log;
mod merged;
mod method;
mod overview;
mod pull_request;
mod readiness;
mod reading;
mod refusal;
mod repository;
#[cfg(test)]
mod tests;
mod thread;
mod thread_comment;

pub use assessed::Assessed;
pub use bot_comment::BotComment;
pub use check_detail::CheckDetail;
pub use check_kind::CheckKind;
pub use commit::Commit;
pub use commits::Commits;
pub use counts::Counts;
pub use depth::Depth;
pub use detail::Detail;
pub use error::Error;
pub use listing::Listing;
pub use log::Log;
pub use merged::Merged;
pub use method::Method;
pub use overview::Overview;
pub use pull_request::PullRequest;
pub use readiness::Readiness;
pub use refusal::Refusal;
pub use repository::Repository;
pub use thread::Thread;
pub use thread_comment::ThreadComment;

use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::PullRequestDetail;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestReference;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use futures::StreamExt;
use futures::future::join_all;
use futures::stream;
use reqwest::Url;
use uuid::Uuid;

use super::authentication::authenticated;
use super::client::Github;
use super::client::MEBIBYTE;
use super::client::RATE_LIMIT_EXHAUSTED;
use super::client::UNAUTHORIZED;
use super::client::listed;
use super::client::status_error;
use super::connection::ConnectedSource;
use super::connection::connect;
use super::connection::connected_sources;
use super::diff;
use super::diff::Diff;
use super::freshness::Freshness;
use super::readiness::Assessment;
use super::readiness::CheckState;
use super::readiness::CommitSha;
use super::snapshot::Snapshot;
use super::source_status::SourceStatus;
use crate::state::AppState;

pub const DIFF_BUDGET: u64 = 16 * MEBIBYTE;
pub const LOG_TAIL: u64 = MEBIBYTE;
pub const FAILURE_REFRESH_INTERVAL: Duration = Duration::from_secs(120);
pub const UNRANKED: u32 = 0;
pub const FOREIGN_COMMIT: &str = "That commit is not on this pull request.";
pub const UNRENDERED_COMMIT: &str = "GitHub would not render this commit's diff.";
pub const INVALID_SOURCE: &str = "has a GitHub configuration Zone cannot read.";
const ASSESSMENT_CONCURRENCY: usize = 4;
const PARENT: &str = "~1";
const MERGE_MESSAGE: &str = "";
const WEB_SCHEMES: [&str; 2] = ["https", "http"];

pub fn absolute(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    (WEB_SCHEMES.contains(&parsed.scheme()) && parsed.has_host()).then(|| url.to_string())
}

pub fn bucket(assessment: &Assessment) -> Readiness {
    if assessment.ready {
        Readiness::Ready
    } else if assessment.checks.state == CheckState::Pending {
        Readiness::InProgress
    } else {
        Readiness::NotReady
    }
}

pub fn refresh_plan<'a>(
    previous: Option<&'a Listing>,
    listed: &[ListedPullRequest],
    now: DateTime<Utc>,
    force: bool,
) -> Vec<Option<&'a PullRequest>> {
    listed
        .iter()
        .map(|row| {
            let kept = previous
                .filter(|_| !force)?
                .pull_requests
                .iter()
                .find(|kept| kept.number == row.reference.number().get())?;
            current(kept, row, now).then_some(kept)
        })
        .collect()
}

pub fn ranked(mut pull_requests: Vec<PullRequest>) -> Vec<PullRequest> {
    pull_requests.sort_by(|first, second| {
        first
            .readiness
            .cmp(&second.readiness)
            .then_with(|| second.updated_at.cmp(&first.updated_at))
            .then_with(|| first.repository.cmp(&second.repository))
            .then_with(|| first.number.cmp(&second.number))
    });
    for (position, pull_request) in pull_requests.iter_mut().enumerate() {
        pull_request.rank = u32::try_from(position + 1).unwrap_or(u32::MAX);
    }
    pull_requests
}

pub async fn list(state: &AppState, workspace_id: Uuid, refresh: bool) -> Result<Overview, Error> {
    let now = Utc::now();
    let sources = connected_sources(state, workspace_id).await?;
    let origin = state.config().github_web_url.as_str();
    let reads: Vec<_> = sources
        .connected
        .iter()
        .map(|source| read(state, source, refresh, now))
        .collect();
    let reads = join_all(reads).await;
    let mut stale = false;
    let mut partial = !sources.invalid.is_empty();
    let mut statuses = Vec::with_capacity(reads.len());
    let mut warnings: Vec<String> = sources
        .invalid
        .iter()
        .map(|source| format!("{} {INVALID_SOURCE}", source.name))
        .collect();
    let mut pull_requests = Vec::new();
    for (source, read) in reads {
        match read {
            Ok((snapshot, freshness)) => {
                stale |= freshness == Freshness::Stale;
                partial |= !snapshot.value.complete;
                warnings.extend(snapshot.value.warnings.iter().cloned());
                pull_requests.extend(snapshot.value.pull_requests.iter().cloned());
                statuses.push(SourceStatus::read(source, origin, &snapshot, freshness));
            }
            Err(warning) => {
                partial = true;
                statuses.push(SourceStatus::failed(source, origin, warning));
            }
        }
    }
    let pull_requests = ranked(pull_requests);
    Ok(Overview {
        generated_at: now,
        stale,
        partial,
        counts: Counts::of(&pull_requests),
        sources: statuses,
        pull_requests,
        warnings,
    })
}

pub async fn detail(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    number: u64,
) -> Result<Detail, Error> {
    let source = connect(state, workspace_id, source_id).await?;
    let repository = &Repository::of(&source, &state.config().github_web_url);
    authorised(state, &source, |github| async move {
        let reference = github.reference(number).map_err(Error::Invalid)?;
        let read = github.pull(&reference).await.map_err(Error::described)?;
        let row = author(listed(reference, &read), &read);
        let (assessed, threads) = tokio::try_join!(
            github.assess(&row, Some(read), Depth::Listing),
            github.unresolved_threads(number),
        )
        .map_err(Error::described)?;
        let comments = assessed.bot_comments();
        Ok(Detail {
            pull_request: assessed.pull_request(repository, &row, Utc::now()),
            threads,
            comments,
        })
    })
    .await
}

pub async fn diff(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    number: u64,
    expected: &CommitSha,
) -> Result<Diff, Error> {
    let source = connect(state, workspace_id, source_id).await?;
    let repository = &Repository::of(&source, &state.config().github_web_url);
    authorised(state, &source, |github| async move {
        let (reference, read) = headed(&github, number, expected).await?;
        let revision = diff::Revision {
            repository_url: &repository.url,
            number,
            head: read.head_sha.as_str(),
            base: read.base.as_str(),
        };
        Ok(github.pull_diff(&reference, &revision).await?)
    })
    .await
}

pub async fn commits(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    number: u64,
    expected: &CommitSha,
) -> Result<Commits, Error> {
    let source = connect(state, workspace_id, source_id).await?;
    let repository = &Repository::of(&source, &state.config().github_web_url);
    authorised(state, &source, |github| async move {
        let (reference, read) = headed(&github, number, expected).await?;
        let (commits, complete) = github
            .pull_commits(&reference, read.commits, repository)
            .await
            .map_err(Error::described)?;
        Ok(Commits {
            number,
            head: read.head_sha.to_string(),
            base: read.base.to_string(),
            complete,
            commits,
        })
    })
    .await
}

pub async fn commit_diff(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    number: u64,
    sha: &CommitSha,
    expected: &CommitSha,
) -> Result<Diff, Error> {
    let source = connect(state, workspace_id, source_id).await?;
    let repository = &Repository::of(&source, &state.config().github_web_url);
    authorised(state, &source, |github| async move {
        let (reference, read) = headed(&github, number, expected).await?;
        let (commits, _) = github
            .pull_commits(&reference, read.commits, repository)
            .await
            .map_err(Error::described)?;
        if !commits.iter().any(|commit| commit.sha == sha.as_str()) {
            return Err(Error::Missing(FOREIGN_COMMIT.to_string()));
        }
        let commit = Revision::parse(sha.as_str())
            .map_err(|_| Error::Invalid(FOREIGN_COMMIT.to_string()))?;
        let parent = format!("{}{PARENT}", sha.as_str());
        let revision = diff::Revision {
            repository_url: &repository.url,
            number,
            head: sha.as_str(),
            base: &parent,
        };
        Ok(match github.commit_diff_excerpt(&commit).await? {
            Some(excerpt) => diff::parse(&excerpt.text, excerpt.truncated, &revision),
            None => Diff {
                number,
                head: sha.as_str().to_string(),
                base: parent.clone(),
                complete: false,
                warning: Some(UNRENDERED_COMMIT.to_string()),
                files: Vec::new(),
            },
        })
    })
    .await
}

pub async fn job_log(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    number: u64,
    job_id: NonZeroU64,
    expected: &CommitSha,
) -> Result<Log, Error> {
    let source = connect(state, workspace_id, source_id).await?;
    authorised(state, &source, |github| async move {
        let (_, read) = headed(&github, number, expected).await?;
        github
            .job_log_tail(job_id, read.head_sha.as_str(), Utc::now())
            .await
            .map_err(Error::described)
    })
    .await
}

pub async fn merge(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    number: u64,
    expected: &CommitSha,
    method: Method,
) -> Result<Merged, Error> {
    let source = connect(state, workspace_id, source_id).await?;
    let repository = &Repository::of(&source, &state.config().github_web_url);
    let administrator = state.config().github.admin_merge;
    let merged = authorised(state, &source, |github| async move {
        let (reference, read) = headed(&github, number, expected).await?;
        let node = read.node_id.clone();
        let head = read.head_sha.clone();
        let title = format!("{} (#{number})", read.title);
        let url = absolute(&read.url).unwrap_or_else(|| repository.pull_url(number));
        let row = listed(reference.clone(), &read);
        let assessed = github
            .assess(&row, Some(read), Depth::Listing)
            .await
            .map_err(Error::described)?;
        if !assessed.assessment.ready {
            return Err(Refusal::NotReady {
                blockers: assessed.assessment.reports(),
            }
            .into());
        }
        let merged = github
            .service()
            .merge(
                &reference,
                github.token(),
                Some(&node),
                &head,
                &title,
                MERGE_MESSAGE,
                method.into(),
                administrator,
            )
            .await;
        match merged {
            Ok(merged) => Ok(Merged {
                number,
                sha: merged.sha.map(|sha| sha.to_string()),
                admin: merged.administrator,
                url,
            }),
            Err(PullRequestError::HeadMoved) => Err(moved(&github, &reference).await),
            Err(PullRequestError::NotMergeable(_)) => Err(Refusal::Conflict.into()),
            Err(PullRequestError::Protected(_)) => Err(Refusal::Protected.into()),
            Err(error) => Err(error.into()),
        }
    })
    .await?;
    state.github_snapshots().invalidate(source_id);
    Ok(merged)
}

async fn read<'a>(
    state: &AppState,
    source: &'a ConnectedSource,
    refresh: bool,
    now: DateTime<Utc>,
) -> (
    &'a ConnectedSource,
    Result<(Snapshot<Listing>, Freshness), String>,
) {
    let read = state
        .github_snapshots()
        .pulls
        .read(source.source.id, refresh, now, |previous| {
            fetch(state, source, previous, now, refresh)
        })
        .await;
    (source, read)
}

async fn fetch(
    state: &AppState,
    source: &ConnectedSource,
    previous: Option<Arc<Listing>>,
    now: DateTime<Utc>,
    force: bool,
) -> Result<Listing, String> {
    let limit = state.config().github.open_pull_request_limit;
    let repository = &Repository::of(source, &state.config().github_web_url);
    let previous = previous.as_deref();
    authenticated(
        state,
        &source.source,
        source.configuration.clone(),
        |github| async move {
            let (rows, complete) = github.open_pulls(limit).await?;
            let plan = refresh_plan(previous, &rows, now, force);
            let pending: Vec<_> = rows
                .iter()
                .zip(plan)
                .map(|(row, kept)| reassess(&github, repository, row, kept, now))
                .collect();
            let assessed: Vec<Result<PullRequest, String>> = stream::iter(pending)
                .buffered(ASSESSMENT_CONCURRENCY)
                .collect()
                .await;
            let mut listing = Listing {
                pull_requests: Vec::with_capacity(rows.len()),
                complete,
                warnings: Vec::new(),
            };
            for (row, result) in rows.iter().zip(assessed) {
                match result {
                    Ok(pull_request) => listing.pull_requests.push(pull_request),
                    Err(message) if halts(&message) => return Err(message),
                    Err(message) => {
                        let number = row.reference.number().get();
                        listing.complete = false;
                        listing
                            .warnings
                            .push(format!("{}#{number}: {message}", repository.name));
                        listing.pull_requests.extend(
                            previous
                                .into_iter()
                                .flat_map(|previous| previous.pull_requests.iter())
                                .find(|kept| kept.number == number)
                                .cloned(),
                        );
                    }
                }
            }
            Ok(listing)
        },
    )
    .await
}

async fn reassess(
    github: &Github,
    repository: &Repository,
    row: &ListedPullRequest,
    kept: Option<&PullRequest>,
    now: DateTime<Utc>,
) -> Result<PullRequest, String> {
    match kept {
        Some(kept) => Ok(kept.clone()),
        None => github
            .assess(row, None, Depth::Listing)
            .await
            .map(|assessed| assessed.pull_request(repository, row, now)),
    }
}

async fn authorised<T, Operation, Operated>(
    state: &AppState,
    source: &ConnectedSource,
    operation: Operation,
) -> Result<T, Error>
where
    Operation: Fn(Github) -> Operated,
    Operated: Future<Output = Result<T, Error>>,
{
    let operation = &operation;
    authenticated(
        state,
        &source.source,
        source.configuration.clone(),
        |github| async move {
            match operation(github).await {
                Err(error) => match error.unauthenticated() {
                    Some(message) => Err(message.to_string()),
                    None => Ok(Err(error)),
                },
                Ok(value) => Ok(Ok(value)),
            }
        },
    )
    .await
    .map_err(Error::Disconnected)?
}

async fn headed(
    github: &Github,
    number: u64,
    expected: &CommitSha,
) -> Result<(PullRequestReference, PullRequestDetail), Error> {
    let reference = github.reference(number).map_err(Error::Invalid)?;
    let read = github.pull(&reference).await.map_err(Error::described)?;
    if !read
        .head_sha
        .as_str()
        .eq_ignore_ascii_case(expected.as_str())
    {
        return Err(Refusal::HeadMoved {
            current_head: read.head_sha.to_string(),
        }
        .into());
    }
    Ok((reference, read))
}

async fn moved(github: &Github, reference: &PullRequestReference) -> Error {
    match github.pull(reference).await {
        Ok(read) => Refusal::HeadMoved {
            current_head: read.head_sha.to_string(),
        }
        .into(),
        Err(message) => Error::described(message),
    }
}

fn author(row: ListedPullRequest, read: &PullRequestDetail) -> ListedPullRequest {
    match &read.author_login {
        Some(login) => row.with_author_login(login.clone()),
        None => row,
    }
}

fn current(kept: &PullRequest, row: &ListedPullRequest, now: DateTime<Utc>) -> bool {
    let unchanged = kept.updated_at == row.updated_at.as_deref().unwrap_or_default()
        && matches!(
            (&kept.head, &row.head_sha),
            (Some(kept), Some(listed)) if kept.as_str().eq_ignore_ascii_case(listed.as_str())
        );
    let settled = kept.checks.complete
        && kept.threads_complete
        && kept.comments_complete
        && !matches!(kept.checks.state, CheckState::Pending | CheckState::Unknown);
    let recent = kept.checks.state != CheckState::Failure
        || now.signed_duration_since(kept.assessed_at)
            <= TimeDelta::from_std(FAILURE_REFRESH_INTERVAL).unwrap_or(TimeDelta::MAX);
    unchanged && settled && recent
}

fn halts(message: &str) -> bool {
    message == RATE_LIMIT_EXHAUSTED || message == status_error(UNAUTHORIZED)
}
