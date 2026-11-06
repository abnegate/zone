use serde::Serialize;

use crate::db::chats::recent::Recent;
use crate::services::provider::Offer;

/// The providers a new chat in a workspace may run on: the workspace's
/// default and every one it may choose, and the settings of the chat the
/// workspace used last, which a new chat starts from.
#[derive(Debug, Serialize)]
pub struct WorkspaceProvidersResponse {
    pub default: Offer,
    pub providers: Vec<Offer>,
    pub recent: Option<Recent>,
}
