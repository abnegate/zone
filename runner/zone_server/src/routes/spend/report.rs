use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use zone_enterprise::spend::Uncounted;

use crate::db::spend::report::{Day, NamedUsage, Usage, UserUsage, WorkspaceUsage};

#[derive(Debug, Serialize)]
pub struct Report {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub ingested_through: Option<DateTime<Utc>>,
    pub total: Usage,
    pub daily: Vec<Day>,
    pub workspaces: Vec<WorkspaceUsage>,
    pub users: Vec<UserUsage>,
    pub models: Vec<NamedUsage>,
    pub providers: Vec<NamedUsage>,
    pub uncounted: Vec<Uncounted>,
}
