use std::fmt;

use abnegate_secret::REDACTED;

use crate::db::scim::TokenRecord;

/// A token as it was just created or rotated, with the secret only this answer ever carries.
#[derive(Clone, PartialEq, Eq)]
pub struct Issued {
    pub token: TokenRecord,
    pub secret: String,
}

impl fmt::Debug for Issued {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Issued")
            .field("token", &self.token)
            .field("secret", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::services::scim::secret;

    #[test]
    fn the_debug_form_never_shows_the_secret() {
        let secret = secret::generate();
        let issued = Issued {
            token: TokenRecord {
                id: Uuid::new_v4(),
                organization_id: Uuid::new_v4(),
                name: "Okta".to_owned(),
                hint: secret::hint(&secret),
                created_by: None,
                created_at: Utc::now(),
                rotated_at: None,
                last_used_at: None,
            },
            secret: secret.clone(),
        };
        let debug = format!("{issued:?}");
        assert!(!debug.contains(&secret), "{debug}");
        assert!(debug.contains(REDACTED), "{debug}");
    }
}
