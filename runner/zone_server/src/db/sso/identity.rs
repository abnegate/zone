use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Identity {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub user_id: Uuid,
    pub subject: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub last_signed_in_at: DateTime<Utc>,
}
