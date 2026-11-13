use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceOptions {
    pub source_id: Uuid,
    pub repository: String,
    pub repository_url: String,
    pub default_branch: String,
    pub latest_tag: Option<String>,
    pub next_tag: String,
    pub previous_tags: Vec<String>,
}
