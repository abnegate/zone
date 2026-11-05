use serde::Serialize;

use crate::services::chat::subagent::ChatSubagent;

#[derive(Debug, Serialize)]
pub struct SubagentsResponse {
    pub subagents: Vec<ChatSubagent>,
}
