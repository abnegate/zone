//! One place to ask for a source's credentials, however that source is wired.

use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

use abnegate_secret::MasterKey;
use abnegate_vcs::ApplicationClaims;
use abnegate_vcs::InstallationIssuer;
use abnegate_vcs::TokenCache;

use crate::db::sources::SourceCredentialRow;
use crate::secret::Unseal;

use super::configuration::StoredConfiguration;
use super::credential::Credential;
use super::error::GithubAppError;

/// Resolves a source row into the credential its requests should carry.
///
/// A source configured with a GitHub App gets an installation token, minted on
/// demand and reused until its safety margin; any other source gets the
/// personal access token stored against it. Callers see [`Credential`] either
/// way.
pub struct Provider {
    issuer: InstallationIssuer,
    cache: TokenCache,
    master_key: Arc<MasterKey>,
}

impl Provider {
    pub fn new(master_key: Arc<MasterKey>) -> Result<Self, GithubAppError> {
        Ok(Self::with_issuer(master_key, InstallationIssuer::new()?))
    }

    /// A provider talking to `issuer`, for GitHub Enterprise and for tests.
    pub fn with_issuer(master_key: Arc<MasterKey>, issuer: InstallationIssuer) -> Self {
        Self {
            issuer,
            cache: TokenCache::new(),
            master_key,
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
        let now = SystemTime::now();

        if let Some(cached) = self.cache.get(installation, now) {
            return Ok(Credential::new(cached.secret().clone()));
        }

        let opened = configuration.open(&self.master_key)?;
        let signed =
            ApplicationClaims::issue(opened.application_id(), now).sign(opened.private_key())?;
        let token = self.issuer.issue(&signed, installation).await?;
        let credential = Credential::new(token.secret().clone());
        self.cache.insert(installation, token);

        Ok(credential)
    }

    fn stored(&self, encrypted: Option<&str>) -> Result<Option<Credential>, GithubAppError> {
        encrypted
            .map(|encrypted| {
                self.master_key
                    .open_strict(encrypted)
                    .map(Credential::new)
                    .map_err(|_| GithubAppError::Credential)
            })
            .transpose()
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
    use std::sync::LazyLock;

    use abnegate_secret::SecretValue;

    use super::*;
    use chrono::Duration;
    use chrono::SecondsFormat;
    use chrono::Utc;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use crate::secret::fixture::seal;
    use crate::secret::fixture::seal_legacy;
    use crate::services::github_app::ApplicationId;
    use crate::services::github_app::InstallationId;
    use crate::services::github_app::testing::TEST_PRIVATE_KEY;

    const PERSONAL_TOKEN: &str = "ghp_personalaccesstoken";
    const UNREACHABLE_ORIGIN: &str = "http://127.0.0.1:1";
    const PLAIN_HTTP_ORIGIN: &str = "http://github.example.com/api/v3";
    const HTTPS_ORIGIN: &str = "https://github.example.com/api/v3";

    /// One arbitrary key per run. Seal and open must agree; the value itself
    /// is irrelevant, and a test that passes with any key proves more than one
    /// pinned to a constant.
    fn key() -> Arc<MasterKey> {
        static KEY: LazyLock<Arc<MasterKey>> =
            LazyLock::new(|| Arc::new(MasterKey::generate().expect("a key")));
        Arc::clone(&KEY)
    }

    fn app_source(master_key: &MasterKey) -> SourceCredentialRow {
        let stored = StoredConfiguration::seal(
            ApplicationId::new(12345),
            InstallationId::new(67890),
            &SecretValue::new(TEST_PRIVATE_KEY),
            master_key,
        )
        .expect("seal");

        let mut config = json!({ "owner": "zone-dev", "repo": "zone" });
        stored.write(&mut config).expect("write");

        SourceCredentialRow {
            config,
            credentials_encrypted: None,
        }
    }

    fn personal_source(master_key: &MasterKey, token: &str) -> SourceCredentialRow {
        stored_source(seal(token, master_key))
    }

    fn stored_source(credentials_encrypted: String) -> SourceCredentialRow {
        SourceCredentialRow {
            config: json!({ "owner": "zone-dev", "repo": "zone" }),
            credentials_encrypted: Some(credentials_encrypted),
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
        Provider::with_issuer(
            key(),
            InstallationIssuer::standing_in_for(&server.uri()).expect("issuer"),
        )
    }

    #[tokio::test]
    async fn a_github_app_source_yields_an_installation_token() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 1).await;

        let credential = provider(&server)
            .credential(&app_source(&key()))
            .await
            .expect("resolve")
            .expect("a configured source has a credential");

        assert_eq!(credential.expose(), "ghs_installationtokenvalue");
    }

    #[tokio::test]
    async fn a_personal_access_token_source_yields_the_stored_token() {
        let server = MockServer::start().await;

        let credential = provider(&server)
            .credential(&personal_source(&key(), PERSONAL_TOKEN))
            .await
            .expect("resolve")
            .expect("a configured source has a credential");

        assert_eq!(credential.expose(), PERSONAL_TOKEN);
    }

    #[tokio::test]
    async fn a_personal_access_token_sealed_before_the_envelope_existed_resolves() {
        let server = MockServer::start().await;

        let credential = provider(&server)
            .credential(&stored_source(seal_legacy(PERSONAL_TOKEN, &key())))
            .await
            .expect("resolve")
            .expect("a configured source has a credential");

        assert_eq!(credential.expose(), PERSONAL_TOKEN);
    }

    #[tokio::test]
    async fn a_personal_access_token_stored_as_plaintext_fails_closed() {
        let server = MockServer::start().await;

        let error = provider(&server)
            .credential(&stored_source(PERSONAL_TOKEN.to_string()))
            .await
            .expect_err("a credential that was never sealed must not resolve");

        assert!(matches!(error, GithubAppError::Credential));
        assert!(!error.to_string().contains(PERSONAL_TOKEN));
    }

    #[tokio::test]
    async fn both_origins_are_indistinguishable_to_the_caller() {
        let server = MockServer::start().await;
        issuing(&server, "ghs_installationtokenvalue", 1).await;
        let provider = provider(&server);

        let installation = provider
            .credential(&app_source(&key()))
            .await
            .expect("resolve")
            .expect("credential");
        let personal = provider
            .credential(&personal_source(&key(), PERSONAL_TOKEN))
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
        let source = app_source(&key());

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
        let source = app_source(&key());

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
        let source = personal_source(&MasterKey::new([9u8; 32]), PERSONAL_TOKEN);

        let error = provider(&server)
            .credential(&source)
            .await
            .expect_err("a credential from another deployment must not resolve");

        assert!(matches!(error, GithubAppError::Credential));
        assert!(!error.to_string().contains(PERSONAL_TOKEN));
    }

    #[tokio::test]
    async fn a_refused_exchange_surfaces_without_the_credentialed_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let error = provider(&server)
            .credential(&app_source(&key()))
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
            .credential(&app_source(&key()))
            .await
            .expect("resolve");

        let rendered = format!("{provider:?}");
        assert!(!rendered.contains("ghs_installationtokenvalue"));
        assert!(!rendered.contains("BEGIN RSA PRIVATE KEY"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn an_installation_token_answer_without_a_token_is_unreadable() {
        let server = MockServer::start().await;
        issuing(&server, "", 1).await;

        let error = provider(&server)
            .credential(&app_source(&key()))
            .await
            .expect_err("an empty token is no credential");

        assert!(matches!(error, GithubAppError::Response));
    }

    #[tokio::test]
    async fn an_unreachable_issuer_fails_without_the_credentialed_url() {
        let provider = Provider::with_issuer(
            key(),
            InstallationIssuer::standing_in_for(UNREACHABLE_ORIGIN).expect("issuer"),
        );

        let error = provider
            .credential(&app_source(&key()))
            .await
            .expect_err("a closed port is a failure");

        let rendered = error.to_string();
        assert!(matches!(error, GithubAppError::Transport(_)));
        assert!(
            !rendered.contains("127.0.0.1") && !rendered.contains("access_tokens"),
            "the request URL survived into the error: {rendered}"
        );
    }

    #[test]
    fn an_installation_issuer_on_plain_http_is_a_configuration_error() {
        let error = InstallationIssuer::configured(PLAIN_HTTP_ORIGIN)
            .map(|issuer| Provider::with_issuer(key(), issuer))
            .map_err(GithubAppError::from)
            .expect_err("an App JWT must not travel over plain HTTP");

        assert!(matches!(error, GithubAppError::Configuration));
        assert!(InstallationIssuer::configured(HTTPS_ORIGIN).is_ok());
    }
}
