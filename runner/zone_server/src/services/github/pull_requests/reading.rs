use std::collections::HashSet;
use std::num::NonZeroU8;
use std::num::NonZeroU32;
use std::num::NonZeroU64;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use abnegate_vcs::ChangedFile;
use abnegate_vcs::Collected;
use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::EvidenceSlot;
use abnegate_vcs::Excerpt;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::PullRequestCommit;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestEvidence;
use abnegate_vcs::PullRequestReference;
use abnegate_vcs::RepositoryId;
use abnegate_vcs::ReviewThreadRecord;
use abnegate_vcs::StateFilter;
use chrono::DateTime;
use chrono::Utc;
use tokio::sync::Semaphore;

use super::Commit;
use super::DIFF_BUDGET;
use super::LOG_TAIL;
use super::Log;
use super::Repository;
use super::Thread;
use super::ThreadComment;
use crate::services::github::client::Github;
use crate::services::github::client::PER_PAGE;
use crate::services::github::diff;
use crate::services::github::diff::Diff;
use crate::services::github::diff::Entry;
use crate::services::github::fault::Fault;
use crate::services::github::link::absolute;
use crate::services::github::throttle::admitted;

pub(super) const NOT_ACCEPTABLE: u16 = 406;
pub(super) const MOVED_REPOSITORY: &str =
    "GitHub listed open pull requests of another repository part way through.";

impl Github {
    pub(crate) async fn open_pulls(
        &self,
        limit: u32,
    ) -> Result<(Vec<ListedPullRequest>, bool), Fault> {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let mut pulls = Vec::new();
        let mut page = NonZeroU32::MIN;
        loop {
            let listed = self
                .service()
                .list_pull_requests(
                    self.repository(),
                    self.token(),
                    StateFilter::Open,
                    page,
                    PER_PAGE,
                )
                .await?;
            pulls.extend(listed.items);
            match listed.next {
                Some(next) if pulls.len() < limit => page = next,
                next => {
                    let complete = next.is_none() && pulls.len() <= limit;
                    pulls.truncate(limit);
                    return Ok((pulls, complete));
                }
            }
        }
    }

    /// Up to `limit` of the repository's open pull requests, the most
    /// recently updated first, read through GraphQL a page at a time under
    /// one of `permits` each, with the repository's node id and whether that
    /// was every open pull request.
    pub(crate) async fn open_pull_requests(
        &self,
        permits: &Semaphore,
        limit: u32,
    ) -> Result<(RepositoryId, Vec<ListedPullRequest>, bool), Fault> {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let mut pulls: Vec<ListedPullRequest> = Vec::new();
        let mut listed = HashSet::new();
        let mut repository: Option<RepositoryId> = None;
        let mut after: Option<String> = None;
        loop {
            let page = {
                let _permit = admitted(permits).await?;
                self.bounded(self.service().list_open_pull_requests(
                    self.repository(),
                    self.token(),
                    page_size(limit.saturating_sub(pulls.len())),
                    after.as_deref(),
                ))
                .await??
            };
            let identified = repository.get_or_insert_with(|| page.repository_id.clone());
            if *identified != page.repository_id {
                return Err(Fault::upstream(MOVED_REPOSITORY));
            }
            pulls.extend(
                page.items
                    .into_iter()
                    .filter(|row| listed.insert(row.node_id.clone())),
            );
            match page.next {
                Some(next) if pulls.len() < limit => after = Some(next),
                next => {
                    let complete = next.is_none() && pulls.len() <= limit;
                    pulls.truncate(limit);
                    return Ok((page.repository_id, pulls, complete));
                }
            }
        }
    }

    /// The evidence on each pull request `rows` lists, read in one GraphQL
    /// request under one of `permits` that is let go once GitHub answers or
    /// the client's guard runs out, whichever is first. A pull request read
    /// for another,
    /// or that GitHub could not answer for, is `None`, and so is every one of
    /// them when GitHub refused the request for any reason short of a halt or
    /// took too long, to be read through REST instead.
    ///
    /// A refusal that every later batch would meet too, or a batch GitHub
    /// could not answer at all, sets `stop`, and a batch sent once `stop` is
    /// set asks GitHub for nothing.
    pub(crate) async fn evidence(
        &self,
        permits: &Semaphore,
        repository: &RepositoryId,
        rows: &[&ListedPullRequest],
        stop: &AtomicBool,
    ) -> Result<Vec<Option<PullRequestEvidence>>, Fault> {
        let unread = || rows.iter().map(|_| None).collect();
        let ids: Vec<&str> = rows.iter().map(|row| row.node_id.as_str()).collect();
        let read = {
            let _permit = admitted(permits).await?;
            if stop.load(Ordering::Acquire) {
                return Ok(unread());
            }
            self.bounded(self.service().fetch_pull_request_evidence(
                self.repository(),
                self.token(),
                repository,
                &ids,
            ))
            .await
        };
        let slots = match read {
            Ok(Ok(slots)) => slots,
            Ok(Err(error)) => {
                let doomed = matches!(
                    error,
                    PullRequestError::Forbidden | PullRequestError::GitHubApi(_)
                );
                let fault = Fault::from(error);
                if fault.halts() {
                    return Err(fault);
                }
                if doomed {
                    stop.store(true, Ordering::Release);
                }
                tracing::warn!(
                    repository = %self.repository(),
                    %fault,
                    "Reading pull requests through REST after GitHub refused their batch"
                );
                return Ok(unread());
            }
            Err(fault) => {
                stop.store(true, Ordering::Release);
                tracing::warn!(
                    repository = %self.repository(),
                    %fault,
                    "Reading pull requests through REST after GitHub took too long over their batch"
                );
                return Ok(unread());
            }
        };
        if !slots.is_empty()
            && slots
                .iter()
                .all(|slot| matches!(slot, EvidenceSlot::Unanswered))
        {
            stop.store(true, Ordering::Release);
        }
        Ok(self.vouched(slots, rows))
    }

    fn vouched(
        &self,
        slots: Vec<EvidenceSlot>,
        rows: &[&ListedPullRequest],
    ) -> Vec<Option<PullRequestEvidence>> {
        if slots.len() != rows.len() {
            tracing::warn!(
                repository = %self.repository(),
                asked = rows.len(),
                answered = slots.len(),
                "GitHub answered for a different number of pull requests than asked"
            );
            return rows.iter().map(|_| None).collect();
        }
        slots
            .into_iter()
            .zip(rows)
            .map(|(slot, row)| slot.into_evidence().filter(|read| answers(read, row)))
            .collect()
    }

    pub(crate) async fn pull_commits(
        &self,
        reference: &PullRequestReference,
        counted: u32,
        repository: &Repository,
    ) -> Result<(Vec<Commit>, bool), Fault> {
        let mut commits = Vec::new();
        let mut page = NonZeroU32::MIN;
        loop {
            let listed = self
                .service()
                .list_commits(reference, self.token(), page, PER_PAGE)
                .await?;
            commits.extend(
                listed
                    .items
                    .into_iter()
                    .map(|commit| written(commit, repository)),
            );
            match listed.next {
                Some(next) => page = next,
                None => {
                    let counted = usize::try_from(counted).unwrap_or(usize::MAX);
                    let complete = commits.len() >= counted;
                    return Ok((commits, complete));
                }
            }
        }
    }

    pub(crate) async fn pull_diff(
        &self,
        reference: &PullRequestReference,
        revision: &diff::Revision<'_>,
    ) -> Result<Diff, PullRequestError> {
        match self
            .service()
            .fetch_diff(reference, self.token(), budget())
            .await
        {
            Ok(excerpt) => Ok(diff::parse(&excerpt.text, excerpt.truncated, revision)),
            Err(PullRequestError::Refused {
                status: NOT_ACCEPTABLE,
                ..
            }) => {
                let files = self.service().fetch_files(reference, self.token()).await?;
                Ok(diff::per_file(budgeted(&files, budget()), revision))
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn commit_diff_excerpt(
        &self,
        sha: &Revision,
    ) -> Result<Option<Excerpt>, PullRequestError> {
        match self
            .service()
            .fetch_commit_diff(self.repository(), self.token(), sha, budget())
            .await
        {
            Ok(excerpt) => Ok(Some(excerpt)),
            Err(PullRequestError::Refused {
                status: NOT_ACCEPTABLE,
                ..
            }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn job_log_tail(
        &self,
        job_id: NonZeroU64,
        head: &str,
        fetched_at: DateTime<Utc>,
    ) -> Result<Log, Fault> {
        let job = self
            .service()
            .fetch_workflow_job(self.repository(), self.token(), job_id)
            .await?;
        let run_id = self
            .verify_job(&job, job_id.get(), head)
            .map_err(Fault::Missing)?;
        let downloaded = self
            .service()
            .fetch_job_log_tail(
                self.repository(),
                self.token(),
                job_id,
                usize::try_from(LOG_TAIL).unwrap_or(usize::MAX),
            )
            .await?;
        Ok(Log {
            job_id: job_id.get(),
            run_id: Some(run_id),
            head: head.to_string(),
            text: String::from_utf8_lossy(&downloaded.content).into_owned(),
            truncated: downloaded.truncated,
            fetched_at,
        })
    }
}

pub(super) fn unresolved(threads: Collected<ReviewThreadRecord>) -> Vec<Thread> {
    threads
        .items
        .into_iter()
        .filter(|thread| !thread.resolved && !thread.comments.is_empty())
        .map(thread)
        .collect()
}

pub(super) fn budgeted(files: &[ChangedFile], budget: usize) -> Vec<Entry<'_>> {
    let mut remaining = budget;
    files
        .iter()
        .map(|file| {
            let patch = file.patch.as_deref().filter(|patch| {
                let fits = patch.len() <= remaining;
                remaining = if fits { remaining - patch.len() } else { 0 };
                fits
            });
            Entry {
                path: &file.filename,
                previous_path: file.previous_filename.as_deref(),
                status: file.status.into(),
                additions: file.additions,
                deletions: file.deletions,
                patch,
            }
        })
        .collect()
}

fn page_size(remaining: usize) -> NonZeroU8 {
    let size = remaining.clamp(1, usize::from(PER_PAGE.get()));
    u8::try_from(size)
        .ok()
        .and_then(NonZeroU8::new)
        .unwrap_or(PER_PAGE)
}

fn answers(read: &PullRequestEvidence, row: &ListedPullRequest) -> bool {
    read.pull_request.node_id == row.node_id && read.pull_request.reference == row.reference
}

fn budget() -> usize {
    usize::try_from(DIFF_BUDGET).unwrap_or(usize::MAX)
}

fn written(commit: PullRequestCommit, repository: &Repository) -> Commit {
    let sha = commit.sha.to_string();
    Commit {
        url: absolute(&commit.url).unwrap_or_else(|| repository.commit_url(&sha)),
        sha,
        message: commit.message,
        author_name: commit.author_name,
        author_login: commit.author_login,
        authored_at: commit.authored_at,
    }
}

fn thread(record: ReviewThreadRecord) -> Thread {
    let comments: Vec<ThreadComment> = record
        .comments
        .into_iter()
        .filter_map(|comment| {
            Some(ThreadComment {
                id: comment.database_id,
                author: comment.author,
                body: comment.body,
                url: absolute(&comment.url)?,
                created_at: comment.created_at,
            })
        })
        .collect();
    Thread {
        url: comments.first().map(|comment| comment.url.clone()),
        id: record.id,
        path: record.path,
        line: record.line.or(record.original_line),
        outdated: record.outdated,
        comments,
    }
}

#[cfg(test)]
mod tests;
