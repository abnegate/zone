use std::num::NonZeroU32;
use std::num::NonZeroU64;

use abnegate_vcs::ChangedFile;
use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::Excerpt;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::PullRequestCommit;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestReference;
use abnegate_vcs::ReviewThreadRecord;
use abnegate_vcs::StateFilter;
use chrono::DateTime;
use chrono::Utc;

use super::Commit;
use super::DIFF_BUDGET;
use super::LOG_TAIL;
use super::Log;
use super::Repository;
use super::Thread;
use super::ThreadComment;
use super::absolute;
use crate::services::github::client::Github;
use crate::services::github::client::PER_PAGE;
use crate::services::github::client::described;
use crate::services::github::diff;
use crate::services::github::diff::Diff;
use crate::services::github::diff::Entry;

pub(super) const NOT_ACCEPTABLE: u16 = 406;
const COMMIT_PAGES: u32 = 3;

impl Github {
    pub(crate) async fn open_pulls(
        &self,
        limit: u32,
    ) -> Result<(Vec<ListedPullRequest>, bool), String> {
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
                .await
                .map_err(described)?;
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

    pub(crate) async fn pull_commits(
        &self,
        reference: &PullRequestReference,
        counted: u32,
        repository: &Repository,
    ) -> Result<(Vec<Commit>, bool), String> {
        let mut commits = Vec::new();
        let mut page = NonZeroU32::MIN;
        loop {
            let listed = self
                .service()
                .list_commits(reference, self.token(), page, PER_PAGE)
                .await
                .map_err(described)?;
            commits.extend(
                listed
                    .items
                    .into_iter()
                    .map(|commit| written(commit, repository)),
            );
            match listed.next {
                Some(next) if next.get() <= COMMIT_PAGES => page = next,
                next => {
                    let counted = usize::try_from(counted).unwrap_or(usize::MAX);
                    let complete = next.is_none() && commits.len() >= counted;
                    return Ok((commits, complete));
                }
            }
        }
    }

    pub(crate) async fn unresolved_threads(&self, number: u64) -> Result<Vec<Thread>, String> {
        Ok(self
            .threads(number)
            .await?
            .items
            .into_iter()
            .filter(|thread| !thread.resolved && !thread.comments.is_empty())
            .map(thread)
            .collect())
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
                Ok(diff::per_file(files.iter().map(entry), revision))
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
    ) -> Result<Log, String> {
        let job = self
            .service()
            .fetch_workflow_job(self.repository(), self.token(), job_id)
            .await
            .map_err(described)?;
        let run_id = self.verify_job(&job, job_id.get(), head)?;
        let downloaded = self
            .service()
            .fetch_job_log(
                self.repository(),
                self.token(),
                job_id,
                usize::try_from(self.log_bytes()).unwrap_or(usize::MAX),
            )
            .await
            .map_err(described)?;
        let text = String::from_utf8_lossy(&downloaded.content);
        let (kept, cut) = tail(&text, LOG_TAIL);
        Ok(Log {
            job_id: job_id.get(),
            run_id: Some(run_id),
            head: head.to_string(),
            text: kept.to_string(),
            truncated: downloaded.truncated || cut,
            fetched_at,
        })
    }
}

pub(super) fn tail(text: &str, limit: u64) -> (&str, bool) {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    if text.len() <= limit {
        return (text, false);
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (&text[start..], true)
}

fn budget() -> usize {
    usize::try_from(DIFF_BUDGET).unwrap_or(usize::MAX)
}

fn entry(file: &ChangedFile) -> Entry<'_> {
    Entry {
        path: &file.filename,
        previous_path: file.previous_filename.as_deref(),
        status: file.status.into(),
        additions: file.additions,
        deletions: file.deletions,
        patch: file.patch.as_deref(),
    }
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
