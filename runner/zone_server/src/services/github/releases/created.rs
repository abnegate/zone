use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Created {
    pub source_id: Uuid,
    pub id: u64,
    pub tag: String,
    pub name: Option<String>,
    pub url: String,
    pub published_at: String,
}
