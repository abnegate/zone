use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Attempt {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub nonce: String,
    pub verifier: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
