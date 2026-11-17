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
use tokio::time::Instant;
use tokio::time::timeout_at;

use super::BATCH;
use super::EVIDENCE_CONCURRENCY;
use super::Listing;
use super::PullRequest;
use super::Repository;
use super::origin::Origin;
use super::reassess;
use super::refresh_plan;
use crate::services::github::client::Github;
use crate::services::github::client::Guard;
use crate::services::github::client::sufficient;
use crate::services::github::fault::Fault;
use crate::services::github::throttle::CREDENTIAL_PERMITS;

type Verdict = (Origin, Result<PullRequest, Fault>);

const LATE: &str = "GitHub was not read in time to assess this pull request again.";

pub(super) struct Survey<'a> {
    github: Github,
    permits: &'a Semaphore,
    repository: &'a Repository,
    previous: Option<&'a Listing>,
    now: DateTime<Utc>,
    deadline: Instant,
    stop: AtomicBool,
}

impl<'a> Survey<'a> {
    pub(super) fn new(
        github: Github,
        permits: &'a Semaphore,
        repository: &'a Repository,
        previous: Option<&'a Listing>,
        now: DateTime<Utc>,
        patience: Duration,
        deadline: Instant,
    ) -> Self {
        Self {
            github: github.guarded(Guard::new(patience)),
            permits,
            repository,
            previous,
            now,
            deadline,
            stop: AtomicBool::new(false),
        }
    }

    /// The listing pages until GitHub has no more open pull requests to
    /// give, so the deadline is what ends a long one rather than a count.
    /// It is handed down because each page is bounded by it: a listing out
    /// of time keeps the pages it read and says it is not every one.
    pub(super) async fn listing(&self) -> Result<Listing, Fault> {
        let opened = self
            .github
            .open_pull_requests(self.permits, self.deadline)
            .await;
        match opened {
            Ok((repository, rows, complete)) => self.batched(&repository, rows, complete).await,
            Err(fault) if fault.halts() => Err(fault),
            Err(fault) => {
                tracing::warn!(
                    repository = %self.repository.name,
                    %fault,
                    "Listing open pull requests through REST after GitHub refused the GraphQL listing"
                );
                let (rows, complete) = self.github.open_pulls(self.deadline).await?;
                self.through_rest(rows, complete).await
            }
        }
    }

    /// Each batch is assessed as soon as its evidence arrives, so one slow
    /// batch holds up only its own rows.
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
        let paired = rows
            .iter()
            .zip(verdicts)
            .map(|(row, verdict)| (row, verdict.unwrap_or_else(late)));
        Ok(self.listed(paired, complete))
    }

    async fn batch(
        &self,
        repository: &RepositoryId,
        batch: &[(usize, &ListedPullRequest)],
    ) -> Result<Vec<(usize, Verdict)>, Fault> {
        let rows: Vec<&ListedPullRequest> = batch.iter().map(|(_, row)| *row).collect();
        let read = timeout_at(
            self.deadline,
            self.github
                .evidence(self.permits, repository, &rows, &self.stop),
        )
        .await;
        let read = match read {
            Ok(Ok(read)) => read,
            Ok(Err(fault)) if fault.halts() => return Err(fault),
            Ok(Err(fault)) => {
                tracing::warn!(
                    repository = %self.repository.name,
                    %fault,
                    "Reading pull requests through REST after their batch could not be read"
                );
                rows.iter().map(|_| None).collect()
            }
            Err(_) => return Ok(batch.iter().map(|(index, _)| (*index, late())).collect()),
        };
        let verdicts: Vec<_> = batch
            .iter()
            .zip(read)
            .map(|(&(index, row), evidence)| async move {
                (index, self.timely(self.verdict(row, evidence)).await)
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
                    None => self.timely(self.verdict(row, None)).await,
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

    async fn timely(&self, verdict: impl Future<Output = Verdict>) -> Verdict {
        timeout_at(self.deadline, verdict)
            .await
            .unwrap_or_else(|_| late())
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
                    &self.github,
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
        let (mut standing, mut dropped) = (0, 0);
        for (row, (origin, verdict)) in verdicts {
            match origin {
                Origin::Kept => kept += 1,
                Origin::Batch => batched += 1,
                Origin::FollowUp => followed += 1,
                Origin::Rest => rested += 1,
                Origin::Late => {}
            }
            let fault = match verdict {
                Ok(pull_request) => {
                    listing.pull_requests.push(pull_request);
                    continue;
                }
                Err(fault) => fault,
            };
            let number = row.reference.number().get();
            listing.complete = false;
            let last = self.last(number);
            match origin {
                Origin::Late if last.is_some() => standing += 1,
                Origin::Late => dropped += 1,
                _ => listing
                    .warnings
                    .push(format!("{}#{number}: {fault}", self.repository.name)),
            }
            listing.pull_requests.extend(last);
        }
        listing
            .warnings
            .extend(late_warnings(&self.repository.name, standing, dropped));
        tracing::info!(
            repository = %self.repository.name,
            kept,
            batched,
            followed,
            rested,
            late = standing + dropped,
            "Assessed open pull requests"
        );
        listing
    }

    fn last(&self, number: u64) -> Option<PullRequest> {
        self.previous
            .into_iter()
            .flat_map(|previous| previous.pull_requests.iter())
            .find(|kept| kept.number == number)
            .cloned()
    }
}

fn late_warnings(repository: &str, standing: usize, dropped: usize) -> Vec<String> {
    let mut warnings = Vec::new();
    match standing {
        0 => {}
        1 => warnings.push(format!(
            "{repository}: 1 pull request was not assessed again in time; its last verdict stands."
        )),
        count => warnings.push(format!(
            "{repository}: {count} pull requests were not assessed again in time; their last verdicts stand."
        )),
    }
    match dropped {
        0 => {}
        1 => warnings.push(format!(
            "{repository}: 1 pull request was not assessed in time, so it is left out."
        )),
        count => warnings.push(format!(
            "{repository}: {count} pull requests were not assessed in time, so they are left out."
        )),
    }
    warnings
}

fn late() -> Verdict {
    (Origin::Late, Err(Fault::upstream(LATE)))
}

fn halted(verdict: Verdict) -> Result<Verdict, Fault> {
    match verdict {
        (_, Err(fault)) if fault.halts() => Err(fault),
        verdict => Ok(verdict),
    }
}

#[cfg(test)]
mod tests;
