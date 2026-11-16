use std::fmt;

use abnegate_secret::REDACTED;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Attempt {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub nonce: String,
    pub verifier: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for Attempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Attempt")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("nonce", &REDACTED)
            .field("verifier", &REDACTED)
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_nonce_or_verifier() {
        let shown = format!(
            "{:?}",
            Attempt {
                id: Uuid::nil(),
                provider_id: Uuid::nil(),
                nonce: "nonce-secret".into(),
                verifier: "verifier-secret".into(),
                expires_at: Utc::now(),
                created_at: Utc::now(),
            }
        );
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
