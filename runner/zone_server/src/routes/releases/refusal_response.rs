use serde::Serialize;

use super::reconciliation_detail::ReconciliationDetail;
use super::refusal_detail::RefusalDetail;
use super::tag_detail::TagDetail;
use crate::services::github::releases::Refusal;

#[derive(Debug, Serialize)]
pub struct RefusalResponse {
    pub success: bool,
    pub error: String,
    pub code: &'static str,
    #[serde(flatten)]
    pub detail: Option<RefusalDetail>,
}

impl From<Refusal> for RefusalResponse {
    fn from(refusal: Refusal) -> Self {
        let (error, code) = (refusal.message(), refusal.code());
        let detail = match refusal {
            Refusal::TagExists { tag, tag_object } => Some(RefusalDetail::Tag(TagDetail {
                tag,
                tag_object,
                ..TagDetail::default()
            })),
            Refusal::TagNotNewer { tag, latest } => Some(RefusalDetail::Tag(TagDetail {
                tag,
                latest_tag: Some(latest),
                ..TagDetail::default()
            })),
            Refusal::ReleaseExists { tag, release_id } => Some(RefusalDetail::Tag(TagDetail {
                tag,
                release_id: Some(release_id),
                ..TagDetail::default()
            })),
            Refusal::ReconciliationRequired(reconciliation) => Some(RefusalDetail::Reconciliation(
                ReconciliationDetail::from(reconciliation),
            )),
            Refusal::LatestTagMoved { .. } | Refusal::PreviewStale | Refusal::InProgress => None,
        };
        Self {
            success: false,
            error,
            code,
            detail,
        }
    }
}
