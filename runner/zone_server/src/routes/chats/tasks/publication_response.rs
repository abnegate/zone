use serde::Serialize;

/// The pull request a task's run published.
#[derive(Debug, Serialize)]
pub struct PublicationResponse {
    pub url: String,
    pub status: Option<String>,
}
