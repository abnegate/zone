//! One place to ask for a credential, however the source or project is wired.

use std::fmt;
use std::time::SystemTime;

use abnegate_secret::{MasterKey, SecretValue, decrypt_value, encrypt_value, is_encrypted};
use abnegate_vcs::pull_request::{
    ApplicationClaims, ApplicationId, InstallationId, InstallationIssuer, TokenCache,
};

use crate::config::Config;
use crate::crypto;

use super::credential::Credential;
use super::error::Error;

/// Seals App secrets at rest, mints installation tokens, and opens stored
/// personal access tokens.
///
/// One cache serves every App because one provider talks to one GitHub host,
/// and GitHub numbers installations uniquely per host across all Apps.
pub struct Provider {
    issuer: Option<InstallationIssuer>,
    cache: TokenCache,
    master_key: MasterKey,
    encryption_key: [u8; 32],
}

impl Provider {
    pub fn new(config: &Config, encryption_key: [u8; 32]) -> Self {
        let issuer = InstallationIssuer::configured(&config.github_api_url)
            .inspect_err(|error| {
                tracing::warn!(
                    %error,
                    "GitHub App installations are unavailable; sources and projects fall back to stored tokens"
                );
            })
            .ok();

        Self::assemble(issuer, encryption_key)
    }

    pub fn with_issuer(issuer: InstallationIssuer, encryption_key: [u8; 32]) -> Self {
        Self::assemble(Some(issuer), encryption_key)
    }

    fn assemble(issuer: Option<InstallationIssuer>, encryption_key: [u8; 32]) -> Self {
        Self {
            issuer,
            cache: TokenCache::new(),
            master_key: MasterKey::new(encryption_key),
            encryption_key,
        }
    }

    pub fn seal(&self, secret: &SecretValue) -> Result<String, Error> {
        encrypt_value(secret, &self.master_key).map_err(|_| Error::Sealing)
    }

    pub fn open(&self, sealed: &str) -> Result<SecretValue, Error> {
        if !is_encrypted(sealed) {
            return Err(Error::Unsealing);
        }

        decrypt_value(sealed, &self.master_key).map_err(|_| Error::Unsealing)
    }

    pub fn application_jwt(
        &self,
        application: ApplicationId,
        sealed_key: &str,
    ) -> Result<SecretValue, Error> {
        Ok(ApplicationClaims::issue(application, SystemTime::now())
            .sign(&self.open(sealed_key)?)?)
    }

    pub async fn mint(
        &self,
        application: ApplicationId,
        sealed_key: &str,
        installation: InstallationId,
    ) -> Result<SecretValue, Error> {
        let issuer = self.issuer.as_ref().ok_or(Error::Unavailable)?;

        if let Some(cached) = self.cache.get(installation, SystemTime::now()) {
            return Ok(cached.secret().clone());
        }

        let jwt = self.application_jwt(application, sealed_key)?;
        let token = issuer.issue(&jwt, installation).await?;
        let secret = token.secret().clone();
        self.cache.insert(installation, token);

        Ok(secret)
    }

    pub fn invalidate(&self, installation: InstallationId) {
        self.cache.forget(installation);
    }

    pub fn stored_source(&self, encrypted: Option<&str>) -> Result<Option<Credential>, Error> {
        encrypted
            .map(|encrypted| {
                crypto::decrypt(&self.encryption_key, encrypted)
                    .map(|token| Credential::stored(SecretValue::new(token)))
                    .map_err(|_| Error::Credential)
            })
            .transpose()
    }

    pub fn stored_project(&self, stored: Option<&str>) -> Option<Credential> {
        stored.map(|stored| {
            Credential::stored(SecretValue::new(crypto::open(&self.encryption_key, stored)))
        })
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.encryption_key.fill(0);
    }
}

impl fmt::Debug for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Provider")
            .field("available", &self.issuer.is_some())
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_vcs::pull_request::ApplicationError;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use std::sync::LazyLock;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::services::github_app::testing::{TEST_PRIVATE_KEY, issuing, provider};

    const APPLICATION: ApplicationId = ApplicationId::new(12345);
    const INSTALLATION: InstallationId = InstallationId::new(67890);

    fn key() -> [u8; 32] {
        static KEY: LazyLock<[u8; 32]> = LazyLock::new(rand::random);
        *KEY
    }

    fn sealed_key(provider: &Provider) -> String {
        provider
            .seal(&SecretValue::new(TEST_PRIVATE_KEY))
            .expect("seal")
    }

    #[tokio::test]
    async fn an_installation_token_is_minted_once_and_reused() {
        let server = MockServer::start().await;
        issuing(&server, INSTALLATION.get(), "ghs_installationtokenvalue", 1).await;
        let provider = provider(&server, key());
        let sealed = sealed_key(&provider);

        for _ in 0..5 {
            let token = provider
                .mint(APPLICATION, &sealed, INSTALLATION)
                .await
                .expect("mint");
            assert_eq!(token.expose(), "ghs_installationtokenvalue");
        }
    }

    #[tokio::test]
    async fn invalidating_an_installation_mints_a_new_token() {
        let server = MockServer::start().await;
        issuing(&server, INSTALLATION.get(), "ghs_installationtokenvalue", 2).await;
        let provider = provider(&server, key());
        let sealed = sealed_key(&provider);

        provider
            .mint(APPLICATION, &sealed, INSTALLATION)
            .await
            .expect("mint");
        provider.invalidate(INSTALLATION);
        provider
            .mint(APPLICATION, &sealed, INSTALLATION)
            .await
            .expect("mint");
    }

    #[tokio::test]
    async fn each_installation_gets_its_own_token() {
        let server = MockServer::start().await;
        issuing(&server, 1, "ghs_first", 1).await;
        issuing(&server, 2, "ghs_second", 1).await;
        let provider = provider(&server, key());
        let sealed = sealed_key(&provider);

        let first = provider
            .mint(APPLICATION, &sealed, InstallationId::new(1))
            .await
            .expect("mint");
        let second = provider
            .mint(APPLICATION, &sealed, InstallationId::new(2))
            .await
            .expect("mint");

        assert_eq!(first.expose(), "ghs_first");
        assert_eq!(second.expose(), "ghs_second");
    }

    #[tokio::test]
    async fn a_refused_exchange_surfaces_without_the_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let provider = provider(&server, key());

        let error = provider
            .mint(APPLICATION, &sealed_key(&provider), INSTALLATION)
            .await
            .expect_err("a 403 is a failure");

        let rendered = error.to_string();
        assert!(matches!(
            error,
            Error::Application(ApplicationError::Status(403))
        ));
        assert!(!rendered.contains(&server.uri()), "{rendered}");
        assert!(!rendered.contains("access_tokens"), "{rendered}");
    }

    #[tokio::test]
    async fn a_plain_http_origin_makes_installations_unavailable() {
        let config = Config {
            github_api_url: "http://127.0.0.1:1".to_string(),
            ..crate::state::test_config()
        };
        let provider = Provider::new(&config, key());

        let error = provider
            .mint(APPLICATION, &sealed_key(&provider), INSTALLATION)
            .await
            .expect_err("an http origin cannot mint");

        assert!(matches!(error, Error::Unavailable));
    }

    #[test]
    fn an_https_origin_makes_installations_available() {
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(provider.issuer.is_some());
    }

    #[test]
    fn the_application_jwt_names_the_application() {
        let provider = Provider::new(&crate::state::test_config(), key());

        let jwt = provider
            .application_jwt(APPLICATION, &sealed_key(&provider))
            .expect("sign");

        let claims = jwt.expose().split('.').nth(1).expect("claims part");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).expect("base64")).expect("json");
        assert_eq!(claims["iss"], "12345");
    }

    #[test]
    fn a_secret_round_trips_through_sealing() {
        let provider = Provider::new(&crate::state::test_config(), key());

        let sealed = sealed_key(&provider);

        assert!(!sealed.contains("BEGIN"));
        assert_eq!(
            provider.open(&sealed).expect("open").expose(),
            TEST_PRIVATE_KEY
        );
    }

    #[test]
    fn opening_refuses_a_secret_stored_in_the_clear() {
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(matches!(
            provider.open(TEST_PRIVATE_KEY),
            Err(Error::Unsealing)
        ));
    }

    #[test]
    fn opening_refuses_a_secret_sealed_under_another_key() {
        let other = Provider::new(&crate::state::test_config(), [9u8; 32]);
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(matches!(
            provider.open(&sealed_key(&other)),
            Err(Error::Unsealing)
        ));
    }

    #[test]
    fn a_stored_source_token_is_decrypted() {
        let provider = Provider::new(&crate::state::test_config(), key());
        let encrypted = crypto::encrypt(&key(), "ghp_personalaccesstoken").expect("encrypt");

        let credential = provider
            .stored_source(Some(&encrypted))
            .expect("decrypt")
            .expect("a stored token yields a credential");

        assert_eq!(credential.secret().expose(), "ghp_personalaccesstoken");
    }

    #[test]
    fn a_source_with_no_stored_token_has_no_credential() {
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(
            provider
                .stored_source(None)
                .expect("nothing to decrypt")
                .is_none()
        );
    }

    #[test]
    fn a_source_token_sealed_under_another_key_fails_closed() {
        let provider = Provider::new(&crate::state::test_config(), key());
        let encrypted = crypto::encrypt(&[9u8; 32], "ghp_personalaccesstoken").expect("encrypt");

        let error = provider
            .stored_source(Some(&encrypted))
            .expect_err("a token from another deployment must not resolve");

        assert!(matches!(error, Error::Credential));
        assert!(!error.to_string().contains("ghp_personalaccesstoken"));
    }

    #[test]
    fn a_source_token_stored_in_the_clear_fails_closed() {
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(matches!(
            provider.stored_source(Some("ghp_personalaccesstoken")),
            Err(Error::Credential)
        ));
    }

    #[test]
    fn a_project_token_opens_whether_encrypted_or_legacy_plaintext() {
        let provider = Provider::new(&crate::state::test_config(), key());
        let encrypted = crypto::encrypt(&key(), "ghp_encrypted").expect("encrypt");

        let opened = provider
            .stored_project(Some(&encrypted))
            .expect("credential");
        let legacy = provider
            .stored_project(Some("ghp_legacyplaintext"))
            .expect("credential");

        assert_eq!(opened.secret().expose(), "ghp_encrypted");
        assert_eq!(legacy.secret().expose(), "ghp_legacyplaintext");
        assert!(provider.stored_project(None).is_none());
    }

    #[tokio::test]
    async fn nothing_the_provider_renders_carries_a_credential() {
        let server = MockServer::start().await;
        issuing(&server, INSTALLATION.get(), "ghs_installationtokenvalue", 1).await;
        let provider = provider(&server, key());
        let sealed = sealed_key(&provider);

        let token = provider
            .mint(APPLICATION, &sealed, INSTALLATION)
            .await
            .expect("mint");
        let credential = provider
            .stored_project(Some("ghp_legacyplaintext"))
            .expect("credential");

        for rendered in [
            format!("{provider:?}"),
            format!("{token:?}"),
            format!("{credential:?}"),
        ] {
            assert!(
                !rendered.contains("ghs_installationtokenvalue"),
                "{rendered}"
            );
            assert!(!rendered.contains("ghp_legacyplaintext"), "{rendered}");
            assert!(!rendered.contains("PRIVATE KEY"), "{rendered}");
            assert!(!rendered.contains(&sealed), "{rendered}");
            assert!(rendered.contains("[REDACTED]"), "{rendered}");
        }
    }
}
