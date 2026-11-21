use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::spend::{Budget, Microdollars, Target};

use super::budget_row::BudgetRow;

const THRESHOLD: &str = "threshold_percent";

#[derive(sqlx::FromRow)]
pub(super) struct Record {
    id: Uuid,
    organization_id: Uuid,
    workspace_id: Option<Uuid>,
    user_id: Option<Uuid>,
    limit_microdollars: i64,
    threshold_percent: i32,
    hard: bool,
    updated_at: DateTime<Utc>,
    workspace_name: Option<String>,
    user_name: Option<String>,
    user_email: Option<String>,
    spent_microdollars: i64,
}

impl TryFrom<Record> for BudgetRow {
    type Error = sqlx::Error;

    fn try_from(record: Record) -> Result<Self, Self::Error> {
        let threshold_percent =
            u8::try_from(record.threshold_percent).map_err(|error| sqlx::Error::ColumnDecode {
                index: THRESHOLD.to_owned(),
                source: Box::new(error),
            })?;
        Ok(Self {
            id: record.id,
            organization_id: record.organization_id,
            target: Target::of(record.workspace_id, record.user_id),
            budget: Budget {
                limit: Microdollars(record.limit_microdollars),
                threshold_percent,
                hard: record.hard,
            },
            workspace_name: record.workspace_name,
            user_name: record.user_name,
            user_email: record.user_email,
            spent: Microdollars(record.spent_microdollars),
            updated_at: record.updated_at,
        })
    }
}
