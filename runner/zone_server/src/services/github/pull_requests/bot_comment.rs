use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BotComment {
    pub reviewer: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}
