use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::publication_response::PublicationResponse;
use super::run_response::RunResponse;
use crate::db::chat_tasks::ChatTaskRow;

/// One of a chat's tasks, as its panel shows it.
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: Uuid,
    pub title: String,
    pub title_pending: bool,
    pub status: String,
    pub is_agentic: bool,
    pub href: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub run: Option<RunResponse>,
    pub pull_request: Option<PublicationResponse>,
}

fn utc(timestamp: Option<NaiveDateTime>) -> Option<DateTime<Utc>> {
    timestamp.map(|timestamp| timestamp.and_utc())
}

impl From<ChatTaskRow> for TaskResponse {
    fn from(row: ChatTaskRow) -> Self {
        let run = match (row.run_id, row.run_status) {
            (Some(id), Some(status)) => Some(RunResponse {
                id,
                status,
                phase: row.run_phase,
                progress: row.run_progress,
                started_at: utc(row.run_started_at),
                completed_at: utc(row.run_completed_at),
            }),
            _ => None,
        };
        Self {
            id: row.id,
            href: format!("/tasks?id={}", row.id),
            title: row.title,
            title_pending: row.title_pending,
            status: row.status,
            is_agentic: row.is_agentic,
            created_at: utc(row.created_at),
            updated_at: utc(row.updated_at),
            run,
            pull_request: row.pr_url.map(|url| PublicationResponse {
                url,
                status: row.pr_status,
            }),
        }
    }
}
