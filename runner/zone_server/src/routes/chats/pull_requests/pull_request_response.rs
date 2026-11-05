use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::services::chat::pull_request::{Status, Tracked};

/// One pull request a chat opened, as its panel shows it.
#[derive(Debug, Serialize)]
pub struct PullRequestResponse {
    pub id: Uuid,
    pub provider: String,
    pub owner: String,
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: Option<String>,
    pub dismissed: bool,
    pub created_at: DateTime<Utc>,
    pub status: Option<Status>,
    /// Whether the reader may change it on GitHub: they can write to the
    /// workspace, and one of its sources holds a credential for it.
    pub actionable: bool,
}

impl PullRequestResponse {
    pub fn new(tracked: Tracked, writer: bool) -> Self {
        let Tracked {
            row,
            status,
            credentialed,
        } = tracked;
        Self {
            id: row.id,
            provider: row.provider,
            owner: row.owner,
            repo: row.repo,
            number: row.number,
            url: row.url,
            title: status
                .as_ref()
                .map(|status| status.title.clone())
                .or(row.title),
            dismissed: row.dismissed_at.is_some(),
            created_at: row.created_at,
            status,
            actionable: writer && credentialed,
        }
    }
}
