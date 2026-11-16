use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::PullRequestEvidence;
use abnegate_vcs::RepositoryId;
use chrono::DateTime;
use chrono::Utc;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use tokio::sync::Semaphore;

use super::BATCH;
use super::EVIDENCE_CONCURRENCY;
use super::Listing;
use super::PullRequest;
use super::Repository;
use super::reassess;
use super::refresh_plan;
use crate::services::github::client::Github;
use crate::services::github::fault::Fault;
use crate::services::github::throttle::CREDENTIAL_PERMITS;

/// One read of a source's open pull requests, against the listing read
/// before it.
pub(super) struct Survey<'a> {
    pub(super) github: &'a Github,
    pub(super) permits: &'a Semaphore,
    pub(super) repository: &'a Repository,
    pub(super) previous: Option<&'a Listing>,
    pub(super) now: DateTime<Utc>,
}

impl<'a> Survey<'a> {
    /// Up to `limit` open pull requests, listed through GraphQL with the
    /// evidence on each one not current read in batches, or through REST
    /// when GitHub refuses the GraphQL listing short of a halt.
    pub(super) async fn listing(&self, limit: u32) -> Result<Listing, Fault> {
        match self.github.open_pull_requests(self.permits, limit).await {
            Ok((repository, rows, complete)) => self.batched(&repository, rows, complete).await,
            Err(fault) if fault.halts() => Err(fault),
            Err(fault) => {
                tracing::warn!(
                    repository = %self.repository.name,
                    %fault,
                    "Listing open pull requests through REST after GitHub refused the GraphQL listing"
                );
                let (rows, complete) = self.github.open_pulls(limit).await?;
                self.rested(rows, complete).await
            }
        }
    }

    async fn batched(
        &self,
        repository: &RepositoryId,
        rows: Vec<ListedPullRequest>,
        complete: bool,
    ) -> Result<Listing, Fault> {
        let plan = refresh_plan(self.previous, &rows, self.now);
        let pending: Vec<&ListedPullRequest> = rows
            .iter()
            .zip(&plan)
            .filter(|(_, kept)| kept.is_none())
            .map(|(row, _)| row)
            .collect();
        let reads: Vec<_> = pending
            .chunks(BATCH)
            .map(|batch| self.github.evidence(self.permits, repository, batch))
            .collect();
        let read: Vec<Vec<Option<PullRequestEvidence>>> = stream::iter(reads)
            .buffered(EVIDENCE_CONCURRENCY)
            .try_collect()
            .await?;
        let mut read = read.into_iter().flatten();
        let surveys: Vec<_> = rows
            .iter()
            .zip(plan)
            .map(|(row, kept)| {
                let evidence = match kept {
                    Some(_) => None,
                    None => read.next().flatten(),
                };
                self.assessed(row, kept, evidence)
            })
            .collect();
        self.gathered(surveys, &rows, complete).await
    }

    async fn rested(&self, rows: Vec<ListedPullRequest>, complete: bool) -> Result<Listing, Fault> {
        let plan = refresh_plan(self.previous, &rows, self.now);
        let surveys: Vec<_> = rows
            .iter()
            .zip(plan)
            .map(|(row, kept)| self.assessed(row, kept, None))
            .collect();
        self.gathered(surveys, &rows, complete).await
    }

    async fn assessed(
        &self,
        row: &ListedPullRequest,
        kept: Option<&'a PullRequest>,
        evidence: Option<PullRequestEvidence>,
    ) -> Result<PullRequest, Fault> {
        match (kept, evidence) {
            (Some(kept), _) => Ok(kept.clone()),
            (None, Some(evidence)) => self
                .github
                .assess_open(self.permits, row, evidence)
                .await
                .map(|assessed| PullRequest::assessed(assessed, self.repository, row, self.now)),
            (None, None) => {
                reassess(
                    self.github,
                    self.permits,
                    self.repository,
                    row,
                    None,
                    self.now,
                )
                .await
            }
        }
    }

    /// The listing `surveys` of `rows` add up to, run [`CREDENTIAL_PERMITS`]
    /// at a time: a halt stops it, and a pull request that could not be
    /// assessed keeps the verdict the listing before held on it.
    async fn gathered<Assessing>(
        &self,
        surveys: Vec<Assessing>,
        rows: &[ListedPullRequest],
        complete: bool,
    ) -> Result<Listing, Fault>
    where
        Assessing: Future<Output = Result<PullRequest, Fault>>,
    {
        let mut assessed = stream::iter(surveys)
            .buffered(CREDENTIAL_PERMITS)
            .zip(stream::iter(rows));
        let mut listing = Listing {
            pull_requests: Vec::with_capacity(rows.len()),
            complete,
            warnings: Vec::new(),
        };
        while let Some((result, row)) = assessed.next().await {
            match result {
                Ok(pull_request) => listing.pull_requests.push(pull_request),
                Err(fault) if fault.halts() => return Err(fault),
                Err(fault) => {
                    let number = row.reference.number().get();
                    listing.complete = false;
                    listing
                        .warnings
                        .push(format!("{}#{number}: {fault}", self.repository.name));
                    listing.pull_requests.extend(
                        self.previous
                            .into_iter()
                            .flat_map(|previous| previous.pull_requests.iter())
                            .find(|kept| kept.number == number)
                            .cloned(),
                    );
                }
            }
        }
        Ok(listing)
    }
}

#[cfg(test)]
mod tests;
