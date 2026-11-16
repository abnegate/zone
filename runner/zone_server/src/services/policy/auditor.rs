use std::sync::Arc;

use uuid::Uuid;
use zone_enterprise::policy::Denial;

use super::{Denied, Subject, record};
use crate::agent::approval::Withholding;
use crate::agent::tools::egress::Refusing;
use crate::db::audit::actions;
use crate::state::AppState;

/// The tools report synchronously, so each refusal is written in a task of its own and never
/// holds the call up.
#[derive(Clone)]
pub struct Auditor {
    state: AppState,
    workspace: Uuid,
    organization: Option<Uuid>,
    actor: Option<Uuid>,
    subject: Subject,
}

impl Auditor {
    pub fn new(
        state: AppState,
        workspace: Uuid,
        organization: Option<Uuid>,
        actor: Option<Uuid>,
        subject: Subject,
    ) -> Self {
        Self {
            state,
            workspace,
            organization,
            actor,
            subject,
        }
    }

    pub fn approvals(&self) -> Arc<Withholding> {
        let auditor = self.clone();
        Arc::new(move |tool: &str| auditor.record(Denial::Approval, tool))
    }

    pub fn egress(&self) -> Arc<Refusing> {
        let auditor = self.clone();
        Arc::new(move |denial: Denial, refused: &str| auditor.record(denial, refused))
    }

    fn record(&self, denial: Denial, detail: &str) {
        let auditor = self.clone();
        let detail = detail.to_owned();
        tokio::spawn(async move {
            let denied = Denied {
                workspace: auditor.workspace,
                organization: auditor.organization,
                actor: auditor.actor,
                subject: auditor.subject,
                denial,
                detail: Some(&detail),
            };
            record(&auditor.state, actions::AGENT_POLICY_TOOL_DENIED, &denied).await;
        });
    }
}
