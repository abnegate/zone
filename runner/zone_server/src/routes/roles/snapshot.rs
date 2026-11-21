use serde::Serialize;
use serde_json::Value;

use crate::auth::{Grant, Scope};
use crate::db::custom_roles::CustomRoleRow;

/// What the audit log records of a role before or after a change.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub scope: Scope,
    pub permissions: &'a Grant,
}

impl<'a> Snapshot<'a> {
    pub fn of(row: &'a CustomRoleRow) -> Self {
        Self {
            name: &row.name,
            description: row.description.as_deref(),
            scope: row.scope,
            permissions: &row.grant,
        }
    }

    pub fn value(&self) -> Option<Value> {
        serde_json::to_value(self).ok()
    }
}
