use serde::Serialize;
use uuid::Uuid;

use crate::services::github::releases::Reconciliation;

#[derive(Debug, Serialize)]
pub struct ReconciliationDetail {
    pub tag: String,
    pub tag_object: String,
    pub reference_created: Option<bool>,
    pub release_id: Option<u64>,
    pub release_exists: bool,
    pub marker: Uuid,
}

impl From<Reconciliation> for ReconciliationDetail {
    fn from(reconciliation: Reconciliation) -> Self {
        Self {
            tag: reconciliation.tag,
            tag_object: reconciliation.tag_object,
            reference_created: reconciliation.reference_created,
            release_id: reconciliation.release_id,
            release_exists: reconciliation.release_exists,
            marker: reconciliation.marker,
        }
    }
}
