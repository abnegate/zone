//! One place to ask for a credential, however the source or project is wired.

use std::fmt;
use std::future::Future;
use std::time::SystemTime;

use abnegate_secret::{MasterKey, SecretValue, decrypt_value, encrypt_value, is_encrypted};
use abnegate_vcs::pull_request::{
    ApplicationClaims, ApplicationError, ApplicationId, InstallationId, InstallationIssuer,
    TokenCache,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::crypto;
use crate::db::github_installations::{self, InstallationCredential, InstallationStatus};
use crate::db::projects::ProjectRow;
use crate::db::sources::SourceRow;

use super::credential::Credential;
use super::error::Error;
use super::unauthenticated::Unauthenticated;

/// Seals App secrets at rest, mints installation tokens, and opens stored
/// personal access tokens.
///
/// One cache serves every App because one provider talks to one GitHub host,
/// and GitHub numbers installations uniquely per host across all Apps.
pub struct Provider {
    issuer: Result<InstallationIssuer, ApplicationError>,
    cache: TokenCache,
    master_key: MasterKey,
    encryption_key: [u8; 32],
}

impl Provider {
    pub fn new(config: &Config, encryption_key: [u8; 32]) -> Self {
        let issuer = match config.github_api_allows_http {
            true => InstallationIssuer::configured_allowing_http(&config.github_api_url),
            false => InstallationIssuer::configured(&config.github_api_url),
        }
        .inspect_err(|error| {
            tracing::warn!(
                %error,
                "GitHub App installations cannot mint tokens; sources and projects connected through one fail until the GitHub API origin is fixed"
            );
        });

        Self::assemble(issuer, encryption_key)
    }

    pub fn with_issuer(issuer: InstallationIssuer, encryption_key: [u8; 32]) -> Self {
        Self::assemble(Ok(issuer), encryption_key)
    }

    fn assemble(
        issuer: Result<InstallationIssuer, ApplicationError>,
        encryption_key: [u8; 32],
    ) -> Self {
        Self {
            issuer,
            cache: TokenCache::new(),
            master_key: MasterKey::new(encryption_key),
            encryption_key,
        }
    }

    fn issuer(&self) -> Result<&InstallationIssuer, Error> {
        self.issuer.as_ref().map_err(|fault| {
            Error::Application(match fault {
                ApplicationError::Client => ApplicationError::Client,
                _ => ApplicationError::InvalidOrigin,
            })
        })
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
        let issuer = self.issuer()?;

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

    pub async fn source(
        &self,
        db: &PgPool,
        source: &SourceRow,
    ) -> Result<Option<Credential>, Error> {
        match source.github_installation_id {
            Some(installation) => self.installation(db, installation).await.map(Some),
            None => self.stored_source(source.credentials_encrypted.as_deref()),
        }
    }

    pub async fn project(
        &self,
        db: &PgPool,
        project: &ProjectRow,
    ) -> Result<Option<Credential>, Error> {
        match project.github_installation_id {
            Some(installation) => self.installation(db, installation).await.map(Some),
            None => Ok(self.stored_project(project.github_access_token.as_deref())),
        }
    }

    pub async fn installation(&self, db: &PgPool, id: Uuid) -> Result<Credential, Error> {
        let row = Self::connected(db, id).await?;
        self.minted(db, &row).await
    }

    pub async fn retrying<T, E, F, Fut>(
        &self,
        db: &PgPool,
        credential: Credential,
        call: F,
    ) -> Result<T, E>
    where
        F: Fn(SecretValue) -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: Unauthenticated,
    {
        let installation = credential.installation();
        let refused = match call(credential.into_secret()).await {
            Err(error) if error.unauthenticated() => error,
            answered => return answered,
        };
        let Some(installation) = installation else {
            return Err(refused);
        };

        match self.refreshed(db, installation).await {
            Ok(fresh) => call(fresh.into_secret()).await,
            Err(error) => {
                tracing::warn!(
                    %error,
                    %installation,
                    "GitHub refused an installation token and a fresh one could not be minted"
                );
                Err(refused)
            }
        }
    }

    async fn refreshed(&self, db: &PgPool, id: Uuid) -> Result<Credential, Error> {
        let row = Self::connected(db, id).await?;
        self.invalidate(Self::installation_of(&row)?);
        self.minted(db, &row).await
    }

    async fn connected(db: &PgPool, id: Uuid) -> Result<InstallationCredential, Error> {
        github_installations::credential(db, id)
            .await?
            .ok_or(Error::NotConnected)
    }

    async fn minted(&self, db: &PgPool, row: &InstallationCredential) -> Result<Credential, Error> {
        let installation = Self::installation_of(row)?;

        match row.status() {
            InstallationStatus::Active => {}
            InstallationStatus::Revoked => {
                self.invalidate(installation);
                return Err(Error::Revoked);
            }
            InstallationStatus::Suspended => {
                self.invalidate(installation);
                return Err(Error::Suspended);
            }
        }

        let application = u64::try_from(row.app_id)
            .map(ApplicationId::new)
            .map_err(|_| Error::NotConnected)?;

        match self.mint(application, &row.private_key, installation).await {
            Ok(secret) => Ok(Credential::installed(secret, row.id)),
            Err(Error::Application(ApplicationError::Status(404))) => {
                Self::record(github_installations::mark_revoked(db, row.id).await, row.id);
                Err(Error::Revoked)
            }
            Err(Error::Application(ApplicationError::Status(403))) => {
                Self::record(
                    github_installations::mark_suspended(db, row.id).await,
                    row.id,
                );
                Err(Error::Suspended)
            }
            Err(error) => Err(error),
        }
    }

    fn installation_of(row: &InstallationCredential) -> Result<InstallationId, Error> {
        u64::try_from(row.installation_id)
            .map(InstallationId::new)
            .map_err(|_| Error::NotConnected)
    }

    fn record(marked: Result<bool, sqlx::Error>, installation: Uuid) {
        if let Err(error) = marked {
            tracing::warn!(
                %error,
                %installation,
                "GitHub refused an installation, and the refusal could not be recorded"
            );
        }
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
            .field("available", &self.issuer.is_ok())
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_vcs::{BranchName, PullRequestError, PullRequestService, Repository};
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::json;
    use std::sync::LazyLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::db::github_apps::tests::{pool, seed_organization};
    use crate::services::github_app::testing::{TEST_PRIVATE_KEY, installed, issuing, provider};

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
    async fn a_plain_http_origin_cannot_mint_unless_the_operator_allowed_it() {
        let server = MockServer::start().await;
        issuing(&server, INSTALLATION.get(), "ghs_installationtokenvalue", 0).await;
        let config = Config {
            github_api_url: server.uri(),
            ..crate::state::test_config()
        };
        let provider = Provider::new(&config, key());

        let error = provider
            .mint(APPLICATION, &sealed_key(&provider), INSTALLATION)
            .await
            .expect_err("an http origin the operator did not allow cannot mint");

        assert!(matches!(
            error,
            Error::Application(ApplicationError::InvalidOrigin)
        ));
        assert!(!error.to_string().contains(&server.uri()), "{error}");
    }

    #[tokio::test]
    async fn an_allowed_plain_http_origin_on_a_private_host_mints() {
        let server = MockServer::start().await;
        issuing(&server, INSTALLATION.get(), "ghs_installationtokenvalue", 1).await;
        let config = Config {
            github_api_url: server.uri(),
            github_api_allows_http: true,
            ..crate::state::test_config()
        };
        let provider = Provider::new(&config, key());

        let token = provider
            .mint(APPLICATION, &sealed_key(&provider), INSTALLATION)
            .await
            .expect("a loopback origin the operator allowed mints");

        assert_eq!(token.expose(), "ghs_installationtokenvalue");
    }

    #[test]
    fn an_https_origin_makes_installations_available() {
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(provider.issuer.is_ok());
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

    const GITHUB_INSTALLATION: u64 = 4_815_162_342;
    const OWNER: &str = "acme";
    const REPOSITORY: &str = "widgets";

    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    fn source_row(installation: Option<Uuid>, credentials_encrypted: Option<String>) -> SourceRow {
        SourceRow {
            id: Uuid::new_v4(),
            name: REPOSITORY.to_string(),
            source_type: "github".to_string(),
            config: json!({}),
            credentials_encrypted,
            description: None,
            url: None,
            is_active: Some(true),
            last_verified_at: None,
            last_error: None,
            created_at: None,
            updated_at: None,
            workspace_id: None,
            github_installation_id: installation,
        }
    }

    fn project_row(installation: Option<Uuid>, github_access_token: Option<String>) -> ProjectRow {
        ProjectRow {
            id: Uuid::new_v4(),
            workspace_id: None,
            source_id: None,
            name: REPOSITORY.to_string(),
            description: None,
            status: "active".to_string(),
            github_repo_url: Some(format!("https://github.com/{OWNER}/{REPOSITORY}")),
            github_access_token,
            created_at: None,
            updated_at: None,
            github_installation_id: installation,
        }
    }

    async fn connected(server: &MockServer) -> (PgPool, Provider, Uuid) {
        let pool = pool().await;
        let provider = provider(server, key());
        let organization = seed_organization(&pool).await;
        let installation = installed(&pool, &provider, organization, GITHUB_INSTALLATION).await;
        (pool, provider, installation)
    }

    async fn status(pool: &PgPool, installation: Uuid) -> InstallationStatus {
        github_installations::credential(pool, installation)
            .await
            .expect("read the installation")
            .expect("the installation still exists")
            .status()
    }

    async fn refusing(server: &MockServer, status: u16, calls: u64) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/app/installations/{GITHUB_INSTALLATION}/access_tokens"
            )))
            .respond_with(ResponseTemplate::new(status))
            .expect(calls)
            .mount(server)
            .await;
    }

    async fn issuing_once(server: &MockServer, token: &str) {
        Mock::given(method("POST"))
            .and(path(format!(
                "/app/installations/{GITHUB_INSTALLATION}/access_tokens"
            )))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "token": token,
                "expires_at": (chrono::Utc::now() + chrono::Duration::hours(1))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(server)
            .await;
    }

    async fn repository_answering(server: &MockServer, token: &str, answer: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(format!("/repos/{OWNER}/{REPOSITORY}")))
            .and(header("authorization", format!("Bearer {token}")))
            .respond_with(answer)
            .expect(1)
            .mount(server)
            .await;
    }

    async fn default_branch(
        provider: &Provider,
        db: &PgPool,
        credential: Credential,
        service: &PullRequestService,
        repository: &Repository,
    ) -> Result<BranchName, PullRequestError> {
        assert_send(provider.retrying(db, credential, |token| async move {
            service.get_default_branch(repository, &token).await
        }))
        .await
    }

    async fn default_branch_and_token(
        provider: &Provider,
        db: &PgPool,
        credential: Credential,
        service: &PullRequestService,
        repository: &Repository,
    ) -> Result<(BranchName, SecretValue), PullRequestError> {
        assert_send(provider.retrying(db, credential, |token| async move {
            let branch = service.get_default_branch(repository, &token).await?;
            Ok((branch, token))
        }))
        .await
    }

    #[tokio::test]
    async fn an_installation_only_source_mints_once_for_two_calls() {
        let server = MockServer::start().await;
        issuing(&server, GITHUB_INSTALLATION, "ghs_sourcetoken", 1).await;
        let (pool, provider, installation) = connected(&server).await;
        let source = source_row(Some(installation), None);

        for _ in 0..2 {
            let credential = provider
                .source(&pool, &source)
                .await
                .expect("resolve")
                .expect("an installation yields a credential");
            assert_eq!(credential.secret().expose(), "ghs_sourcetoken");
            assert_eq!(credential.installation(), Some(installation));
        }
    }

    #[tokio::test]
    async fn a_personal_access_token_source_gets_its_stored_token() {
        let server = MockServer::start().await;
        let pool = pool().await;
        let provider = provider(&server, key());
        let encrypted = crypto::encrypt(&key(), "ghp_personalaccesstoken").expect("encrypt");

        let credential = provider
            .source(&pool, &source_row(None, Some(encrypted)))
            .await
            .expect("resolve")
            .expect("a stored token yields a credential");

        assert_eq!(credential.secret().expose(), "ghp_personalaccesstoken");
        assert_eq!(credential.installation(), None);
        assert!(
            provider
                .source(&pool, &source_row(None, None))
                .await
                .expect("resolve")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_legacy_plaintext_project_token_resolves() {
        let server = MockServer::start().await;
        let pool = pool().await;
        let provider = provider(&server, key());

        let credential = provider
            .project(
                &pool,
                &project_row(None, Some("ghp_legacyplaintext".to_string())),
            )
            .await
            .expect("resolve")
            .expect("a stored token yields a credential");

        assert_eq!(credential.secret().expose(), "ghp_legacyplaintext");
        assert_eq!(credential.installation(), None);
    }

    #[tokio::test]
    async fn a_project_with_an_installation_mints_instead_of_reading_its_token() {
        let server = MockServer::start().await;
        issuing(&server, GITHUB_INSTALLATION, "ghs_projecttoken", 1).await;
        let (pool, provider, installation) = connected(&server).await;

        let credential = provider
            .project(
                &pool,
                &project_row(Some(installation), Some("ghp_stale".to_string())),
            )
            .await
            .expect("resolve")
            .expect("an installation yields a credential");

        assert_eq!(credential.secret().expose(), "ghs_projecttoken");
    }

    #[tokio::test]
    async fn a_revoked_installation_fails_closed_without_asking_github() {
        let server = MockServer::start().await;
        issuing(&server, GITHUB_INSTALLATION, "ghs_unused", 0).await;
        let (pool, provider, installation) = connected(&server).await;
        github_installations::mark_revoked(&pool, installation)
            .await
            .expect("revoke");
        let encrypted = crypto::encrypt(&key(), "ghp_fallback").expect("encrypt");

        let error = provider
            .source(&pool, &source_row(Some(installation), Some(encrypted)))
            .await
            .expect_err("a revoked installation must not fall back to the stored token");

        assert!(matches!(error, Error::Revoked), "{error:?}");
    }

    #[tokio::test]
    async fn a_suspended_installation_fails_closed_without_asking_github() {
        let server = MockServer::start().await;
        issuing(&server, GITHUB_INSTALLATION, "ghs_unused", 0).await;
        let (pool, provider, installation) = connected(&server).await;
        github_installations::mark_suspended(&pool, installation)
            .await
            .expect("suspend");

        let error = provider
            .project(
                &pool,
                &project_row(Some(installation), Some("ghp_fallback".to_string())),
            )
            .await
            .expect_err("a suspended installation must not fall back to the stored token");

        assert!(matches!(error, Error::Suspended), "{error:?}");
    }

    #[tokio::test]
    async fn a_mint_answered_not_found_marks_the_installation_revoked() {
        let server = MockServer::start().await;
        refusing(&server, 404, 1).await;
        let (pool, provider, installation) = connected(&server).await;

        for _ in 0..2 {
            let error = provider
                .installation(&pool, installation)
                .await
                .expect_err("GitHub no longer knows the installation");
            assert!(matches!(error, Error::Revoked), "{error:?}");
        }
        assert_eq!(
            status(&pool, installation).await,
            InstallationStatus::Revoked
        );
    }

    #[tokio::test]
    async fn a_mint_answered_forbidden_marks_the_installation_suspended() {
        let server = MockServer::start().await;
        refusing(&server, 403, 1).await;
        let (pool, provider, installation) = connected(&server).await;

        for _ in 0..2 {
            let error = provider
                .installation(&pool, installation)
                .await
                .expect_err("GitHub refuses the suspended installation");
            assert!(matches!(error, Error::Suspended), "{error:?}");
        }
        assert_eq!(
            status(&pool, installation).await,
            InstallationStatus::Suspended
        );
    }

    #[tokio::test]
    async fn any_other_mint_failure_leaves_the_installation_active() {
        let server = MockServer::start().await;
        refusing(&server, 500, 1).await;
        let (pool, provider, installation) = connected(&server).await;

        let error = provider
            .installation(&pool, installation)
            .await
            .expect_err("GitHub failed");

        assert!(
            matches!(error, Error::Application(ApplicationError::Status(500))),
            "{error:?}"
        );
        assert_eq!(
            status(&pool, installation).await,
            InstallationStatus::Active
        );
    }

    #[tokio::test]
    async fn a_missing_installation_is_not_connected() {
        let server = MockServer::start().await;
        let pool = pool().await;
        let provider = provider(&server, key());

        let error = provider
            .source(&pool, &source_row(Some(Uuid::new_v4()), None))
            .await
            .expect_err("no such installation");

        assert!(matches!(error, Error::NotConnected), "{error:?}");
    }

    #[tokio::test]
    async fn a_refused_installation_token_is_reminted_once_and_retried() {
        let server = MockServer::start().await;
        issuing_once(&server, "ghs_first").await;
        issuing_once(&server, "ghs_second").await;
        repository_answering(&server, "ghs_first", ResponseTemplate::new(401)).await;
        repository_answering(
            &server,
            "ghs_second",
            ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "trunk" })),
        )
        .await;
        let (pool, provider, installation) = connected(&server).await;
        let service = PullRequestService::configured_allowing_http(&server.uri()).expect("service");
        let repository = Repository::new(OWNER, REPOSITORY).expect("repository");
        let credential = provider
            .installation(&pool, installation)
            .await
            .expect("mint");

        let branch = assert_send(default_branch(
            &provider,
            &pool,
            credential,
            &service,
            &repository,
        ))
        .await
        .expect("the retry succeeds with the fresh token");

        assert_eq!(branch.as_str(), "trunk");
    }

    #[tokio::test]
    async fn a_retried_call_can_hand_back_the_token_it_used() {
        let server = MockServer::start().await;
        issuing_once(&server, "ghs_first").await;
        issuing_once(&server, "ghs_second").await;
        repository_answering(&server, "ghs_first", ResponseTemplate::new(401)).await;
        repository_answering(
            &server,
            "ghs_second",
            ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "main" })),
        )
        .await;
        let (pool, provider, installation) = connected(&server).await;
        let service = PullRequestService::configured_allowing_http(&server.uri()).expect("service");
        let repository = Repository::new(OWNER, REPOSITORY).expect("repository");
        let credential = provider
            .installation(&pool, installation)
            .await
            .expect("mint");

        let (branch, token) = assert_send(default_branch_and_token(
            &provider,
            &pool,
            credential,
            &service,
            &repository,
        ))
        .await
        .expect("the retry succeeds with the fresh token");

        assert_eq!(branch.as_str(), "main");
        assert_eq!(token.expose(), "ghs_second");
    }

    #[tokio::test]
    async fn a_refused_stored_token_is_not_retried() {
        let server = MockServer::start().await;
        let pool = pool().await;
        let provider = provider(&server, key());
        let credential = provider
            .stored_project(Some("ghp_personalaccesstoken"))
            .expect("credential");
        let calls = AtomicUsize::new(0);

        let error = provider
            .retrying(&pool, credential, |_token| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(PullRequestError::AuthenticationFailed)
            })
            .await
            .expect_err("the stored token was refused");

        assert!(matches!(error, PullRequestError::AuthenticationFailed));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failure_other_than_a_refused_token_is_not_retried() {
        let server = MockServer::start().await;
        issuing(&server, GITHUB_INSTALLATION, "ghs_only", 1).await;
        let (pool, provider, installation) = connected(&server).await;
        let credential = provider
            .installation(&pool, installation)
            .await
            .expect("mint");
        let calls = AtomicUsize::new(0);

        let error = provider
            .retrying(&pool, credential, |_token| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(PullRequestError::NotFound)
            })
            .await
            .expect_err("the repository is missing");

        assert!(matches!(error, PullRequestError::NotFound));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_remint_returns_the_original_error() {
        let server = MockServer::start().await;
        issuing_once(&server, "ghs_first").await;
        refusing(&server, 500, 1).await;
        let (pool, provider, installation) = connected(&server).await;
        let credential = provider
            .installation(&pool, installation)
            .await
            .expect("mint");
        let calls = AtomicUsize::new(0);

        let error = provider
            .retrying(&pool, credential, |_token| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(PullRequestError::AuthenticationFailed)
            })
            .await
            .expect_err("the token was refused and no fresh one could be minted");

        assert!(matches!(error, PullRequestError::AuthenticationFailed));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_remint_refused_by_a_revocation_returns_the_original_error() {
        let server = MockServer::start().await;
        issuing_once(&server, "ghs_first").await;
        refusing(&server, 404, 1).await;
        let (pool, provider, installation) = connected(&server).await;
        let credential = provider
            .installation(&pool, installation)
            .await
            .expect("mint");

        let error = provider
            .retrying(&pool, credential, |_token| async {
                Err::<(), _>(PullRequestError::AuthenticationFailed)
            })
            .await
            .expect_err("the installation was removed");

        assert!(matches!(error, PullRequestError::AuthenticationFailed));
        assert_eq!(
            status(&pool, installation).await,
            InstallationStatus::Revoked
        );
    }
}
