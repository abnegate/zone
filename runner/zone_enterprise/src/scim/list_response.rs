use serde::Serialize;

use crate::scim::urn;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse<T> {
    pub schemas: Vec<&'static str>,
    pub total_results: u64,
    pub start_index: u64,
    pub items_per_page: u64,
    #[serde(rename = "Resources")]
    pub resources: Vec<T>,
}

impl<T> ListResponse<T> {
    pub fn new(resources: Vec<T>, total_results: u64, start_index: u64) -> Self {
        Self {
            schemas: vec![urn::LIST_RESPONSE],
            total_results,
            start_index,
            items_per_page: u64::try_from(resources.len()).unwrap_or(u64::MAX),
            resources,
        }
    }
}
