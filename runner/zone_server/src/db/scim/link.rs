use std::fmt;

use abnegate_secret::REDACTED;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Link {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub external_id: Option<String>,
    pub user_name: String,
    pub display_name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub email: Option<String>,
    pub provisioned_password: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Link {
    pub fn new(organization_id: Uuid, user_id: Uuid, user_name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            organization_id,
            user_id,
            external_id: None,
            user_name: user_name.into(),
            display_name: None,
            given_name: None,
            family_name: None,
            email: None,
            provisioned_password: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl fmt::Debug for Link {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Link")
            .field("organization_id", &self.organization_id)
            .field("user_id", &self.user_id)
            .field("external_id", &self.external_id)
            .field("user_name", &self.user_name)
            .field("display_name", &self.display_name)
            .field("given_name", &self.given_name)
            .field("family_name", &self.family_name)
            .field("email", &self.email)
            .field(
                "provisioned_password",
                &self.provisioned_password.as_ref().map(|_| REDACTED),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}
