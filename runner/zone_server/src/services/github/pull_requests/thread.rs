use serde::Serialize;

use super::ThreadComment;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Thread {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub outdated: bool,
    pub url: Option<String>,
    pub comments: Vec<ThreadComment>,
}
