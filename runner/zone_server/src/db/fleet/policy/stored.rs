use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};
use uuid::Uuid;

use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::runners::undecodable;

/// The server's one fleet policy. A fleet with no minimum admits every version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    pub minimum: Option<Minimum>,
    pub updated_by: Option<Uuid>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl<'row> FromRow<'row, PgRow> for Policy {
    fn from_row(row: &'row PgRow) -> Result<Self, sqlx::Error> {
        let minimum: Option<String> = row.try_get("minimum_version")?;
        Ok(Self {
            minimum: minimum
                .map(|minimum| Minimum::parse(&minimum))
                .transpose()
                .map_err(|error| undecodable("minimum_version", error))?,
            updated_by: row.try_get("updated_by")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}
