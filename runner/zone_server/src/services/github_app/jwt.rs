//! Signing the JWT that authenticates as the App itself.

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use zone_core::secret::SecretValue;

use super::claims::Claims;
use super::configuration::AppConfiguration;
use super::error::GithubAppError;

/// Sign a JWT for `configuration`, valid from a backdated `iat`.
///
/// The result is a bearer credential for the whole App rather than one
/// installation, so it is returned as a [`SecretValue`] and never as a
/// `String`. Both failure paths discard the underlying `jsonwebtoken` error:
/// its `Display` can quote the key it failed to parse.
pub fn sign(
    configuration: &AppConfiguration,
    now: DateTime<Utc>,
) -> Result<SecretValue, GithubAppError> {
    let key = EncodingKey::from_rsa_pem(configuration.private_key().expose().as_bytes())
        .map_err(|_| GithubAppError::PrivateKey)?;

    encode(
        &Header::new(Algorithm::RS256),
        &Claims::issue(configuration.application_id(), now),
        &key,
    )
    .map(SecretValue::new)
    .map_err(|_| GithubAppError::Signing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64};

    use crate::services::github_app::identifier::{ApplicationId, InstallationId};
    use crate::services::github_app::testing::{TEST_PRIVATE_KEY, at};

    fn configuration(private_key: &str) -> AppConfiguration {
        AppConfiguration::new(
            ApplicationId::new(12345),
            InstallationId::new(67890),
            SecretValue::new(private_key),
        )
    }

    fn part(jwt: &str, index: usize) -> serde_json::Value {
        let encoded = jwt.split('.').nth(index).expect("jwt part");
        serde_json::from_slice(&BASE64.decode(encoded).expect("base64")).expect("json")
    }

    #[test]
    fn a_signed_jwt_has_three_parts_and_an_rs256_header() {
        let jwt = sign(&configuration(TEST_PRIVATE_KEY), Utc::now()).expect("sign");

        assert_eq!(jwt.expose().split('.').count(), 3);
        assert_eq!(part(jwt.expose(), 0)["alg"], "RS256");
    }

    #[test]
    fn the_claims_carry_a_backdated_iat_and_a_bounded_exp() {
        let now = at(1_700_000_000);
        let jwt = sign(&configuration(TEST_PRIVATE_KEY), now).expect("sign");
        let claims = part(jwt.expose(), 1);

        assert_eq!(claims["iss"], "12345");
        assert_eq!(claims["iat"].as_i64().expect("iat"), now.timestamp() - 60);
        assert_eq!(
            claims["exp"].as_i64().expect("exp") - claims["iat"].as_i64().expect("iat"),
            9 * 60
        );
    }

    #[test]
    fn the_jwt_never_renders_itself() {
        let jwt = sign(&configuration(TEST_PRIVATE_KEY), Utc::now()).expect("sign");

        assert_eq!(format!("{jwt:?}"), "[REDACTED]");
        assert_eq!(jwt.to_string(), "[REDACTED]");
        assert!(!format!("{jwt:?}").contains('.'));
    }

    #[test]
    fn a_broken_private_key_is_rejected_without_quoting_it() {
        let error = sign(
            &configuration(
                "-----BEGIN RSA PRIVATE KEY-----\nnot-a-key\n-----END RSA PRIVATE KEY-----",
            ),
            Utc::now(),
        )
        .expect_err("an unparseable key cannot sign");

        assert!(matches!(error, GithubAppError::PrivateKey));
        assert!(!error.to_string().contains("not-a-key"));
    }
}
