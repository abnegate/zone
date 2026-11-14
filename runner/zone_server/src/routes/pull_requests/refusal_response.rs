use serde::Serialize;

use crate::services::github::pull_requests::Refusal;
use crate::services::github::readiness::BlockerReport;

#[derive(Debug, Serialize)]
pub struct RefusalResponse {
    pub success: bool,
    pub error: &'static str,
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blockers: Option<Vec<BlockerReport>>,
}

impl From<Refusal> for RefusalResponse {
    fn from(refusal: Refusal) -> Self {
        let (error, code) = (refusal.message(), refusal.code());
        let (current_head, blockers) = match refusal {
            Refusal::HeadMoved { current_head } => (Some(current_head), None),
            Refusal::NotReady { blockers } => (None, Some(blockers)),
            Refusal::Conflict | Refusal::Protected | Refusal::Merged | Refusal::Closed => {
                (None, None)
            }
        };
        Self {
            success: false,
            error,
            code,
            current_head,
            blockers,
        }
    }
}
