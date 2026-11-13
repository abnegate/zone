use abnegate_vcs::CheckRun;
use abnegate_vcs::CommitStatus;
use abnegate_vcs::WorkflowRun;
use serde::Serialize;

use super::CheckKind;
use super::absolute;
use crate::services::github::client::assessment;
use crate::services::github::client::ci_token;
use crate::services::github::readiness::CheckState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckDetail {
    pub kind: CheckKind,
    pub id: u64,
    pub name: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub updated_at: Option<String>,
}

impl CheckDetail {
    pub fn workflow(run: &WorkflowRun) -> Self {
        Self {
            kind: CheckKind::Workflow,
            id: run.id.get(),
            name: run.name.clone().unwrap_or_else(|| run.path.clone()),
            state: state(ci_token(run.status.as_deref(), run.conclusion.as_deref())),
            url: absolute(&run.url),
            updated_at: Some(run.updated_at.clone()),
        }
    }

    pub fn check_run(check: &CheckRun) -> Self {
        Self {
            kind: CheckKind::CheckRun,
            id: check.id.unwrap_or_default(),
            name: check.name.clone().unwrap_or_default(),
            state: state(ci_token(Some(&check.status), check.conclusion.as_deref())),
            url: [&check.url, &check.details_url]
                .into_iter()
                .flatten()
                .find_map(|url| absolute(url)),
            updated_at: check.completed_at.clone().or(check.started_at.clone()),
        }
    }

    pub fn status(status: &CommitStatus, position: usize) -> Self {
        Self {
            kind: CheckKind::Status,
            id: u64::try_from(position).unwrap_or(u64::MAX),
            name: status.context.clone(),
            state: state(&status.state),
            url: status.target_url.as_deref().and_then(absolute),
            updated_at: status.created_at.clone(),
        }
    }
}

fn state(token: &str) -> CheckState {
    CheckState::parse(assessment(&[token])).unwrap_or(CheckState::Unknown)
}
