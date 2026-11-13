use serde::Serialize;

use crate::services::github::pull_requests::Log;

#[derive(Debug, Serialize)]
pub struct LogResponse {
    pub log: Log,
}
