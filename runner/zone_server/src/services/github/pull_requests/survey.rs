use std::sync::atomic::AtomicBool;
use std::time::Duration;

use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::PullRequestEvidence;
use abnegate_vcs::RepositoryId;
use chrono::DateTime;
use chrono::Utc;
use futures::StreamExt;
use futures::stream;
use tokio::sync::Semaphore;

use super::BATCH;
use super::EVIDENCE_CONCURRENCY;
use super::EVIDENCE_LIMIT;
use super::Listing;
use super::PullRequest;
use super::Repository;
use super::origin::Origin;
use super::reassess;
use super::refresh_plan;
use crate::services::github::client::Github;
use crate::services::github::client::sufficient;
use crate::services::github::fault::Fault;
use crate::services::github::throttle::CREDENTIAL_PERMITS;

type Verdict = (Origin, Result<PullRequest, Fault>);

/// One read of a source's open pull requests, against the listing read
/// before it.
pub(super) struct Survey<'a> {
    pub(super) github: &'a Github,
    pub(super) permits: &'a Semaphore,
    pub(super) repository: &'a Repository,
    pub(super) previous: Option<&'a Listing>,
    pub(super) now: DateTime<Utc>,
    pub(super) limit: Duration,
    pub(super) stop: AtomicBool,
}

impl<'a> Survey<'a> {
    pub(super) fn new(
        github: &'a Github,
        permits: &'a Semaphore,
        repository: &'a Repository,
        previous: Option<&'a Listing>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            github,
            permits,
            repository,
            previous,
            now,
            limit: EVIDENCE_LIMIT,
            stop: AtomicBool::new(false),
        }
    }

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
                self.through_rest(rows, complete).await
            }
        }
    }

    /// Each batch of rows that are not current is assessed as soon as its
    /// evidence is read, [`EVIDENCE_CONCURRENCY`] batches at a time, so one
    /// slow batch holds up only its own rows.
    async fn batched(
        &self,
        repository: &RepositoryId,
        rows: Vec<ListedPullRequest>,
        complete: bool,
    ) -> Result<Listing, Fault> {
        let plan = refresh_plan(self.previous, &rows, self.now);
        let pending: Vec<(usize, &ListedPullRequest)> = rows
            .iter()
            .enumerate()
            .zip(&plan)
            .filter(|(_, kept)| kept.is_none())
            .map(|(entry, _)| entry)
            .collect();
        let mut verdicts: Vec<Option<Verdict>> = plan
            .iter()
            .map(|kept| kept.map(|kept| (Origin::Kept, Ok(kept.clone()))))
            .collect();
        let batches: Vec<_> = pending
            .chunks(BATCH)
            .map(|batch| self.batch(repository, batch))
            .collect();
        let mut batches = stream::iter(batches).buffer_unordered(EVIDENCE_CONCURRENCY);
        while let Some(judged) = batches.next().await {
            for (index, verdict) in judged? {
                verdicts[index] = Some(verdict);
            }
        }
        Ok(self.listed(rows.iter().zip(verdicts.into_iter().flatten()), complete))
    }

    /// The verdicts on one batch of rows. Its evidence is read under a
    /// permit let go before any of them takes one to read what the batch
    /// could not hold.
    async fn batch(
        &self,
        repository: &RepositoryId,
        batch: &[(usize, &ListedPullRequest)],
    ) -> Result<Vec<(usize, Verdict)>, Fault> {
        let rows: Vec<&ListedPullRequest> = batch.iter().map(|(_, row)| *row).collect();
        let read = self
            .github
            .evidence(self.permits, repository, &rows, self.limit, &self.stop)
            .await?;
        let verdicts: Vec<_> =
            batch
                .iter()
                .zip(read)
                .map(|(&(index, row), evidence)| async move {
                    (index, self.verdict(row, evidence).await)
                })
                .collect();
        let mut verdicts = stream::iter(verdicts).buffer_unordered(BATCH);
        let mut judged = Vec::with_capacity(batch.len());
        while let Some((index, verdict)) = verdicts.next().await {
            judged.push((index, halted(verdict)?));
        }
        Ok(judged)
    }

    async fn through_rest(
        &self,
        rows: Vec<ListedPullRequest>,
        complete: bool,
    ) -> Result<Listing, Fault> {
        let plan = refresh_plan(self.previous, &rows, self.now);
        let verdicts: Vec<_> = rows
            .iter()
            .zip(plan)
            .map(|(row, kept)| async move {
                match kept {
                    Some(kept) => (Origin::Kept, Ok(kept.clone())),
                    None => self.verdict(row, None).await,
                }
            })
            .collect();
        let mut verdicts = stream::iter(verdicts).buffered(CREDENTIAL_PERMITS);
        let mut judged = Vec::with_capacity(rows.len());
        while let Some(verdict) = verdicts.next().await {
            judged.push(halted(verdict)?);
        }
        Ok(self.listed(rows.iter().zip(judged), complete))
    }

    async fn verdict(
        &self,
        row: &ListedPullRequest,
        evidence: Option<PullRequestEvidence>,
    ) -> Verdict {
        match evidence {
            Some(evidence) => {
                let origin = match sufficient(row, &evidence) {
                    true => Origin::Batch,
                    false => Origin::FollowUp,
                };
                let assessed = self
                    .github
                    .assess_open(self.permits, row, evidence)
                    .await
                    .map(|assessed| {
                        PullRequest::assessed(assessed, self.repository, row, self.now)
                    });
                (origin, assessed)
            }
            None => (
                Origin::Rest,
                reassess(
                    self.github,
                    self.permits,
                    self.repository,
                    row,
                    None,
                    self.now,
                )
                .await,
            ),
        }
    }

    /// The listing the verdicts on each row add up to: a pull request that
    /// could not be assessed keeps the verdict the listing before held on it.
    fn listed<'r>(
        &self,
        verdicts: impl IntoIterator<Item = (&'r ListedPullRequest, Verdict)>,
        complete: bool,
    ) -> Listing {
        let mut listing = Listing {
            pull_requests: Vec::new(),
            complete,
            warnings: Vec::new(),
        };
        let (mut kept, mut batched, mut followed, mut rested) = (0, 0, 0, 0);
        for (row, (origin, verdict)) in verdicts {
            match origin {
                Origin::Kept => kept += 1,
                Origin::Batch => batched += 1,
                Origin::FollowUp => followed += 1,
                Origin::Rest => rested += 1,
            }
            match verdict {
                Ok(pull_request) => listing.pull_requests.push(pull_request),
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
        tracing::info!(
            repository = %self.repository.name,
            kept,
            batched,
            followed,
            rested,
            "Assessed open pull requests"
        );
        listing
    }
}

fn halted(verdict: Verdict) -> Result<Verdict, Fault> {
    match verdict {
        (_, Err(fault)) if fault.halts() => Err(fault),
        verdict => Ok(verdict),
    }
}

#[cfg(test)]
mod tests;
