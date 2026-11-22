use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};
use uuid::Uuid;
use zone_enterprise::fleet::Labels;

use crate::db::fleet::runners::undecodable;

/// The labels an organization or a workspace requires of runners taking its unattended work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub labels: Labels,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

impl<'row> FromRow<'row, PgRow> for Stored {
    fn from_row(row: &'row PgRow) -> Result<Self, sqlx::Error> {
        let labels: Vec<String> = row.try_get("labels")?;
        Ok(Self {
            labels: Labels::from_strings(&labels).map_err(|error| undecodable("labels", error))?,
            updated_by: row.try_get("updated_by")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}
