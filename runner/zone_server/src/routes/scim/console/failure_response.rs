use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::scim::ErrorRecord;

/// `path` is the route template, so it never carries an ID or a filter value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailureResponse {
    pub id: Uuid,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub scim_type: Option<String>,
    pub detail: String,
    pub occurred_at: DateTime<Utc>,
}

impl From<ErrorRecord> for FailureResponse {
    fn from(record: ErrorRecord) -> Self {
        Self {
            id: record.id,
            method: record.method,
            path: record.path,
            status: record.status,
            scim_type: record.scim_type,
            detail: record.detail,
            occurred_at: record.occurred_at,
        }
    }
}
