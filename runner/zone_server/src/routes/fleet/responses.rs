use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::fleet::Labels;
use zone_enterprise::{Edition, Status};

use crate::db::fleet::labels::{Available, Stored};
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::mode::Mode;
use crate::db::fleet::policy::Policy;
use crate::db::fleet::runners::Listed;
use crate::db::fleet::shortfall::Shortfall;
use crate::db::fleet::state::State;
use crate::routes::policy::access::Access;
use crate::routes::policy::standing::Standing;
use crate::services::fleet::refusal;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetPolicyResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    pub minimum_version: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl FleetPolicyResponse {
    pub fn new(standing: Standing, policy: &Policy) -> Self {
        Self {
            entitled: standing.entitled,
            enforced: standing.enforced,
            edition: standing.edition,
            status: standing.status,
            editable: standing.editable(Access::Write),
            minimum_version: policy.minimum.as_ref().map(Minimum::to_string),
            updated_at: policy.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnersResponse {
    pub editable: bool,
    pub enforced: bool,
    /// The runner this server process registered as, which served this response.
    pub current: Option<Uuid>,
    pub runners: Vec<RunnerResponse>,
}

impl RunnersResponse {
    pub fn new(
        standing: Standing,
        listed: &[Listed],
        minimum: Option<&Minimum>,
        current: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            editable: standing.editable(Access::Write),
            enforced: standing.enforced,
            current,
            runners: listed
                .iter()
                .map(|listed| RunnerResponse::new(listed, minimum, current, now))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerResponse {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub labels: Vec<String>,
    pub state: State,
    pub connected: bool,
    pub outdated: bool,
    /// Connected and able to take unattended work under the fleet policy, labels aside.
    pub serving: bool,
    /// Why the fleet policy keeps this runner from unattended work, labels aside.
    pub code: Option<&'static str>,
    pub current: bool,
    pub runs: i64,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub state_changed_at: Option<DateTime<Utc>>,
}

impl RunnerResponse {
    pub fn new(
        listed: &Listed,
        minimum: Option<&Minimum>,
        current: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Self {
        let runner = &listed.runner;
        let connected = runner.connected(now);
        let shortfall = Shortfall::of(
            Mode::Unattended,
            runner.state,
            &runner.version,
            minimum,
            Labels::default(),
        );
        Self {
            id: runner.id,
            name: runner.name.clone(),
            version: runner.version.to_string(),
            labels: runner.labels.to_vec(),
            state: runner.state,
            connected,
            outdated: minimum.is_some_and(|minimum| !minimum.admits(&runner.version)),
            serving: connected && shortfall.is_none(),
            code: shortfall.map(|shortfall| refusal::of(shortfall).code()),
            current: current == Some(runner.id),
            runs: listed.runs,
            started_at: runner.started_at,
            last_seen_at: runner.last_seen_at,
            state_changed_at: runner.state_changed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AvailableLabel {
    pub label: String,
    pub runners: i64,
}

impl From<Available> for AvailableLabel {
    fn from(available: Available) -> Self {
        Self {
            label: available.label,
            runners: available.runners,
        }
    }
}

/// Carries label names and counts only, never a runner's name, id, version or runs, since
/// organization members see it and the fleet belongs to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrganizationRunnerLabelsResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    pub labels: Vec<String>,
    pub available: Vec<AvailableLabel>,
    pub matching: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

impl OrganizationRunnerLabelsResponse {
    pub fn new(
        standing: Standing,
        access: Access,
        stored: Option<&Stored>,
        available: Vec<AvailableLabel>,
        matching: i64,
    ) -> Self {
        Self {
            entitled: standing.entitled,
            enforced: standing.enforced,
            edition: standing.edition,
            status: standing.status,
            editable: standing.editable(access),
            labels: requirement(stored).to_vec(),
            available,
            matching,
            updated_at: stored.map(|stored| stored.updated_at),
        }
    }
}

/// Carries label names and counts only, as [`OrganizationRunnerLabelsResponse`] does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceRunnerLabelsResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    pub organization: Vec<String>,
    pub workspace: Vec<String>,
    pub effective: Vec<String>,
    pub available: Vec<AvailableLabel>,
    pub matching: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

impl WorkspaceRunnerLabelsResponse {
    pub fn new(
        standing: Standing,
        access: Access,
        organization: Option<&Stored>,
        workspace: Option<&Stored>,
        available: Vec<AvailableLabel>,
        matching: i64,
    ) -> Self {
        Self {
            entitled: standing.entitled,
            enforced: standing.enforced,
            edition: standing.edition,
            status: standing.status,
            editable: standing.editable(access),
            organization: requirement(organization).to_vec(),
            workspace: requirement(workspace).to_vec(),
            effective: effective(organization, workspace).to_vec(),
            available,
            matching,
            updated_at: workspace.map(|stored| stored.updated_at),
        }
    }
}

/// The 409 refusing a change that would leave no runner able to take work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoRunnerLeftResponse {
    pub error: String,
    pub code: &'static str,
    pub remaining: i64,
}

/// The labels a scope requires; one that never saved any requires none.
pub(super) fn requirement(stored: Option<&Stored>) -> Labels {
    stored
        .map(|stored| stored.labels.clone())
        .unwrap_or_default()
}

/// What unattended work in a workspace requires: its organization's labels and its own.
pub(super) fn effective(organization: Option<&Stored>, workspace: Option<&Stored>) -> Labels {
    requirement(organization).union(&requirement(workspace))
}
