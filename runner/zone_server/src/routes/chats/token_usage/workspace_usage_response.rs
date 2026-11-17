use serde::Serialize;

use super::chat_totals_response::ChatTotalsResponse;
use super::model_totals_response::ModelTotalsResponse;

/// A workspace's token spend over one window, by chat and by model. `chats`
/// is capped by the request's limit and leaves out deleted chats; `models`
/// counts every turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceUsageResponse {
    pub workspace_id: String,
    /// The start of the window these totals cover, as RFC 3339.
    pub since: String,
    pub chats: Vec<ChatTotalsResponse>,
    pub models: Vec<ModelTotalsResponse>,
}
