//! A Claude sign-in's tokens, and the sealed form they are stored in.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use zone_core::secret::SecretValue;

use super::Error;

const PLANS: &[(&str, &str)] = &[
    ("max", "Claude Max"),
    ("pro", "Claude Pro"),
    ("team", "Claude Team"),
    ("enterprise", "Claude Enterprise"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access: SecretValue,
    pub refresh: Option<SecretValue>,
    pub expires_at: DateTime<Utc>,
    pub scope: String,
    pub subscription: Option<String>,
}

impl Tokens {
    pub fn expiring(&self, now: DateTime<Utc>, margin: TimeDelta) -> bool {
        self.expires_at.signed_duration_since(now) <= margin
    }

    pub fn label(&self) -> Option<String> {
        let subscription = self.subscription.as_deref()?;
        PLANS
            .iter()
            .find(|(kind, _)| *kind == subscription)
            .map(|(_, label)| label.to_string())
    }

    pub fn seal(&self, key: &[u8; 32]) -> Result<String, Error> {
        let plaintext = SecretValue::new(serde_json::to_string(self).map_err(|_| Error::Sealing)?);
        crate::crypto::encrypt(key, plaintext.expose()).map_err(|_| Error::Sealing)
    }

    pub fn open(key: &[u8; 32], sealed: &str) -> Result<Self, Error> {
        let plaintext =
            SecretValue::new(crate::crypto::decrypt(key, sealed).map_err(|_| Error::Sealing)?);
        serde_json::from_str(plaintext.expose()).map_err(|_| Error::Sealing)
    }
}

#[cfg(test)]
mod tests {
    use zone_core::secret::REDACTED;

    use super::*;

    const ACCESS: &str = "fake-access-token-for-tests";
    const REFRESH: &str = "fake-refresh-token-for-tests";

    fn expiry() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000, 0).expect("a valid timestamp")
    }

    fn tokens() -> Tokens {
        Tokens {
            access: SecretValue::new(ACCESS),
            refresh: Some(SecretValue::new(REFRESH)),
            expires_at: expiry(),
            scope: "user:inference".to_string(),
            subscription: Some("max".to_string()),
        }
    }

    fn key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::fill(&mut key);
        key
    }

    #[test]
    fn sealed_tokens_open_to_the_same_tokens_without_ever_showing_them() {
        let key = key();
        let tokens = tokens();

        let sealed = tokens.seal(&key).expect("seal");

        for secret in [ACCESS, REFRESH] {
            assert!(!sealed.contains(secret), "the sealed form shows {secret}");
        }
        assert_eq!(Tokens::open(&key, &sealed).expect("open"), tokens);
        assert!(matches!(
            Tokens::open(&[0; 32], &sealed),
            Err(Error::Sealing)
        ));
    }

    #[test]
    fn tokens_that_were_never_sealed_are_not_trusted() {
        let plaintext = serde_json::to_string(&tokens()).expect("serialise");

        assert!(matches!(
            Tokens::open(&key(), &plaintext),
            Err(Error::Sealing)
        ));
    }

    #[test]
    fn debug_of_tokens_is_redacted() {
        let rendered = format!("{:?}", tokens());

        assert!(
            !rendered.contains(ACCESS) && !rendered.contains(REFRESH),
            "{rendered}"
        );
        assert!(rendered.contains(REDACTED), "{rendered}");
    }

    #[test]
    fn tokens_are_expiring_once_inside_the_margin() {
        let tokens = tokens();
        let margin = TimeDelta::minutes(5);

        assert!(!tokens.expiring(expiry() - TimeDelta::minutes(6), margin));
        assert!(tokens.expiring(expiry() - margin, margin));
        assert!(tokens.expiring(expiry() + TimeDelta::minutes(1), margin));
    }

    #[test]
    fn the_label_names_the_plan_as_the_cli_does() {
        let mut tokens = tokens();

        for (subscription, label) in [
            ("max", Some("Claude Max")),
            ("pro", Some("Claude Pro")),
            ("team", Some("Claude Team")),
            ("enterprise", Some("Claude Enterprise")),
            ("free", None),
        ] {
            tokens.subscription = Some(subscription.to_string());
            assert_eq!(tokens.label().as_deref(), label, "{subscription}");
        }
        tokens.subscription = None;
        assert_eq!(tokens.label(), None);
    }
}
