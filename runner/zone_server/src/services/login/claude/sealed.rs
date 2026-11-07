//! A Claude sign-in's tokens in the sealed form Zone stores them in.

use abnegate_agent_cli::login::claude::Tokens;
use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;

use super::SealingError;
use crate::secret::Unseal;

pub trait Sealed: Sized {
    /// The tokens' JSON sealed under `key`, ready to store.
    fn seal(&self, key: &MasterKey) -> Result<String, SealingError>;

    /// The tokens `sealed` holds, sealed under `key` in an envelope or before envelopes existed.
    fn open(key: &MasterKey, sealed: &str) -> Result<Self, SealingError>;
}

impl Sealed for Tokens {
    fn seal(&self, key: &MasterKey) -> Result<String, SealingError> {
        let plaintext = SecretValue::new(serde_json::to_string(self).map_err(|_| SealingError)?);
        encrypt_value(&plaintext, key).map_err(|_| SealingError)
    }

    fn open(key: &MasterKey, sealed: &str) -> Result<Self, SealingError> {
        let plaintext = key.open_strict(sealed).map_err(|_| SealingError)?;
        serde_json::from_str(plaintext.expose()).map_err(|_| SealingError)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use abnegate_secret::is_encrypted;
    use chrono::{DateTime, TimeDelta, Utc};

    use super::*;
    use crate::secret::fixture::seal_legacy;

    const ACCESS: &str = "fake-access-token-for-tests";
    const REFRESH: &str = "fake-refresh-token-for-tests";
    const LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
    /// The key [`SEALED_BEFORE_THE_SWITCH`] and [`UNTIMED_SEALED_BEFORE_THE_SWITCH`] were sealed
    /// under.
    const FIXTURE_KEY: [u8; 32] = [7; 32];
    /// [`tokens`] as Zone's own `Tokens` serialised them before the switch to abnegate-agent-cli.
    const SHAPE: &str = r#"{"access":"fake-access-token-for-tests","refresh":"fake-refresh-token-for-tests","expires_at":"2026-09-21T14:13:20Z","issued_at":"2026-09-21T06:13:20Z","scope":"user:inference","subscription":"max"}"#;
    /// Tokens as Zone sealed them before it recorded their lifetime.
    const UNTIMED: &str = r#"{"access":"fake-access-token-for-tests","refresh":"fake-refresh-token-for-tests","expires_at":"2026-09-21T14:13:20Z","scope":"user:inference","subscription":"max"}"#;
    /// [`SHAPE`] as Zone's own `Tokens::seal` sealed it before the switch, under [`FIXTURE_KEY`].
    const SEALED_BEFORE_THE_SWITCH: &str = "ENC[v1:T0qHkcxrKZoNAzK5Y2B8cGSiTjWK5AGwIFKW6dfCCxFDuKaSkE3C/l0QZRPfx1VG2vuComS/hVkHGIxXxyaak38PoMpvF04DE2KX/RxN2+lENfBeHrohuXhZbP9BHGM2+Fumn3MI6vmW1YKiO9/g2aCSqWOPzp29fXUu43jBHlXj2ZUr4G3K01Ds9/dv65TlLBxGDXKRun9WNMjwdepeMibj7SFCw3EaFN6p4R5gIaQnWFqlsPby0Fwvvs/PFJM+LOdx8tiWcdLYI9invU5QtksNroAiRKqlj/XdzvJAHYI7Iw==]";
    /// [`UNTIMED`] sealed under [`FIXTURE_KEY`] the way Zone stored it before envelopes existed.
    const UNTIMED_SEALED_BEFORE_THE_SWITCH: &str = "SJsFV+/ccu6ePQSWw+p/Lk6PDbwsJsUl8Hwb8e+Pc9p22f8oLfgAZDBthFjPuaB4gkWVesW4K9nA4lMLrPaHPXzgq7On29n9gaMY7QKUqfH3SiTTY7/Wttcjd07Jx+3PiE0ZS4oDIQM4rVzZE+iZdCSbM+5pUjbAK1bUDJz/o3iB0MEDtNAyY+qPcL+xM9oE/7yFZPhPzmKvUy9EiZSAl61oOn3YHz4JYvDbJNfuQi2NdIpZ5nGjcw7pF0Fv7Gk=";

    fn expiry() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000, 0).expect("a valid timestamp")
    }

    fn tokens() -> Tokens {
        Tokens::new(
            SecretValue::new(ACCESS),
            Some(SecretValue::new(REFRESH)),
            expiry(),
            Some(expiry() - TimeDelta::hours(8)),
            "user:inference",
            Some("max".to_string()),
        )
    }

    fn untimed() -> Tokens {
        Tokens::new(
            SecretValue::new(ACCESS),
            Some(SecretValue::new(REFRESH)),
            expiry(),
            None,
            "user:inference",
            Some("max".to_string()),
        )
    }

    fn key() -> MasterKey {
        MasterKey::generate().expect("a key")
    }

    #[test]
    fn a_login_sealed_before_the_switch_still_opens() {
        let key = MasterKey::new(FIXTURE_KEY);

        let opened = Tokens::open(&key, SEALED_BEFORE_THE_SWITCH).expect("a login sealed before");
        assert_eq!(opened, tokens());
        assert_eq!(opened.lifetime(), Some(LIFETIME));

        let untimed = Tokens::open(&key, UNTIMED_SEALED_BEFORE_THE_SWITCH)
            .expect("a login sealed before its lifetime was recorded");
        assert_eq!(untimed, self::untimed());
        assert_eq!(untimed.lifetime(), None);
    }

    #[test]
    fn tokens_seal_to_the_same_json_as_before() {
        let key = key();

        let sealed = tokens().seal(&key).expect("seal");

        let plaintext = key.open_strict(&sealed).expect("an envelope that opens");
        assert_eq!(plaintext.expose(), SHAPE);
        let untimed = untimed().seal(&key).expect("seal");
        let plaintext = key.open_strict(&untimed).expect("an envelope that opens");
        assert_eq!(
            plaintext.expose(),
            SHAPE.replace(
                r#""issued_at":"2026-09-21T06:13:20Z""#,
                r#""issued_at":null"#
            ),
            "unknown lifetimes are written as Zone wrote them"
        );
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
            Tokens::open(&MasterKey::new([0; 32]), &sealed),
            Err(SealingError)
        ));
    }

    #[test]
    fn tokens_are_sealed_in_an_envelope_that_opens() {
        let key = key();
        let tokens = tokens();

        let sealed = tokens.seal(&key).expect("seal");

        assert!(is_encrypted(&sealed), "{sealed} is not an envelope");
        assert_eq!(Tokens::open(&key, &sealed).expect("open"), tokens);
    }

    #[test]
    fn tokens_sealed_before_the_envelope_existed_open() {
        let key = key();

        let opened = Tokens::open(&key, &seal_legacy(SHAPE, &key)).expect("a legacy body");

        assert_eq!(opened, tokens());
    }

    #[test]
    fn tokens_sealed_before_their_lifetime_was_recorded_open_with_it_unknown() {
        let key = key();

        let opened =
            Tokens::open(&key, &seal_legacy(UNTIMED, &key)).expect("tokens in the earlier form");

        assert_eq!(opened, untimed());
        assert_eq!(opened.lifetime(), None);
    }

    #[test]
    fn tokens_that_were_never_sealed_are_not_trusted() {
        assert!(matches!(Tokens::open(&key(), SHAPE), Err(SealingError)));
    }
}
