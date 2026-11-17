use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::audit::Kind;
use zone_enterprise::audit::destination::{SecretName, Settings};

use crate::db::audit::deliveries::Delivery;
use crate::db::audit::events::Backlog;

/// A destination as an admin sees it: its settings, the names of the secrets that hold a value,
/// and how delivery is going. Never the secrets themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub id: Uuid,
    pub name: String,
    pub kind: Kind,
    pub enabled: bool,
    pub settings: Settings,
    pub secrets: Vec<SecretName>,
    pub delivery: Delivery,
    pub backlog: Backlog,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
