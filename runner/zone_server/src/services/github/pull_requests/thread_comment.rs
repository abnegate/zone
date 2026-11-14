use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadComment {
    pub id: Option<u64>,
    pub author: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}
