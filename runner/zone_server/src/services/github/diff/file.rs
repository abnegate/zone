use serde::Serialize;

use super::Hunk;
use super::Status;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct File {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: Status,
    pub additions: u32,
    pub deletions: u32,
    pub changes: u32,
    pub binary: bool,
    pub truncated: bool,
    pub blob_url: String,
    pub hunks: Vec<Hunk>,
}
