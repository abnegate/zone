use abnegate_secret::{MasterKey, SecretValue};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::error::{DestinationError, DestinationResult};
use crate::db::audit::scope::Scope;
use crate::secret::Unseal;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct DestinationRecord {
    pub id: Uuid,
    pub organization_id: Option<Uuid>,
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    pub settings: Value,
    pub sealed_secrets: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DestinationRecord {
    pub fn scope(&self) -> Scope {
        Scope::from(self.organization_id)
    }

    pub fn secrets(&self, key: &MasterKey) -> DestinationResult<SecretValue> {
        key.open_strict(&self.sealed_secrets)
            .map_err(|error| DestinationError::Unopenable(self.id, error))
    }
}
