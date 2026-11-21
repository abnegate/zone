use std::fmt;

use abnegate_secret::REDACTED;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::asserted::Asserted;

#[derive(Clone, PartialEq, Eq)]
pub struct Attempt {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub nonce: Option<String>,
    pub verifier: Option<String>,
    pub request_id: Option<String>,
    pub asserted: Option<Asserted>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for Attempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Attempt")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("nonce", &self.nonce.as_ref().map(|_| REDACTED))
            .field("verifier", &self.verifier.as_ref().map(|_| REDACTED))
            .field("request_id", &self.request_id)
            .field("asserted", &self.asserted)
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_nonce_verifier_or_code_hash() {
        let shown = format!(
            "{:?}",
            Attempt {
                id: Uuid::nil(),
                provider_id: Uuid::nil(),
                nonce: Some("nonce-secret".into()),
                verifier: Some("verifier-secret".into()),
                request_id: Some("_request".into()),
                asserted: Some(Asserted {
                    code_hash: "code-secret".into(),
                    subject: "subject-1".into(),
                    email: "ada@acme.com".into(),
                    display_name: None,
                }),
                expires_at: Utc::now(),
                created_at: Utc::now(),
            }
        );
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
        assert!(shown.contains("_request"), "{shown}");
    }
}
