//! One place to ask for a source's credentials, however that source is wired.

use std::fmt;

use chrono::Utc;
use zone_core::secret::{MasterKey, SecretValue};

use crate::crypto;
use crate::db::sources::SourceCredentialRow;

use super::cache::TokenCache;
use super::configuration::StoredConfiguration;
use super::credential::Credential;
use super::error::GithubAppError;
use super::issuer::Issuer;
use super::jwt;

/// Resolves a source row into the credential its requests should carry.
///
/// A source configured with a GitHub App gets an installation token, minted on
/// demand and reused until its safety margin; any other source gets the
/// personal access token stored against it. Callers see [`Credential`] either
/// way.
pub struct Provider {
    issuer: Issuer,
    cache: TokenCache,
    master_key: MasterKey,
    stored_credential_key: [u8; 32],
}

impl Provider {
    pub fn new(encryption_key: [u8; 32]) -> Result<Self, GithubAppError> {
        Ok(Self::with_issuer(encryption_key, Issuer::new()?))
    }

    /// A provider talking to `issuer`, for GitHub Enterprise and for tests.
    pub fn with_issuer(encryption_key: [u8; 32], issuer: Issuer) -> Self {
        Self {
            issuer,
            cache: TokenCache::new(),
            master_key: MasterKey::new(encryption_key),
            stored_credential_key: encryption_key,
        }
    }

    /// The credential for `source`, or `None` when the source carries none.
    pub async fn credential(
        &self,
        source: &SourceCredentialRow,
    ) -> Result<Option<Credential>, GithubAppError> {
        match StoredConfiguration::read(&source.config)? {
            Some(configuration) => self.installation(&configuration).await.map(Some),
            None => self.stored(source.credentials_encrypted.as_deref()),
        }
    }

    /// Drop the cached token for a source, so the next request mints a new one.
    ///
    /// Worth calling when GitHub answers `401`: an installation whose
    /// permissions changed invalidates tokens issued before the change, and
    /// the cache cannot see that happen.
    pub fn invalidate(&self, source: &SourceCredentialRow) -> Result<(), GithubAppError> {
        if let Some(configuration) = StoredConfiguration::read(&source.config)? {
            self.cache.forget(configuration.installation_id());
        }
        Ok(())
    }

    async fn installation(
        &self,
        configuration: &StoredConfiguration,
    ) -> Result<Credential, GithubAppError> {
        let installation = configuration.installation_id();
        let now = Utc::now();

        if let Some(cached) = self.cache.get(installation, now) {
            return Ok(Credential::new(cached.secret().clone()));
        }

        let signed = jwt::sign(&configuration.open(&self.master_key)?, now)?;
        let token = self.issuer.issue(&signed, installation).await?;
        let credential = Credential::new(token.secret().clone());
        self.cache.insert(installation, token);

        Ok(credential)
    }

    fn stored(&self, encrypted: Option<&str>) -> Result<Option<Credential>, GithubAppError> {
        encrypted
            .map(|encrypted| {
                crypto::decrypt(&self.stored_credential_key, encrypted)
                    .map(|token| Credential::new(SecretValue::new(token)))
                    .map_err(|_| GithubAppError::Credential)
            })
            .transpose()
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stored_credential_key.fill(0);
    }
}

impl fmt::Debug for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Provider")
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, SecondsFormat};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::services::github_app::identifier::{ApplicationId, InstallationId};
    use crate::services::github_app::testing::TEST_PRIVATE_KEY;

    const KEY: [u8; 32] = [7u8; 32];

    fn app_source(encryption_key: [u8; 32]) -> SourceCredentialRow {
        let stored = StoredConfiguration::seal(
            ApplicationId::new(12345),
            InstallationId::new(67890),
            &SecretValue::new(TEST_PRIVATE_KEY),
            &MasterKey::new(encryption_key),
        )
        .expect("seal");

        let mut config = json!({ "owner": "zone-dev", "repo": "zone" });
        stored.write(&mut config).expect("write");

        SourceCredentialRow {
            config,
            credentials_encrypted: None,
        }
    }

    fn personal_source(encryption_key: [u8; 32], token: &str) -> SourceCredentialRow {
        SourceCredentialRow {
            config: json!({ "owner": "zone-dev", "repo": "zone" }),
            credentials_encrypted: Some(crypto::encrypt(&encryption_key, token).expect("encrypt")),
        }
    }

    async fn issuing(server: &MockServer, token: &str, calls: u64) {
        let expires_at =
            (Utc::now() + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true);

        Mock::given(method("POST"))
            .and(path("/app/installations/67890/access_tokens"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "token": token,
                "expires_at": expires_at,
            })))
            .expect(calls)
            .mount(server)
            .await;
    }

    fn provider(server: &MockServer) -> Provider {
        Provider::with_issuer(KEY, Issuer::at(&server.uri()).expect("issuer"))
    }

    #[tokio::test]
    async fn a_github_app_source_yields_an_installation_token() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 1).await;

        let credential = provider(&server)
            .credential(&app_source(KEY))
            .await
            .expect("resolve")
            .expect("a configured source has a credential");

        assert_eq!(credential.expose(), "ghs_installationtokenvalue");
    }

    #[tokio::test]
    async fn a_personal_access_token_source_yields_the_stored_token() {
        let server = MockServer::start().await;

        let credential = provider(&server)
            .credential(&personal_source(KEY, "ghp_personalaccesstoken"))
            .await
            .expect("resolve")
            .expect("a configured source has a credential");

        assert_eq!(credential.expose(), "ghp_personalaccesstoken");
    }

    #[tokio::test]
    async fn both_origins_are_indistinguishable_to_the_caller() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 1).await;
        let provider = provider(&server);

        let installation = provider
            .credential(&app_source(KEY))
            .await
            .expect("resolve")
            .expect("credential");
        let personal = provider
            .credential(&personal_source(KEY, "ghp_personalaccesstoken"))
            .await
            .expect("resolve")
            .expect("credential");

        assert_eq!(
            format!("{installation:?}"),
            format!("{personal:?}"),
            "the caller must not be able to tell the two apart"
        );
    }

    #[tokio::test]
    async fn an_installation_token_is_minted_once_and_reused() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 1).await;

        let provider = provider(&server);
        let source = app_source(KEY);

        for _ in 0..5 {
            assert_eq!(
                provider
                    .credential(&source)
                    .await
                    .expect("resolve")
                    .expect("credential")
                    .expose(),
                "ghs_installationtokenvalue"
            );
        }
    }

    #[tokio::test]
    async fn invalidating_a_source_mints_a_new_token() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 2).await;

        let provider = provider(&server);
        let source = app_source(KEY);

        provider.credential(&source).await.expect("resolve");
        provider.invalidate(&source).expect("invalidate");
        provider.credential(&source).await.expect("resolve");
    }

    #[tokio::test]
    async fn a_source_with_no_credentials_yields_none() {
        let server = MockServer::start().await;
        let source = SourceCredentialRow {
            config: json!({ "owner": "zone-dev", "repo": "zone" }),
            credentials_encrypted: None,
        };

        assert!(
            provider(&server)
                .credential(&source)
                .await
                .expect("resolve")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_credential_sealed_with_another_key_fails_closed() {
        let server = MockServer::start().await;
        let source = personal_source([9u8; 32], "ghp_personalaccesstoken");

        let error = provider(&server)
            .credential(&source)
            .await
            .expect_err("a credential from another deployment must not resolve");

        assert!(matches!(error, GithubAppError::Credential));
        assert!(!error.to_string().contains("ghp_personalaccesstoken"));
    }

    #[tokio::test]
    async fn a_refused_exchange_surfaces_without_the_credentialed_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let error = provider(&server)
            .credential(&app_source(KEY))
            .await
            .expect_err("a 403 is a failure");

        let rendered = error.to_string();
        assert!(matches!(error, GithubAppError::Status(403)));
        assert!(!rendered.contains(&server.uri()));
        assert!(!rendered.contains("access_tokens"));
    }

    #[tokio::test]
    async fn nothing_the_provider_renders_carries_a_credential() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 1).await;

        let provider = provider(&server);
        provider
            .credential(&app_source(KEY))
            .await
            .expect("resolve");

        let rendered = format!("{provider:?}");
        assert!(!rendered.contains("ghs_installationtokenvalue"));
        assert!(!rendered.contains("BEGIN RSA PRIVATE KEY"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
