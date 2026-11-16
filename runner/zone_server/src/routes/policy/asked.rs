//! A request a REST route was asked to start that the agent policy refuses: audited as a denied
//! turn, then answered 403 with the denial's code.

use axum::response::Response;
use uuid::Uuid;
use zone_enterprise::policy::Denial;

use super::refusal;
use crate::db::audit::actions;
use crate::services::policy::{self, Denied, Subject};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked {
    pub workspace: Uuid,
    pub actor: Option<Uuid>,
    pub subject: Subject,
}

impl Asked {
    pub async fn record(&self, state: &AppState, denial: Denial, detail: Option<&str>) {
        let denied = Denied {
            workspace: self.workspace,
            organization: None,
            actor: self.actor,
            subject: self.subject,
            denial,
            detail,
        };
        policy::record(state, actions::AGENT_POLICY_TURN_DENIED, &denied).await;
    }

    pub async fn refuse(&self, state: &AppState, denial: Denial, detail: Option<&str>) -> Response {
        self.record(state, denial, detail).await;
        refusal::denied(denial)
    }
}

pub async fn record_organization(
    state: &AppState,
    organization: Uuid,
    actor: Uuid,
    denial: Denial,
    detail: Option<&str>,
) {
    policy::record_organization(
        state,
        actions::AGENT_POLICY_TURN_DENIED,
        organization,
        actor,
        denial,
        detail,
    )
    .await;
}
