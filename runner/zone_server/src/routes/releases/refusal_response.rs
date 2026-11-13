use serde::Serialize;

use crate::services::github::releases::Refusal;

#[derive(Debug, Serialize)]
pub struct RefusalResponse {
    pub success: bool,
    pub error: String,
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_created: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_id: Option<Option<u64>>,
}

impl From<Refusal> for RefusalResponse {
    fn from(refusal: Refusal) -> Self {
        let refused = Self {
            success: false,
            error: refusal.message(),
            code: refusal.code(),
            tag: None,
            tag_object: None,
            reference_created: None,
            release_id: None,
        };
        match refusal {
            Refusal::TagExists { tag, tag_object } => Self {
                tag: Some(tag),
                tag_object,
                ..refused
            },
            Refusal::ReconciliationRequired(reconciliation) => Self {
                tag: Some(reconciliation.tag),
                tag_object: Some(reconciliation.tag_object),
                reference_created: Some(reconciliation.reference_created),
                release_id: Some(reconciliation.release_id),
                ..refused
            },
            Refusal::LatestTagMoved { .. } | Refusal::PreviewStale | Refusal::InProgress => refused,
        }
    }
}
