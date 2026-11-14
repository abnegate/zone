use serde::Deserialize;
use uuid::Uuid;

use crate::services::github::releases::Proposal;

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub source_id: Uuid,
    pub tag: String,
    pub expected_latest_tag: Option<String>,
}

impl From<PreviewRequest> for Proposal {
    fn from(request: PreviewRequest) -> Self {
        Self {
            source_id: request.source_id,
            tag: request.tag,
            expected_latest_tag: request.expected_latest_tag,
        }
    }
}
