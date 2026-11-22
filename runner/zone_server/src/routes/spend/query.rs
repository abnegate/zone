use chrono::NaiveDate;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::spend::{Error, Range};

use crate::db::spend::report::Filter;

const FROM: &str = "from";
const TO: &str = "to";
const WORKSPACE_ID: &str = "workspace_id";
const USER_ID: &str = "user_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query {
    pub range: Range,
    pub workspace: Option<Uuid>,
    pub user: Option<Uuid>,
}

impl Query {
    /// Unknown parameters are ignored. A repeated one is ambiguous and a malformed one is
    /// unreadable, so both are refused as an invalid range.
    pub fn read(raw: Option<&str>, today: NaiveDate) -> Result<Self, Error> {
        let mut from = None;
        let mut to = None;
        let mut workspace = None;
        let mut user = None;
        for (key, value) in form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
            let slot = match key.as_ref() {
                FROM => &mut from,
                TO => &mut to,
                WORKSPACE_ID => &mut workspace,
                USER_ID => &mut user,
                _ => continue,
            };
            if slot.replace(value.into_owned()).is_some() {
                return Err(Error::RangeInvalid);
            }
        }
        Ok(Self {
            range: Range::parse(from.as_deref(), to.as_deref(), today)?,
            workspace: identifier(workspace.as_deref())?,
            user: identifier(user.as_deref())?,
        })
    }

    pub fn filter(&self, organization: Uuid) -> Filter {
        Filter {
            organization,
            range: self.range,
            workspace: self.workspace,
            user: self.user,
        }
    }

    pub fn audited(&self) -> Value {
        json!({
            FROM: self.range.from,
            TO: self.range.to,
            WORKSPACE_ID: self.workspace,
            USER_ID: self.user,
        })
    }
}

fn identifier(value: Option<&str>) -> Result<Option<Uuid>, Error> {
    value
        .map(|value| Uuid::parse_str(value).map_err(|_| Error::RangeInvalid))
        .transpose()
}
