use chrono::NaiveDate;
use uuid::Uuid;

use crate::audit::csv::field;
use crate::spend::microdollars::Microdollars;

pub const HEADER: &str = "date,workspace_id,workspace,user_id,user_email,model,provider,requests,prompt_tokens,completion_tokens,spend_usd\r\n";

const DELIMITER: &str = ",";
const TERMINATOR: &str = "\r\n";

/// One CSV row: a day's usage for one workspace, user, model and provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub date: NaiveDate,
    pub workspace_id: Option<Uuid>,
    pub workspace: Option<String>,
    pub user_id: Option<Uuid>,
    pub user_email: Option<String>,
    pub model: String,
    pub provider: String,
    pub requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub spend: Microdollars,
}

pub fn row(line: &Line) -> String {
    let fields = [
        line.date.to_string(),
        identifier(line.workspace_id),
        field(line.workspace.as_deref().unwrap_or_default()),
        identifier(line.user_id),
        field(line.user_email.as_deref().unwrap_or_default()),
        field(&line.model),
        field(&line.provider),
        line.requests.to_string(),
        line.prompt_tokens.to_string(),
        line.completion_tokens.to_string(),
        line.spend.exact(),
    ];
    let mut row = fields.join(DELIMITER);
    row.push_str(TERMINATOR);
    row
}

fn identifier(value: Option<Uuid>) -> String {
    value.map(|id| id.to_string()).unwrap_or_default()
}
