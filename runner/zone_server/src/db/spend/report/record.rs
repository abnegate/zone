use chrono::NaiveDate;
use uuid::Uuid;
use zone_enterprise::spend::{Line, Microdollars};

#[derive(sqlx::FromRow)]
pub(super) struct Record {
    date: NaiveDate,
    workspace_id: Option<Uuid>,
    workspace_name: Option<String>,
    user_id: Option<Uuid>,
    user_email: Option<String>,
    model: String,
    provider: String,
    requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    spend_microdollars: i64,
}

impl From<Record> for Line {
    fn from(record: Record) -> Self {
        Self {
            date: record.date,
            workspace_id: record.workspace_id,
            workspace: record.workspace_name,
            user_id: record.user_id,
            user_email: record.user_email,
            model: record.model,
            provider: record.provider,
            requests: record.requests,
            prompt_tokens: record.prompt_tokens,
            completion_tokens: record.completion_tokens,
            spend: Microdollars(record.spend_microdollars),
        }
    }
}
