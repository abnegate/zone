//! Release pipelines: what the workflows behind a published release did, and
//! how sure we are that we saw all of them.
//!
//! A release is not a workflow run. GitHub creates release-triggered runs some
//! time after the release is published, several workflows can answer the same
//! release, each of them can be rerun, and a listing can stop short of the
//! whole set. Every one of those is a way to look green while the evidence is
//! partial, so this module keeps the reading and the confidence in the reading
//! apart: `PipelineState` is what the runs reported, `Lookup` is whether they
//! were all seen, and a pipeline that fails either test is `Unknown` rather
//! than succeeded.
//!
//! Observations are merged rather than replaced. A later look that could not
//! reach the provider must not erase what an earlier one established, and a
//! run that was still going when it was last seen must not be frozen at that
//! state once a complete look no longer lists it.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

use super::readiness::CommitSha;

const RELEASE_EVENT: &str = "release";
const RUN_URL_ORIGIN: &str = "https://github.com";

/// The closed set of states one workflow run may be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PipelineState {
    ActionRequired,
    Cancelled,
    Failed,
    Neutral,
    Queued,
    Running,
    Skipped,
    Stale,
    Succeeded,
    TimedOut,
    Unknown,
}

/// The order a pipeline reports its runs in: the most consequential thing any
/// run is doing is what the pipeline as a whole is doing.
const PRECEDENCE: [PipelineState; 10] = [
    PipelineState::Failed,
    PipelineState::TimedOut,
    PipelineState::Cancelled,
    PipelineState::ActionRequired,
    PipelineState::Running,
    PipelineState::Queued,
    PipelineState::Stale,
    PipelineState::Neutral,
    PipelineState::Skipped,
    PipelineState::Unknown,
];

impl PipelineState {
    /// Read a provider status and conclusion as one state.
    ///
    /// The pair has to be coherent. A run that is still going has no
    /// conclusion, and a run that has finished has one; anything else is a
    /// report that does not describe a real run, so it collapses to unknown
    /// rather than being resolved in the provider's favour.
    pub fn normalize(status: &str, conclusion: Option<&str>) -> Self {
        let conclusion = conclusion.map(str::trim).filter(|value| !value.is_empty());
        match (status.trim(), conclusion) {
            ("in_progress", None) => Self::Running,
            ("pending" | "queued" | "requested" | "waiting", None) => Self::Queued,
            ("completed", Some(conclusion)) => match conclusion {
                "action_required" => Self::ActionRequired,
                "cancelled" => Self::Cancelled,
                "failure" | "startup_failure" => Self::Failed,
                "neutral" => Self::Neutral,
                "skipped" => Self::Skipped,
                "stale" => Self::Stale,
                "success" => Self::Succeeded,
                "timed_out" => Self::TimedOut,
                _ => Self::Unknown,
            },
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActionRequired => "action-required",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Neutral => "neutral",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Skipped => "skipped",
            Self::Stale => "stale",
            Self::Succeeded => "succeeded",
            Self::TimedOut => "timed-out",
            Self::Unknown => "unknown",
        }
    }

    /// Still going, so a later look can legitimately report something else.
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    /// Finished, and finished badly.
    pub fn failed(self) -> bool {
        matches!(
            self,
            Self::Failed | Self::TimedOut | Self::Cancelled | Self::ActionRequired
        )
    }

    /// Finished without saying whether the work was done.
    pub fn inconclusive(self) -> bool {
        matches!(
            self,
            Self::Neutral | Self::Skipped | Self::Stale | Self::Unknown
        )
    }
}

impl fmt::Display for PipelineState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How much of a release's pipeline was actually seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lookup {
    /// Every run for this release was retrieved.
    Complete,
    /// The provider was reached and had no run for this release yet.
    Pending,
    /// The provider could not be read, or returned a record that does not
    /// describe a run.
    Unavailable,
}

/// The release a pipeline belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseIdentity {
    pub repository: String,
    pub id: u64,
    pub tag: String,
    pub published_at: String,
    pub url: String,
}

impl ReleaseIdentity {
    /// The identity a pipeline observation is filed under. A retagged or
    /// republished release is a different pipeline, not an update to this one.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.repository, self.id, self.tag, self.published_at
        )
    }

    pub fn valid(&self) -> bool {
        !self.repository.trim().is_empty()
            && self.id > 0
            && !self.tag.trim().is_empty()
            && moment(&self.published_at).is_some()
    }

    /// When the release was published, ordered so that a timestamp that cannot
    /// be read sorts before every one that can.
    pub fn published(&self) -> i64 {
        observed_at(&self.published_at)
    }
}

/// One workflow run exactly as a provider reported it, before validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunEvidence {
    pub id: u64,
    pub workflow_id: u64,
    pub attempt: u32,
    pub name: String,
    pub path: String,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub head_branch: String,
    pub head_sha: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub updated_at: String,
    pub url: String,
    pub repository: String,
}

/// A workflow run whose identity has been proven to describe this release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipelineRun {
    pub id: u64,
    pub workflow_id: u64,
    pub attempt: u32,
    pub name: String,
    pub path: String,
    pub state: PipelineState,
    pub head_sha: CommitSha,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub updated_at: String,
    pub url: String,
}

impl RunEvidence {
    /// Whether this record is about that release at all.
    ///
    /// A repository-wide listing carries every release's runs, and one release
    /// saying nothing about another is routing, not incoherence. Only records
    /// that address a release are held to its evidence bar.
    pub fn addresses(&self, release: &ReleaseIdentity) -> bool {
        self.head_branch == release.tag
    }

    /// Accept the record only if every part of its identity says it is a
    /// release-triggered run of this release in this repository, published at
    /// the URL a run of that id would live at.
    pub fn normalize(&self, release: &ReleaseIdentity) -> Option<PipelineRun> {
        (self.id > 0
            && self.workflow_id > 0
            && self.attempt > 0
            && !self.name.trim().is_empty()
            && !self.path.trim().is_empty()
            && self.event == RELEASE_EVENT
            && self.addresses(release)
            && self.repository == release.repository
            && moment(&self.created_at).is_some()
            && moment(&self.updated_at).is_some()
            && self
                .started_at
                .as_deref()
                .is_none_or(|value| moment(value).is_some())
            && self.url == run_url(&release.repository, self.id))
        .then(|| CommitSha::parse(&self.head_sha))
        .flatten()
        .map(|head_sha| PipelineRun {
            id: self.id,
            workflow_id: self.workflow_id,
            attempt: self.attempt,
            name: self.name.trim().to_string(),
            path: self.path.clone(),
            state: PipelineState::normalize(&self.status, self.conclusion.as_deref()),
            head_sha,
            created_at: self.created_at.clone(),
            started_at: self.started_at.clone(),
            updated_at: self.updated_at.clone(),
            url: self.url.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineBlockerCode {
    LookupPending,
    LookupUnavailable,
    RunsAbsent,
    CommitUnverified,
    RunsFailed,
    RunsActive,
    RunsInconclusive,
}

/// One reason a release pipeline is not green, carrying the detail behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineBlocker {
    LookupPending,
    LookupUnavailable,
    RunsAbsent,
    CommitUnverified,
    RunsFailed { failed: u32, total: u32 },
    RunsActive { active: u32, total: u32 },
    RunsInconclusive { inconclusive: u32, total: u32 },
}

impl PipelineBlocker {
    pub fn code(&self) -> PipelineBlockerCode {
        match self {
            Self::LookupPending => PipelineBlockerCode::LookupPending,
            Self::LookupUnavailable => PipelineBlockerCode::LookupUnavailable,
            Self::RunsAbsent => PipelineBlockerCode::RunsAbsent,
            Self::CommitUnverified => PipelineBlockerCode::CommitUnverified,
            Self::RunsFailed { .. } => PipelineBlockerCode::RunsFailed,
            Self::RunsActive { .. } => PipelineBlockerCode::RunsActive,
            Self::RunsInconclusive { .. } => PipelineBlockerCode::RunsInconclusive,
        }
    }

    pub fn report(&self) -> PipelineBlockerReport {
        PipelineBlockerReport {
            code: self.code(),
            detail: self.to_string(),
        }
    }
}

impl fmt::Display for PipelineBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LookupPending => {
                formatter.write_str("No release workflow run has been created for this release yet")
            }
            Self::LookupUnavailable => {
                formatter.write_str("Release workflow runs could not be fully checked")
            }
            Self::RunsAbsent => {
                formatter.write_str("No release workflow run was observed for this release")
            }
            Self::CommitUnverified => formatter
                .write_str("Observed runs disagree on the commit this release was built from"),
            Self::RunsFailed { failed, total } => {
                write!(
                    formatter,
                    "{failed} of {total} release workflow runs failed"
                )
            }
            Self::RunsActive { active, total } => write!(
                formatter,
                "{active} of {total} release workflow runs are still running"
            ),
            Self::RunsInconclusive {
                inconclusive,
                total,
            } => write!(
                formatter,
                "{inconclusive} of {total} release workflow runs finished without a result"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PipelineBlockerReport {
    pub code: PipelineBlockerCode,
    pub detail: String,
}

/// Everything observed about one release's pipeline at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleasePipeline {
    pub release: ReleaseIdentity,
    pub lookup: Lookup,
    pub runs: Vec<PipelineRun>,
    pub checked_at: String,
}

impl ReleasePipeline {
    /// Build an observation from raw provider records, collapsing reruns and
    /// duplicate workflows and dropping anything whose identity does not hold.
    ///
    /// A dropped record makes the lookup unavailable: a listing we could only
    /// partly read is not a listing, and the runs it did carry are not proof
    /// that nothing else exists.
    pub fn observe(release: ReleaseIdentity, evidence: &[RunEvidence], checked_at: &str) -> Self {
        let mut runs = Vec::with_capacity(evidence.len());
        let mut dropped = false;
        for record in evidence.iter().filter(|record| record.addresses(&release)) {
            match record.normalize(&release) {
                Some(run) => runs.push(run),
                None => dropped = true,
            }
        }
        let runs = collapse(runs);
        let lookup = if dropped {
            Lookup::Unavailable
        } else if runs.is_empty() {
            Lookup::Pending
        } else {
            Lookup::Complete
        };
        Self {
            release,
            lookup,
            runs,
            checked_at: checked_at.to_string(),
        }
    }

    /// An observation that reached nothing at all.
    pub fn unavailable(release: ReleaseIdentity, checked_at: &str) -> Self {
        Self {
            release,
            lookup: Lookup::Unavailable,
            runs: Vec::new(),
            checked_at: checked_at.to_string(),
        }
    }

    /// The commit every observed run agrees it built, when they agree.
    pub fn commit(&self) -> Option<&CommitSha> {
        let first = &self.runs.first()?.head_sha;
        self.runs
            .iter()
            .all(|run| &run.head_sha == first)
            .then_some(first)
    }

    /// Whether the evidence behind this pipeline is whole enough to read.
    pub fn complete(&self) -> bool {
        self.lookup == Lookup::Complete && !self.runs.is_empty() && self.commit().is_some()
    }

    /// What the pipeline did, or unknown when that was never established.
    pub fn state(&self) -> PipelineState {
        if !self.complete() {
            return PipelineState::Unknown;
        }
        PRECEDENCE
            .into_iter()
            .find(|state| self.runs.iter().any(|run| run.state == *state))
            .unwrap_or(PipelineState::Succeeded)
    }

    /// Every reason this pipeline is not green. Empty exactly when it is.
    pub fn blockers(&self) -> Vec<PipelineBlocker> {
        let mut blockers = Vec::new();
        match self.lookup {
            Lookup::Pending => blockers.push(PipelineBlocker::LookupPending),
            Lookup::Unavailable => blockers.push(PipelineBlocker::LookupUnavailable),
            Lookup::Complete => {}
        }
        if self.runs.is_empty() {
            blockers.push(PipelineBlocker::RunsAbsent);
            return blockers;
        }
        if self.commit().is_none() {
            blockers.push(PipelineBlocker::CommitUnverified);
        }
        let total = count(self.runs.len());
        let failed = count(self.runs.iter().filter(|run| run.state.failed()).count());
        let active = count(self.runs.iter().filter(|run| run.state.active()).count());
        let inconclusive = count(
            self.runs
                .iter()
                .filter(|run| run.state.inconclusive())
                .count(),
        );
        if failed > 0 {
            blockers.push(PipelineBlocker::RunsFailed { failed, total });
        }
        if active > 0 {
            blockers.push(PipelineBlocker::RunsActive { active, total });
        }
        if inconclusive > 0 {
            blockers.push(PipelineBlocker::RunsInconclusive {
                inconclusive,
                total,
            });
        }
        blockers
    }

    pub fn reports(&self) -> Vec<PipelineBlockerReport> {
        self.blockers()
            .iter()
            .map(PipelineBlocker::report)
            .collect()
    }

    pub fn succeeded(&self) -> bool {
        self.state() == PipelineState::Succeeded
    }
}

/// Fold a later observation into an earlier one.
///
/// The later observation wins, with two exceptions the evidence demands. A
/// look that could not reach the provider keeps what was already established
/// instead of erasing it, including runs that were still going. And a look
/// that found nothing does not downgrade a lookup that had already completed.
pub fn merge(previous: Option<ReleasePipeline>, incoming: ReleasePipeline) -> ReleasePipeline {
    let Some(previous) = previous else {
        return incoming;
    };
    if observed_at(&incoming.checked_at) < observed_at(&previous.checked_at) {
        return previous;
    }
    let unavailable = incoming.lookup == Lookup::Unavailable;
    let runs = merge_runs(previous.runs, incoming.runs, unavailable);
    let lookup = if unavailable {
        if previous.lookup == Lookup::Pending {
            Lookup::Pending
        } else {
            Lookup::Unavailable
        }
    } else if incoming.lookup == Lookup::Pending
        && (previous.lookup == Lookup::Complete || !runs.is_empty())
    {
        Lookup::Complete
    } else {
        incoming.lookup
    };
    ReleasePipeline {
        lookup,
        runs,
        ..incoming
    }
}

/// Reduce a listing to one run per workflow: the newest attempt of each run,
/// then the newest run of each workflow.
pub fn collapse(runs: Vec<PipelineRun>) -> Vec<PipelineRun> {
    let mut by_run: BTreeMap<u64, PipelineRun> = BTreeMap::new();
    for run in runs {
        match by_run.remove(&run.id) {
            Some(seen) => by_run.insert(run.id, newer_attempt(seen, run)),
            None => by_run.insert(run.id, run),
        };
    }
    let mut by_workflow: BTreeMap<u64, PipelineRun> = BTreeMap::new();
    for run in by_run.into_values() {
        match by_workflow.remove(&run.workflow_id) {
            Some(seen) => by_workflow.insert(run.workflow_id, newer_run(seen, run)),
            None => by_workflow.insert(run.workflow_id, run),
        };
    }
    sorted(by_workflow.into_values().collect())
}

fn merge_runs(
    previous: Vec<PipelineRun>,
    incoming: Vec<PipelineRun>,
    preserve_active: bool,
) -> Vec<PipelineRun> {
    let mut by_workflow: BTreeMap<u64, PipelineRun> = incoming
        .into_iter()
        .map(|run| (run.workflow_id, run))
        .collect();
    for run in previous {
        if !preserve_active && run.state.active() {
            continue;
        }
        match by_workflow.remove(&run.workflow_id) {
            Some(current) if current.id == run.id => {
                by_workflow.insert(run.workflow_id, newer_attempt(run, current))
            }
            Some(current) => by_workflow.insert(run.workflow_id, newer_run(run, current)),
            None => by_workflow.insert(run.workflow_id, run),
        };
    }
    sorted(by_workflow.into_values().collect())
}

fn sorted(mut runs: Vec<PipelineRun>) -> Vec<PipelineRun> {
    runs.sort_by(|left, right| {
        observed_at(&right.created_at)
            .cmp(&observed_at(&left.created_at))
            .then(observed_at(&right.updated_at).cmp(&observed_at(&left.updated_at)))
            .then(right.attempt.cmp(&left.attempt))
            .then(right.id.cmp(&left.id))
    });
    runs
}

/// The later of two attempts at the same run.
fn newer_attempt(left: PipelineRun, right: PipelineRun) -> PipelineRun {
    let newer = right
        .attempt
        .cmp(&left.attempt)
        .then(observed_at(&right.updated_at).cmp(&observed_at(&left.updated_at)))
        .then(right.id.cmp(&left.id));
    if newer.is_lt() { left } else { right }
}

/// The later of two runs of the same workflow.
fn newer_run(left: PipelineRun, right: PipelineRun) -> PipelineRun {
    let newer = observed_at(&right.created_at)
        .cmp(&observed_at(&left.created_at))
        .then(observed_at(&right.updated_at).cmp(&observed_at(&left.updated_at)))
        .then(right.id.cmp(&left.id));
    if newer.is_lt() { left } else { right }
}

/// The canonical page a workflow run of this id lives at. A record that names
/// anything else is not a record of that run.
pub fn run_url(repository: &str, id: u64) -> String {
    format!("{RUN_URL_ORIGIN}/{repository}/actions/runs/{id}")
}

fn moment(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|moment| moment.timestamp_millis())
}

/// An unreadable timestamp orders before every readable one, so it can never
/// win a tie-break or overwrite an observation that carried a real clock.
fn observed_at(value: &str) -> i64 {
    moment(value).unwrap_or(i64::MIN)
}

fn count(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPOSITORY: &str = "owner/repository";
    const BUILT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn release() -> ReleaseIdentity {
        ReleaseIdentity {
            repository: REPOSITORY.into(),
            id: 90,
            tag: "v1.4.0".into(),
            published_at: "2026-09-05T00:00:00Z".into(),
            url: "https://github.com/owner/repository/releases/tag/v1.4.0".into(),
        }
    }

    fn evidence(id: u64, workflow_id: u64, status: &str, conclusion: Option<&str>) -> RunEvidence {
        RunEvidence {
            id,
            workflow_id,
            attempt: 1,
            name: "Publish".into(),
            path: ".github/workflows/publish.yml".into(),
            event: RELEASE_EVENT.into(),
            status: status.into(),
            conclusion: conclusion.map(str::to_string),
            head_branch: "v1.4.0".into(),
            head_sha: BUILT.into(),
            created_at: "2026-09-05T00:01:00Z".into(),
            started_at: Some("2026-09-05T00:01:10Z".into()),
            updated_at: "2026-09-05T00:04:00Z".into(),
            url: run_url(REPOSITORY, id),
            repository: REPOSITORY.into(),
        }
    }

    fn succeeded(id: u64, workflow_id: u64) -> RunEvidence {
        evidence(id, workflow_id, "completed", Some("success"))
    }

    fn observed(evidence: &[RunEvidence]) -> ReleasePipeline {
        ReleasePipeline::observe(release(), evidence, "2026-09-05T00:05:00Z")
    }

    #[test]
    fn a_status_and_conclusion_pair_reads_as_one_state() {
        for (status, conclusion, expected) in [
            ("in_progress", None, PipelineState::Running),
            ("queued", None, PipelineState::Queued),
            ("pending", None, PipelineState::Queued),
            ("requested", None, PipelineState::Queued),
            ("waiting", None, PipelineState::Queued),
            (
                "completed",
                Some("action_required"),
                PipelineState::ActionRequired,
            ),
            ("completed", Some("cancelled"), PipelineState::Cancelled),
            ("completed", Some("failure"), PipelineState::Failed),
            ("completed", Some("startup_failure"), PipelineState::Failed),
            ("completed", Some("neutral"), PipelineState::Neutral),
            ("completed", Some("skipped"), PipelineState::Skipped),
            ("completed", Some("stale"), PipelineState::Stale),
            ("completed", Some("success"), PipelineState::Succeeded),
            ("completed", Some("timed_out"), PipelineState::TimedOut),
        ] {
            assert_eq!(
                PipelineState::normalize(status, conclusion),
                expected,
                "{status}/{conclusion:?}"
            );
        }
    }

    #[test]
    fn an_incoherent_status_and_conclusion_pair_collapses_to_unknown() {
        for (status, conclusion) in [
            ("in_progress", Some("success")),
            ("queued", Some("failure")),
            ("completed", None),
            ("completed", Some("")),
            ("completed", Some("teleported")),
            ("half_done", None),
            ("", None),
        ] {
            assert_eq!(
                PipelineState::normalize(status, conclusion),
                PipelineState::Unknown,
                "{status}/{conclusion:?} must not be resolved in the provider's favour"
            );
        }
    }

    #[test]
    fn a_run_record_is_only_accepted_when_its_whole_identity_holds() {
        let release = release();
        assert!(succeeded(1, 1).normalize(&release).is_some());
        for (label, record) in [
            (
                "zero id",
                RunEvidence {
                    id: 0,
                    ..succeeded(1, 1)
                },
            ),
            (
                "zero workflow",
                RunEvidence {
                    workflow_id: 0,
                    ..succeeded(1, 1)
                },
            ),
            (
                "zero attempt",
                RunEvidence {
                    attempt: 0,
                    ..succeeded(1, 1)
                },
            ),
            (
                "blank name",
                RunEvidence {
                    name: "  ".into(),
                    ..succeeded(1, 1)
                },
            ),
            (
                "another event",
                RunEvidence {
                    event: "push".into(),
                    ..succeeded(1, 1)
                },
            ),
            (
                "another tag",
                RunEvidence {
                    head_branch: "v1.3.0".into(),
                    ..succeeded(1, 1)
                },
            ),
            (
                "another repository",
                RunEvidence {
                    repository: "owner/other".into(),
                    ..succeeded(1, 1)
                },
            ),
            (
                "unreadable commit",
                RunEvidence {
                    head_sha: "abc".into(),
                    ..succeeded(1, 1)
                },
            ),
            (
                "unreadable date",
                RunEvidence {
                    created_at: "yesterday".into(),
                    ..succeeded(1, 1)
                },
            ),
            (
                "another run's url",
                RunEvidence {
                    url: run_url(REPOSITORY, 999),
                    ..succeeded(1, 1)
                },
            ),
            (
                "another host",
                RunEvidence {
                    url: "https://github.example/owner/repository/actions/runs/1".into(),
                    ..succeeded(1, 1)
                },
            ),
        ] {
            assert!(record.normalize(&release).is_none(), "{label}");
        }
    }

    #[test]
    fn a_dropped_record_makes_the_whole_lookup_unavailable() {
        let pipeline = observed(&[
            succeeded(1, 1),
            RunEvidence {
                event: "push".into(),
                ..succeeded(2, 2)
            },
        ]);
        assert_eq!(pipeline.lookup, Lookup::Unavailable);
        assert_eq!(pipeline.runs.len(), 1);
        assert!(!pipeline.succeeded());
        assert_eq!(pipeline.state(), PipelineState::Unknown);
        assert_eq!(
            pipeline.reports()[0].detail,
            "Release workflow runs could not be fully checked"
        );
    }

    #[test]
    fn a_release_with_no_run_is_pending_and_never_green() {
        let pipeline = observed(&[]);
        assert_eq!(pipeline.lookup, Lookup::Pending);
        assert_eq!(pipeline.state(), PipelineState::Unknown);
        assert!(!pipeline.succeeded());
        assert_eq!(
            pipeline
                .reports()
                .into_iter()
                .map(|report| report.code)
                .collect::<Vec<_>>(),
            vec![
                PipelineBlockerCode::LookupPending,
                PipelineBlockerCode::RunsAbsent
            ]
        );
    }

    #[test]
    fn runs_that_disagree_on_the_commit_are_never_green() {
        let pipeline = observed(&[
            succeeded(1, 1),
            RunEvidence {
                head_sha: OTHER.into(),
                ..succeeded(2, 2)
            },
        ]);
        assert_eq!(pipeline.lookup, Lookup::Complete);
        assert!(pipeline.commit().is_none());
        assert_eq!(pipeline.state(), PipelineState::Unknown);
        assert_eq!(
            pipeline.reports(),
            vec![PipelineBlockerReport {
                code: PipelineBlockerCode::CommitUnverified,
                detail: "Observed runs disagree on the commit this release was built from".into(),
            }]
        );
    }

    #[test]
    fn a_complete_lookup_of_successful_runs_is_green() {
        let pipeline = observed(&[succeeded(1, 1), succeeded(2, 2)]);
        assert_eq!(pipeline.lookup, Lookup::Complete);
        assert_eq!(pipeline.state(), PipelineState::Succeeded);
        assert!(pipeline.succeeded());
        assert!(pipeline.blockers().is_empty());
        assert_eq!(pipeline.commit(), CommitSha::parse(BUILT).as_ref());
        assert_eq!(pipeline.commit().unwrap().short(), "aaaaaaa");
    }

    #[test]
    fn the_most_consequential_run_names_the_pipeline() {
        for (status, conclusion, expected, code) in [
            (
                "completed",
                Some("failure"),
                PipelineState::Failed,
                PipelineBlockerCode::RunsFailed,
            ),
            (
                "completed",
                Some("timed_out"),
                PipelineState::TimedOut,
                PipelineBlockerCode::RunsFailed,
            ),
            (
                "in_progress",
                None,
                PipelineState::Running,
                PipelineBlockerCode::RunsActive,
            ),
            (
                "queued",
                None,
                PipelineState::Queued,
                PipelineBlockerCode::RunsActive,
            ),
            (
                "completed",
                Some("skipped"),
                PipelineState::Skipped,
                PipelineBlockerCode::RunsInconclusive,
            ),
            (
                "completed",
                Some("teleported"),
                PipelineState::Unknown,
                PipelineBlockerCode::RunsInconclusive,
            ),
        ] {
            let pipeline = observed(&[succeeded(1, 1), evidence(2, 2, status, conclusion)]);
            assert_eq!(pipeline.state(), expected, "{status}/{conclusion:?}");
            assert!(!pipeline.succeeded());
            assert_eq!(pipeline.reports()[0].code, code);
            assert!(
                pipeline.reports()[0].detail.contains("1 of 2"),
                "{}",
                pipeline.reports()[0].detail
            );
        }
    }

    #[test]
    fn succeeding_is_exactly_the_absence_of_blockers() {
        let states = [
            ("completed", Some("success")),
            ("completed", Some("failure")),
            ("completed", Some("skipped")),
            ("in_progress", None),
            ("completed", Some("teleported")),
        ];
        let mut green = 0;
        for (status, conclusion) in states {
            for commit in [BUILT, OTHER] {
                for dropped in [false, true] {
                    let mut records = vec![
                        succeeded(1, 1),
                        RunEvidence {
                            head_sha: commit.into(),
                            ..evidence(2, 2, status, conclusion)
                        },
                    ];
                    if dropped {
                        records.push(RunEvidence {
                            event: "push".into(),
                            ..succeeded(3, 3)
                        });
                    }
                    let pipeline = observed(&records);
                    assert_eq!(
                        pipeline.succeeded(),
                        pipeline.blockers().is_empty(),
                        "state and blockers disagree for {status}/{conclusion:?} commit={commit} dropped={dropped}"
                    );
                    green += usize::from(pipeline.succeeded());
                }
            }
        }
        assert_eq!(
            green, 1,
            "exactly one combination describes a green release"
        );
    }

    #[test]
    fn a_rerun_replaces_the_attempt_it_supersedes() {
        let pipeline = observed(&[
            RunEvidence {
                attempt: 1,
                status: "completed".into(),
                conclusion: Some("failure".into()),
                ..succeeded(1, 1)
            },
            RunEvidence {
                attempt: 2,
                updated_at: "2026-09-05T00:09:00Z".into(),
                ..succeeded(1, 1)
            },
        ]);
        assert_eq!(pipeline.runs.len(), 1);
        assert_eq!(pipeline.runs[0].attempt, 2);
        assert!(pipeline.succeeded());
    }

    #[test]
    fn a_newer_run_of_the_same_workflow_replaces_the_older_one() {
        let pipeline = observed(&[
            RunEvidence {
                status: "completed".into(),
                conclusion: Some("failure".into()),
                ..succeeded(1, 7)
            },
            RunEvidence {
                created_at: "2026-09-05T00:03:00Z".into(),
                updated_at: "2026-09-05T00:06:00Z".into(),
                ..succeeded(2, 7)
            },
        ]);
        assert_eq!(pipeline.runs.len(), 1);
        assert_eq!(pipeline.runs[0].id, 2);
        assert!(pipeline.succeeded());
    }

    #[test]
    fn merging_two_observations_keeps_the_later_reading_of_each_workflow() {
        let earlier = ReleasePipeline::observe(
            release(),
            &[evidence(1, 1, "in_progress", None), succeeded(2, 2)],
            "2026-09-05T00:05:00Z",
        );
        let later = ReleasePipeline::observe(
            release(),
            &[RunEvidence {
                updated_at: "2026-09-05T00:08:00Z".into(),
                ..succeeded(1, 1)
            }],
            "2026-09-05T00:09:00Z",
        );

        let merged = merge(Some(earlier.clone()), later);
        assert_eq!(merged.lookup, Lookup::Complete);
        assert_eq!(merged.checked_at, "2026-09-05T00:09:00Z");
        assert_eq!(
            merged.runs.len(),
            2,
            "a workflow the later look did not list is kept"
        );
        assert!(merged.succeeded());
        assert_eq!(
            merged.runs.iter().find(|run| run.id == 1).unwrap().state,
            PipelineState::Succeeded,
            "the later reading of a workflow replaces the earlier one"
        );
    }

    #[test]
    fn an_unavailable_look_keeps_what_was_already_established() {
        let earlier = observed(&[evidence(1, 1, "in_progress", None), succeeded(2, 2)]);
        let merged = merge(
            Some(earlier),
            ReleasePipeline::unavailable(release(), "2026-09-05T00:09:00Z"),
        );
        assert_eq!(merged.lookup, Lookup::Unavailable);
        assert_eq!(
            merged.runs.len(),
            2,
            "a failed look must not erase evidence"
        );
        assert!(
            merged
                .runs
                .iter()
                .any(|run| run.state == PipelineState::Running),
            "a run still going stays going when the later look reached nothing"
        );
        assert!(!merged.succeeded());
    }

    #[test]
    fn a_complete_look_retires_a_run_that_is_no_longer_listed() {
        let earlier = observed(&[evidence(1, 1, "in_progress", None), succeeded(2, 2)]);
        let merged = merge(
            Some(earlier),
            ReleasePipeline::observe(release(), &[succeeded(2, 2)], "2026-09-05T00:09:00Z"),
        );
        assert_eq!(merged.runs.len(), 1);
        assert_eq!(merged.runs[0].id, 2);
        assert!(merged.succeeded());
    }

    #[test]
    fn an_empty_later_look_does_not_downgrade_a_completed_one() {
        let earlier = observed(&[succeeded(1, 1)]);
        let merged = merge(
            Some(earlier),
            ReleasePipeline::observe(release(), &[], "2026-09-05T00:09:00Z"),
        );
        assert_eq!(merged.lookup, Lookup::Complete);
        assert_eq!(merged.runs.len(), 1);
    }

    #[test]
    fn an_older_or_unreadable_observation_never_overwrites_a_newer_one() {
        let newer = observed(&[succeeded(1, 1)]);
        for checked_at in ["2026-09-05T00:04:00Z", "yesterday"] {
            let merged = merge(
                Some(newer.clone()),
                ReleasePipeline::unavailable(release(), checked_at),
            );
            assert_eq!(merged.checked_at, "2026-09-05T00:05:00Z", "{checked_at}");
            assert_eq!(merged.lookup, Lookup::Complete);
        }
        assert_eq!(merge(None, newer.clone()), newer);
    }

    #[test]
    fn a_release_identity_is_the_whole_publication() {
        let release = release();
        assert!(release.valid());
        assert_eq!(
            release.key(),
            "owner/repository:90:v1.4.0:2026-09-05T00:00:00Z"
        );
        assert_ne!(
            ReleaseIdentity {
                tag: "v1.4.1".into(),
                ..release.clone()
            }
            .key(),
            release.key(),
            "a retagged release is a different pipeline"
        );
        for invalid in [
            ReleaseIdentity {
                id: 0,
                ..release.clone()
            },
            ReleaseIdentity {
                tag: " ".into(),
                ..release.clone()
            },
            ReleaseIdentity {
                repository: "".into(),
                ..release.clone()
            },
            ReleaseIdentity {
                published_at: "soon".into(),
                ..release.clone()
            },
        ] {
            assert!(!invalid.valid(), "{invalid:?}");
        }
    }

    #[test]
    fn runs_are_ordered_newest_first() {
        let pipeline = observed(&[
            RunEvidence {
                created_at: "2026-09-05T00:01:00Z".into(),
                ..succeeded(1, 1)
            },
            RunEvidence {
                created_at: "2026-09-05T00:03:00Z".into(),
                ..succeeded(2, 2)
            },
            RunEvidence {
                created_at: "2026-09-05T00:02:00Z".into(),
                ..succeeded(3, 3)
            },
        ]);
        assert_eq!(
            pipeline.runs.iter().map(|run| run.id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    #[test]
    fn a_pipeline_serialises_for_a_tool_result() {
        let pipeline = observed(&[succeeded(1, 1)]);
        let encoded = serde_json::to_value(&pipeline).expect("pipeline serialises");
        assert_eq!(encoded["lookup"], "complete");
        assert_eq!(encoded["runs"][0]["state"], "succeeded");
        assert_eq!(encoded["release"]["tag"], "v1.4.0");
        assert_eq!(
            serde_json::to_value(PipelineState::ActionRequired).unwrap(),
            "action-required"
        );
        assert_eq!(PipelineState::TimedOut.to_string(), "timed-out");
    }

    #[test]
    fn another_releases_run_is_routed_away_rather_than_dropped() {
        let elsewhere = RunEvidence {
            head_branch: "v1.3.0".into(),
            ..succeeded(9, 90)
        };
        assert!(!elsewhere.addresses(&release()));

        let pipeline = observed(&[succeeded(1, 1), elsewhere]);
        assert_eq!(
            pipeline.lookup,
            Lookup::Complete,
            "a repository-wide listing carries other releases; that is not incomplete evidence"
        );
        assert_eq!(pipeline.runs.len(), 1);
        assert!(pipeline.succeeded());
    }
}
