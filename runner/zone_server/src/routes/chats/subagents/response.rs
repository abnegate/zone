use serde::Serialize;

use crate::services::chat::subagent::ChatSubagent;

#[derive(Debug, Serialize)]
pub struct Response {
    pub subagents: Vec<ChatSubagent>,
}
