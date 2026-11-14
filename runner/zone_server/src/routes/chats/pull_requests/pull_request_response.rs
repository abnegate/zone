use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::services::chat::pull_request::{Connection, Status, Tracked};

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
    /// Whether the reader can write to the chat's workspace.
    pub writable: bool,
    /// Whether one of the workspace's sources holds a credential for it.
    pub credentialed: bool,
    /// How the workspace reaches GitHub for it; `disconnected` asks for
    /// GitHub to be connected again.
    pub connection: Connection,
    /// Whether a failing or conflicted state starts a run that fixes it.
    pub auto_fix: bool,
    /// Whether a run auto-fix started is still working.
    pub auto_fixing: bool,
}

impl PullRequestResponse {
    pub fn new(tracked: Tracked, writable: bool) -> Self {
        let Tracked {
            row,
            status,
            connection,
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
            auto_fix: row.auto_fix,
            auto_fixing: row.auto_fixing,
            status,
            writable,
            credentialed: connection.credentialed(),
            connection,
        }
    }
}
