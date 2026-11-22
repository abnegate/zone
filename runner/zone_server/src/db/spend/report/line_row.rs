use chrono::NaiveDate;
use uuid::Uuid;
use zone_enterprise::spend::{Line, Microdollars};

#[derive(sqlx::FromRow)]
pub(super) struct LineRow {
    date: NaiveDate,
    workspace_id: Option<Uuid>,
    workspace_name: Option<String>,
    user_id: Option<Uuid>,
    user_email: Option<String>,
    automated: bool,
    model: String,
    provider: String,
    requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    spend_microdollars: i64,
}

impl From<LineRow> for Line {
    fn from(row: LineRow) -> Self {
        Self {
            date: row.date,
            workspace_id: row.workspace_id,
            workspace: row.workspace_name,
            user_id: row.user_id,
            user_email: row.user_email,
            automated: row.automated,
            model: row.model,
            provider: row.provider,
            requests: row.requests,
            prompt_tokens: row.prompt_tokens,
            completion_tokens: row.completion_tokens,
            spend: Microdollars(row.spend_microdollars),
        }
    }
}
