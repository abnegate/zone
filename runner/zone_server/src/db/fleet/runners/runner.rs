use chrono::{DateTime, Utc};
use sqlx::error::BoxDynError;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};
use uuid::Uuid;
use zone_enterprise::fleet::{Labels, Version};

use crate::db::fleet::runners::CONNECTED;
use crate::db::fleet::state::State;

pub(crate) const COLUMNS: &str = "runner.id, runner.name, runner.instance_id, runner.version, \
     runner.labels, runner.state, runner.started_at, runner.last_seen_at, runner.created_at, \
     runner.state_changed_by, runner.state_changed_at";

/// One Zone server process, registered under its name. The state belongs to the name, so it
/// outlives the process that registered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
    pub id: Uuid,
    pub name: String,
    pub instance: Uuid,
    pub version: Version,
    pub labels: Labels,
    pub state: State,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub state_changed_by: Option<Uuid>,
    pub state_changed_at: Option<DateTime<Utc>>,
}

impl Runner {
    pub fn connected(&self, now: DateTime<Utc>) -> bool {
        now - self.last_seen_at < CONNECTED
    }
}

impl<'row> FromRow<'row, PgRow> for Runner {
    fn from_row(row: &'row PgRow) -> Result<Self, sqlx::Error> {
        let version: String = row.try_get("version")?;
        let labels: Vec<String> = row.try_get("labels")?;
        let state: String = row.try_get("state")?;
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            instance: row.try_get("instance_id")?,
            version: Version::parse(&version).map_err(|error| undecodable("version", error))?,
            labels: Labels::from_strings(&labels).map_err(|error| undecodable("labels", error))?,
            state: State::parse(&state).ok_or_else(|| {
                undecodable("state", format!("\"{state}\" is not a runner state"))
            })?,
            started_at: row.try_get("started_at")?,
            last_seen_at: row.try_get("last_seen_at")?,
            created_at: row.try_get("created_at")?,
            state_changed_by: row.try_get("state_changed_by")?,
            state_changed_at: row.try_get("state_changed_at")?,
        })
    }
}

pub(crate) fn undecodable(column: &str, source: impl Into<BoxDynError>) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: column.to_string(),
        source: source.into(),
    }
}
