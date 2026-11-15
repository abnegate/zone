use serde::Deserialize;
use uuid::Uuid;

use crate::services::github::releases::Proposal;

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub source_id: Uuid,
    pub tag: String,
    pub expected_latest_tag: Option<String>,
    pub prerelease: bool,
    pub digest: String,
}

impl CreateRequest {
    pub(crate) fn proposal(&self) -> Proposal {
        Proposal {
            source_id: self.source_id,
            tag: self.tag.clone(),
            expected_latest_tag: self.expected_latest_tag.clone(),
        }
    }
}
