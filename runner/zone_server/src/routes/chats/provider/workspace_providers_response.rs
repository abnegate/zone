use serde::Serialize;

use crate::services::provider::Offer;

/// The providers a new chat in a workspace may run on: the workspace's
/// default and every one it may choose.
#[derive(Debug, Serialize)]
pub struct WorkspaceProvidersResponse {
    pub default: Offer,
    pub providers: Vec<Offer>,
}
