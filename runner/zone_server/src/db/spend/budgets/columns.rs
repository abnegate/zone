use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::spend::{Budget, Microdollars, Target};

use super::budget_row::BudgetRow;

const THRESHOLD: &str = "threshold_percent";

#[derive(sqlx::FromRow)]
pub(super) struct Columns {
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

impl TryFrom<Columns> for BudgetRow {
    type Error = sqlx::Error;

    fn try_from(columns: Columns) -> Result<Self, Self::Error> {
        let threshold_percent =
            u8::try_from(columns.threshold_percent).map_err(|error| sqlx::Error::ColumnDecode {
                index: THRESHOLD.to_owned(),
                source: Box::new(error),
            })?;
        Ok(Self {
            id: columns.id,
            organization_id: columns.organization_id,
            target: Target::of(columns.workspace_id, columns.user_id),
            budget: Budget {
                limit: Microdollars(columns.limit_microdollars),
                threshold_percent,
                hard: columns.hard,
            },
            workspace_name: columns.workspace_name,
            user_name: columns.user_name,
            user_email: columns.user_email,
            spent: Microdollars(columns.spent_microdollars),
            updated_at: columns.updated_at,
        })
    }
}
