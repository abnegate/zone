use serde::Serialize;

use super::reconciliation_detail::ReconciliationDetail;
use super::tag_detail::TagDetail;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RefusalDetail {
    Tag(TagDetail),
    Reconciliation(ReconciliationDetail),
}
