use std::fmt;
use std::future::Future;
use std::num::{NonZeroU8, NonZeroU32};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use abnegate_secret::{MasterKey, SecretValue, decrypt_value, encrypt_value, is_encrypted};
use abnegate_vcs::pull_request::{
    ApplicationClaims, ApplicationError, ApplicationId, ApplicationService, InstallationId,
    InstallationIssuer, InstalledRepository, TokenCache,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::db::github_apps::AppRow;
use crate::db::github_installations::{self, InstallationCredential, InstallationStatus};
use crate::db::projects::ProjectRow;
use crate::db::sources::SourceRow;
use crate::secret::Unseal;

use super::catalog::Catalog;
use super::credential::Credential;
use super::error::Error;
use super::flights::Flights;
use super::mint::Mint;
use super::scope::Scope;
use super::tenant::Tenant;
use super::unauthenticated::Unauthenticated;

const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;

struct GitHub {
    issuer: InstallationIssuer,
    applications: ApplicationService,
}

impl GitHub {
    fn configured(url: &str, allows_http: bool) -> Result<Self, ApplicationError> {
        Ok(match allows_http {
            true => Self {
                issuer: InstallationIssuer::configured_allowing_http(url)?,
                applications: ApplicationService::configured_allowing_http(url)?,
            },
            false => Self {
                issuer: InstallationIssuer::configured(url)?,
                applications: ApplicationService::configured(url)?,
            },
        })
    }
}

enum Standing {
    Removed,
    Suspended,
    Active,
    Unconfirmed,
}

/// One cache serves every App because GitHub numbers installations uniquely
/// per host across all Apps.
pub struct Provider {
    github: Result<GitHub, ApplicationError>,
    cache: TokenCache,
    exchange: Mutex<()>,
    flights: Flights<InstallationId, Mint>,
    catalog: Catalog,
    master_key: Arc<MasterKey>,
}

impl Provider {
    pub fn new(config: &Config, master_key: Arc<MasterKey>) -> Self {
        let github = GitHub::configured(&config.github_api_url, config.github_api_allows_http)
            .inspect_err(|error| {
                tracing::warn!(
                    %error,
                    "GitHub App installations cannot mint tokens; sources and projects connected through one fail until the GitHub API origin is fixed"
                );
            });

        Self {
            github,
            cache: TokenCache::new(),
            exchange: Mutex::default(),
            flights: Flights::default(),
            catalog: Catalog::default(),
            master_key,
        }
    }

    fn github(&self) -> Result<&GitHub, Error> {
        self.github.as_ref().map_err(|fault| {
            Error::Application(match fault {
                ApplicationError::Client => ApplicationError::Client,
                _ => ApplicationError::InvalidOrigin,
            })
        })
    }

    pub fn applications(&self) -> Result<&ApplicationService, Error> {
        Ok(&self.github()?.applications)
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub async fn repositories(
        &self,
        db: &PgPool,
        organization_id: Uuid,
        installation: Uuid,
        refresh: bool,
    ) -> Result<Arc<[InstalledRepository]>, Error> {
        self.catalog
            .repositories(organization_id, installation, refresh, || {
                self.listing(db, organization_id, installation)
            })
            .await
    }

    async fn listing(
        &self,
        db: &PgPool,
        organization_id: Uuid,
        installation: Uuid,
    ) -> Result<Vec<InstalledRepository>, Error> {
        let credential = self
            .organization_installation(db, organization_id, installation)
            .await?;
        let applications = self.applications()?;
        self.retrying(db, credential, |token| Catalog::listed(applications, token))
            .await
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

    /// Refuses an App whose stored GitHub id is negative with
    /// [`Error::NotConnected`], since GitHub never numbers one so.
    pub fn application_jwt_for(&self, app: &AppRow) -> Result<SecretValue, Error> {
        self.application_jwt(Self::application_of(app.app_id)?, &app.private_key)
    }

    async fn issued(
        &self,
        issuer: &InstallationIssuer,
        application: ApplicationId,
        sealed_key: &str,
        installation: InstallationId,
    ) -> Result<SecretValue, Error> {
        if let Some(secret) = self.cached(installation) {
            return Ok(secret);
        }

        let jwt = self.application_jwt(application, sealed_key)?;
        let token = issuer.issue(&jwt, installation).await?;
        let secret = token.secret().clone();
        let _exchange = self.exchange();
        self.cache.insert(installation, token);

        Ok(secret)
    }

    fn exchange(&self) -> MutexGuard<'_, ()> {
        self.exchange.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn discard(&self, installation: InstallationId, refused: &SecretValue) {
        let _exchange = self.exchange();
        if self
            .cached(installation)
            .is_some_and(|held| held == *refused)
        {
            self.cache.forget(installation);
        }
    }

    fn cached(&self, installation: InstallationId) -> Option<SecretValue> {
        self.cache
            .get(installation, SystemTime::now())
            .map(|token| token.secret().clone())
    }

    pub fn invalidate(&self, installation: InstallationId) {
        self.cache.forget(installation);
    }

    pub fn stored_source(&self, encrypted: Option<&str>) -> Result<Option<Credential>, Error> {
        encrypted
            .map(|encrypted| {
                self.master_key
                    .open_strict(encrypted)
                    .map(Credential::stored)
                    .map_err(|_| Error::Credential)
            })
            .transpose()
    }

    pub fn stored_project(&self, stored: Option<&str>) -> Result<Option<Credential>, Error> {
        stored
            .map(|stored| {
                self.master_key
                    .open_lenient(stored)
                    .map(Credential::stored)
                    .map_err(|_| Error::Credential)
            })
            .transpose()
    }

    pub async fn source(
        &self,
        db: &PgPool,
        source: &SourceRow,
    ) -> Result<Option<Credential>, Error> {
        match source.github_installation_id {
            Some(installation) => self
                .within(db, source.workspace_id, installation)
                .await
                .map(Some),
            None => self.stored_source(source.credentials_encrypted.as_deref()),
        }
    }

    pub async fn project(
        &self,
        db: &PgPool,
        project: &ProjectRow,
    ) -> Result<Option<Credential>, Error> {
        match project.github_installation_id {
            Some(installation) => self
                .within(db, project.workspace_id, installation)
                .await
                .map(Some),
            None => self.stored_project(project.github_access_token.as_deref()),
        }
    }

    async fn within(
        &self,
        db: &PgPool,
        workspace_id: Option<Uuid>,
        id: Uuid,
    ) -> Result<Credential, Error> {
        let workspace_id = workspace_id.ok_or(Error::NotConnected)?;
        self.installation(db, workspace_id, id).await
    }

    pub async fn installation(
        &self,
        db: &PgPool,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Credential, Error> {
        self.resolved(
            db,
            Scope {
                tenant: Tenant::Workspace(workspace_id),
                installation: id,
            },
        )
        .await
    }

    pub async fn organization_installation(
        &self,
        db: &PgPool,
        organization_id: Uuid,
        id: Uuid,
    ) -> Result<Credential, Error> {
        self.resolved(
            db,
            Scope {
                tenant: Tenant::Organization(organization_id),
                installation: id,
            },
        )
        .await
    }

    async fn resolved(&self, db: &PgPool, scope: Scope) -> Result<Credential, Error> {
        let row = Self::connected(db, scope).await?;
        self.minted(db, scope, &row).await
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
        E: Unauthenticated + From<Error>,
    {
        let Some(scope) = credential.scope() else {
            return call(credential.into_secret()).await;
        };
        let presented = credential.into_secret();
        let refused = match call(presented.clone()).await {
            Err(error) if error.unauthenticated() => error,
            answered => return answered,
        };

        match self.refreshed(db, scope, &presented).await {
            Ok(fresh) => call(fresh.into_secret()).await,
            Err(error) if error.disconnects() => Err(E::from(error)),
            Err(error) => {
                tracing::warn!(
                    %error,
                    installation = %scope.installation,
                    "GitHub refused an installation token and a fresh one could not be minted"
                );
                Err(refused)
            }
        }
    }

    async fn refreshed(
        &self,
        db: &PgPool,
        scope: Scope,
        refused: &SecretValue,
    ) -> Result<Credential, Error> {
        let row = Self::connected(db, scope).await?;
        self.discard(Self::installation_of(&row)?, refused);
        self.minted(db, scope, &row).await
    }

    async fn connected(db: &PgPool, scope: Scope) -> Result<InstallationCredential, Error> {
        let row = match scope.tenant {
            Tenant::Workspace(workspace_id) => {
                github_installations::credential(db, workspace_id, scope.installation).await?
            }
            Tenant::Organization(organization_id) => {
                github_installations::organization_credential(
                    db,
                    organization_id,
                    scope.installation,
                )
                .await?
            }
        };
        row.ok_or(Error::NotConnected)
    }

    async fn minted(
        &self,
        db: &PgPool,
        scope: Scope,
        row: &InstallationCredential,
    ) -> Result<Credential, Error> {
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

        let application = Self::application_of(row.app_id)?;

        let issuer = &self.github()?.issuer;
        if let Some(secret) = self.cached(installation) {
            return Ok(Credential::installed(secret, scope));
        }

        let flight = self.flights.board(installation);
        match flight
            .minted(|| self.issued(issuer, application, &row.private_key, installation))
            .await
        {
            Ok(secret) => Ok(Credential::installed(secret, scope)),
            Err(Error::Application(refusal @ ApplicationError::Status(FORBIDDEN | NOT_FOUND))) => {
                Err(flight
                    .confirmed(|| self.confirmed(db, row, application, installation, refusal))
                    .await)
            }
            Err(error) => Err(error),
        }
    }

    async fn confirmed(
        &self,
        db: &PgPool,
        row: &InstallationCredential,
        application: ApplicationId,
        installation: InstallationId,
        refusal: ApplicationError,
    ) -> Error {
        match self
            .standing(application, &row.private_key, installation)
            .await
        {
            Standing::Removed => {
                Self::record(github_installations::mark_revoked(db, row.id).await, row.id);
                Error::Revoked
            }
            Standing::Suspended => {
                Self::record(
                    github_installations::mark_suspended(db, row.id).await,
                    row.id,
                );
                Error::Suspended
            }
            Standing::Active | Standing::Unconfirmed => Error::Application(refusal),
        }
    }

    async fn standing(
        &self,
        application: ApplicationId,
        sealed_key: &str,
        installation: InstallationId,
    ) -> Standing {
        let asked = async {
            let applications = &self.github()?.applications;
            let jwt = self.application_jwt(application, sealed_key)?;
            let standing = match applications.fetch_installation(&jwt, installation).await {
                Ok(found) if found.suspended_at.is_some() => Standing::Suspended,
                Ok(_) => Standing::Active,
                Err(ApplicationError::Status(NOT_FOUND)) => {
                    applications
                        .list_installations(&jwt, NonZeroU32::MIN, NonZeroU8::MIN)
                        .await?;
                    Standing::Removed
                }
                Err(error) => return Err(Error::Application(error)),
            };
            Ok::<_, Error>(standing)
        };

        asked.await.unwrap_or_else(|error| {
            tracing::warn!(
                %error,
                installation = installation.get(),
                "GitHub refused an installation token, and whether the installation still exists could not be confirmed"
            );
            Standing::Unconfirmed
        })
    }

    fn application_of(app_id: i64) -> Result<ApplicationId, Error> {
        u64::try_from(app_id)
            .map(ApplicationId::new)
            .map_err(|_| Error::NotConnected)
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

impl fmt::Debug for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Provider")
            .field("available", &self.github.is_ok())
            .field("cache", &self.cache)
            .field("catalog", &self.catalog)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_vcs::{BranchName, PullRequestError, PullRequestService, Repository};
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use futures::future::join_all;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{LazyLock, Mutex};
    use std::time::Duration;
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::db::github_apps::testing::{pool, seed_organization, seed_workspace};
    use crate::secret::fixture::seal;
    use crate::secret::fixture::seal_legacy;
    use crate::services::github_app::Failure;
    use crate::services::github_app::testing::{
        TEST_PRIVATE_KEY, installation_number, installed, issuing, issuing_once, provider,
        uninstalled,
    };

    const APPLICATION: ApplicationId = ApplicationId::new(12345);

    fn key() -> Arc<MasterKey> {
        static KEY: LazyLock<Arc<MasterKey>> =
            LazyLock::new(|| Arc::new(MasterKey::generate().expect("a key")));
        Arc::clone(&KEY)
    }

    fn other_key() -> Arc<MasterKey> {
        Arc::new(MasterKey::new([9u8; 32]))
    }

    fn sealed_key(provider: &Provider) -> String {
        provider
            .seal(&SecretValue::new(TEST_PRIVATE_KEY))
            .expect("seal")
    }

    #[test]
    fn an_https_origin_makes_installations_available() {
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(provider.github.is_ok());
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
        let other = Provider::new(&crate::state::test_config(), other_key());
        let provider = Provider::new(&crate::state::test_config(), key());

        assert!(matches!(
            provider.open(&sealed_key(&other)),
            Err(Error::Unsealing)
        ));
    }

    #[test]
    fn a_stored_source_token_is_decrypted() {
        let provider = Provider::new(&crate::state::test_config(), key());
        let encrypted = seal("ghp_personalaccesstoken", &key());

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
        let encrypted = seal("ghp_personalaccesstoken", &other_key());

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
        let encrypted = seal("ghp_encrypted", &key());

        let opened = provider
            .stored_project(Some(&encrypted))
            .expect("open")
            .expect("credential");
        let legacy = provider
            .stored_project(Some("ghp_legacyplaintext"))
            .expect("open")
            .expect("credential");

        assert_eq!(opened.secret().expose(), "ghp_encrypted");
        assert_eq!(legacy.secret().expose(), "ghp_legacyplaintext");
        assert!(provider.stored_project(None).expect("open").is_none());
    }

    #[test]
    fn a_project_token_sealed_under_another_key_fails_closed() {
        let provider = Provider::new(&crate::state::test_config(), key());
        let encrypted = seal("ghp_encrypted", &other_key());

        assert!(matches!(
            provider.stored_project(Some(&encrypted)),
            Err(Error::Credential)
        ));
    }

    #[test]
    fn a_source_token_sealed_before_the_envelope_existed_is_decrypted() {
        let provider = Provider::new(&crate::state::test_config(), key());
        let sealed = seal_legacy("ghp_personalaccesstoken", &key());

        let credential = provider
            .stored_source(Some(&sealed))
            .expect("decrypt")
            .expect("a stored token yields a credential");

        assert_eq!(credential.secret().expose(), "ghp_personalaccesstoken");
    }

    const OWNER: &str = "acme";
    const REPOSITORY: &str = "widgets";

    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    fn source_row(
        workspace_id: Option<Uuid>,
        installation: Option<Uuid>,
        credentials_encrypted: Option<String>,
    ) -> SourceRow {
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
            workspace_id,
            github_installation_id: installation,
        }
    }

    fn project_row(
        workspace_id: Option<Uuid>,
        installation: Option<Uuid>,
        github_access_token: Option<String>,
    ) -> ProjectRow {
        ProjectRow {
            id: Uuid::new_v4(),
            workspace_id,
            source_id: None,
            name: REPOSITORY.to_string(),
            description: None,
            status: "active".to_string(),
            github_repo_url: Some(format!("https://github.com/{OWNER}/{REPOSITORY}")),
            github_access_token,
            created_at: None,
            updated_at: None,
            github_installation_id: installation,
            position: 0,
        }
    }

    struct Connected {
        pool: PgPool,
        provider: Provider,
        organization: Uuid,
        workspace: Uuid,
        installation: Uuid,
        number: u64,
    }

    impl Connected {
        async fn new(server: &MockServer) -> Self {
            Self::serving(provider(server, key())).await
        }

        async fn serving(provider: Provider) -> Self {
            let pool = pool().await;
            let organization = seed_organization(&pool).await;
            let workspace = seed_workspace(&pool, organization).await;
            let number = installation_number();
            let installation = installed(&pool, &provider, organization, number).await;
            Self {
                pool,
                provider,
                organization,
                workspace,
                installation,
                number,
            }
        }

        async fn resolve(&self) -> Result<Credential, Error> {
            self.provider
                .installation(&self.pool, self.workspace, self.installation)
                .await
        }

        async fn resolve_for_organization(&self) -> Result<Credential, Error> {
            self.provider
                .organization_installation(&self.pool, self.organization, self.installation)
                .await
        }

        fn source(&self, credentials_encrypted: Option<String>) -> SourceRow {
            source_row(
                Some(self.workspace),
                Some(self.installation),
                credentials_encrypted,
            )
        }

        fn project(&self, github_access_token: Option<String>) -> ProjectRow {
            project_row(
                Some(self.workspace),
                Some(self.installation),
                github_access_token,
            )
        }

        fn scope(&self) -> Option<Scope> {
            Some(Scope {
                tenant: Tenant::Workspace(self.workspace),
                installation: self.installation,
            })
        }

        async fn row(&self) -> InstallationCredential {
            github_installations::credential(&self.pool, self.workspace, self.installation)
                .await
                .expect("read the installation")
                .expect("the installation still exists")
        }

        async fn status(&self) -> InstallationStatus {
            self.row().await.status()
        }

        async fn minted(&self, row: &InstallationCredential) -> Result<Credential, Error> {
            let scope = self.scope().expect("a workspace scope");
            self.provider.minted(&self.pool, scope, row).await
        }
    }

    fn tokens_path(number: u64) -> String {
        format!("/app/installations/{number}/access_tokens")
    }

    async fn refusing(server: &MockServer, number: u64, status: u16, calls: u64) {
        Mock::given(method("POST"))
            .and(path(tokens_path(number)))
            .respond_with(ResponseTemplate::new(status))
            .expect(calls)
            .mount(server)
            .await;
    }

    fn issued(token: &str) -> ResponseTemplate {
        ResponseTemplate::new(201).set_body_json(json!({
            "token": token,
            "expires_at": (chrono::Utc::now() + chrono::Duration::hours(1))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }))
    }

    fn installation_answer(number: u64, suspended_at: Option<&str>) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "id": number,
            "app_id": 12345,
            "account": {"login": OWNER, "type": "Organization"},
            "target_type": "Organization",
            "repository_selection": "all",
            "html_url": format!("https://github.com/organizations/{OWNER}/settings/installations/{number}"),
            "suspended_at": suspended_at,
        }))
    }

    async fn confirming(server: &MockServer, number: u64, answer: ResponseTemplate, calls: u64) {
        Mock::given(method("GET"))
            .and(path(format!("/app/installations/{number}")))
            .and(header_exists("authorization"))
            .respond_with(answer)
            .expect(calls)
            .mount(server)
            .await;
    }

    async fn listing(server: &MockServer, answer: ResponseTemplate, calls: u64) {
        Mock::given(method("GET"))
            .and(path("/app/installations"))
            .respond_with(answer)
            .expect(calls)
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
    ) -> Result<BranchName, Failure<PullRequestError>> {
        assert_send(provider.retrying(db, credential, |token| async move {
            service
                .get_default_branch(repository, &token)
                .await
                .map_err(Failure::Call)
        }))
        .await
    }

    async fn default_branch_and_token(
        provider: &Provider,
        db: &PgPool,
        credential: Credential,
        service: &PullRequestService,
        repository: &Repository,
    ) -> Result<(BranchName, SecretValue), Failure<PullRequestError>> {
        assert_send(provider.retrying(db, credential, |token| async move {
            let branch = service
                .get_default_branch(repository, &token)
                .await
                .map_err(Failure::Call)?;
            Ok((branch, token))
        }))
        .await
    }

    #[tokio::test]
    async fn an_installation_token_is_minted_once_and_reused() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_installationtokenvalue", 1).await;

        for _ in 0..5 {
            let credential = connected.resolve().await.expect("mint");
            assert_eq!(credential.secret().expose(), "ghs_installationtokenvalue");
        }
    }

    #[tokio::test]
    async fn invalidating_an_installation_mints_a_new_token() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_installationtokenvalue", 2).await;

        connected.resolve().await.expect("mint");
        connected
            .provider
            .invalidate(InstallationId::new(connected.number));
        connected.resolve().await.expect("mint");
    }

    #[tokio::test]
    async fn each_installation_gets_its_own_token() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        let other_number = installation_number();
        let other = installed(
            &connected.pool,
            &connected.provider,
            connected.organization,
            other_number,
        )
        .await;
        issuing(&server, connected.number, "ghs_first", 1).await;
        issuing(&server, other_number, "ghs_second", 1).await;

        let first = connected.resolve().await.expect("mint");
        let second = connected
            .provider
            .installation(&connected.pool, connected.workspace, other)
            .await
            .expect("mint");

        assert_eq!(first.secret().expose(), "ghs_first");
        assert_eq!(second.secret().expose(), "ghs_second");
    }

    #[tokio::test]
    async fn a_refused_exchange_surfaces_without_the_url() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 403, 1).await;
        confirming(&server, connected.number, ResponseTemplate::new(403), 1).await;

        let error = connected.resolve().await.expect_err("a 403 is a failure");

        let rendered = error.to_string();
        assert!(
            matches!(error, Error::Application(ApplicationError::Status(403))),
            "{error:?}"
        );
        assert!(!rendered.contains(&server.uri()), "{rendered}");
        assert!(!rendered.contains("access_tokens"), "{rendered}");
    }

    #[tokio::test]
    async fn an_installation_token_answer_without_a_token_is_unreadable() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "", 1).await;

        let error = connected
            .resolve()
            .await
            .expect_err("an empty token is no credential");

        assert!(
            matches!(error, Error::Application(ApplicationError::Response)),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn an_unreachable_issuer_fails_without_the_credentialed_url() {
        const UNREACHABLE_ORIGIN: &str = "http://127.0.0.1:1";
        let config = Config {
            github_api_url: UNREACHABLE_ORIGIN.to_string(),
            github_api_allows_http: true,
            ..crate::state::test_config()
        };
        let connected = Connected::serving(Provider::new(&config, key())).await;

        let error = connected
            .resolve()
            .await
            .expect_err("a closed port is a failure");

        let rendered = error.to_string();
        assert!(
            matches!(error, Error::Application(ApplicationError::Transport(_))),
            "{error:?}"
        );
        assert!(
            !rendered.contains("127.0.0.1") && !rendered.contains("access_tokens"),
            "the request URL survived into the error: {rendered}"
        );
    }

    #[tokio::test]
    async fn a_plain_http_origin_cannot_mint_unless_the_operator_allowed_it() {
        let server = MockServer::start().await;
        let config = Config {
            github_api_url: server.uri(),
            ..crate::state::test_config()
        };
        let connected = Connected::serving(Provider::new(&config, key())).await;
        issuing(&server, connected.number, "ghs_installationtokenvalue", 0).await;

        let error = connected
            .resolve()
            .await
            .expect_err("an http origin the operator did not allow cannot mint");

        assert!(
            matches!(error, Error::Application(ApplicationError::InvalidOrigin)),
            "{error:?}"
        );
        assert!(!error.to_string().contains(&server.uri()), "{error}");
    }

    #[tokio::test]
    async fn an_allowed_plain_http_origin_on_a_private_host_mints() {
        let server = MockServer::start().await;
        let config = Config {
            github_api_url: server.uri(),
            github_api_allows_http: true,
            ..crate::state::test_config()
        };
        let connected = Connected::serving(Provider::new(&config, key())).await;
        issuing(&server, connected.number, "ghs_installationtokenvalue", 1).await;

        let credential = connected
            .resolve()
            .await
            .expect("a loopback origin the operator allowed mints");

        assert_eq!(credential.secret().expose(), "ghs_installationtokenvalue");
    }

    #[tokio::test]
    async fn nothing_the_provider_renders_carries_a_credential() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_installationtokenvalue", 1).await;

        let minted = connected.resolve().await.expect("mint");
        let sealed = connected.row().await.private_key;
        let stored = connected
            .provider
            .stored_project(Some("ghp_legacyplaintext"))
            .expect("open")
            .expect("credential");

        for rendered in [
            format!("{:?}", connected.provider),
            format!("{minted:?}"),
            format!("{stored:?}"),
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

    #[tokio::test]
    async fn an_installation_only_source_mints_once_for_two_calls() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_sourcetoken", 1).await;
        let source = connected.source(None);

        for _ in 0..2 {
            let credential = connected
                .provider
                .source(&connected.pool, &source)
                .await
                .expect("resolve")
                .expect("an installation yields a credential");
            assert_eq!(credential.secret().expose(), "ghs_sourcetoken");
            assert_eq!(credential.scope(), connected.scope());
        }
    }

    #[tokio::test]
    async fn concurrent_resolutions_of_a_cold_installation_mint_once() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        Mock::given(method("POST"))
            .and(path(tokens_path(connected.number)))
            .respond_with(issued("ghs_sharedtoken").set_delay(Duration::from_millis(300)))
            .expect(1)
            .mount(&server)
            .await;

        let (first, second) = tokio::join!(connected.resolve(), connected.resolve());

        for credential in [first, second] {
            assert_eq!(
                credential.expect("resolve").secret().expose(),
                "ghs_sharedtoken"
            );
        }
        assert_eq!(connected.provider.flights.len(), 0);
    }

    #[tokio::test]
    async fn concurrent_mints_share_one_failure() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        Mock::given(method("POST"))
            .and(path(tokens_path(connected.number)))
            .respond_with(ResponseTemplate::new(500).set_delay(Duration::from_millis(300)))
            .expect(1)
            .mount(&server)
            .await;

        let row = connected.row().await;

        let (first, second, third) = tokio::join!(
            connected.minted(&row),
            connected.minted(&row),
            connected.minted(&row)
        );

        for outcome in [first, second, third] {
            let error = outcome.expect_err("the one mint failed");
            assert!(
                matches!(error, Error::Application(ApplicationError::Status(500))),
                "{error:?}"
            );
        }
        assert_eq!(connected.provider.flights.len(), 0);
    }

    #[tokio::test]
    async fn a_resolution_after_a_failed_mint_landed_mints_again() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 500, 2).await;

        for _ in 0..2 {
            let error = connected.resolve().await.expect_err("the mint failed");
            assert!(
                matches!(error, Error::Application(ApplicationError::Status(500))),
                "{error:?}"
            );
        }
        assert_eq!(connected.provider.flights.len(), 0);
    }

    #[tokio::test]
    async fn an_abandoned_mint_leaves_no_flight_behind() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        Mock::given(method("POST"))
            .and(path(tokens_path(connected.number)))
            .respond_with(issued("ghs_late").set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;

        tokio::time::timeout(Duration::from_millis(200), connected.resolve())
            .await
            .expect_err("the mint outlives the caller's patience");

        assert_eq!(connected.provider.flights.len(), 0);
    }

    #[tokio::test]
    async fn concurrent_mints_share_one_confirmed_refusal() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        Mock::given(method("POST"))
            .and(path(tokens_path(connected.number)))
            .respond_with(ResponseTemplate::new(404).set_delay(Duration::from_millis(300)))
            .expect(1)
            .mount(&server)
            .await;
        uninstalled(&server, connected.number).await;
        let row = connected.row().await;

        let (first, second) = tokio::join!(connected.minted(&row), connected.minted(&row));

        for outcome in [first, second] {
            let error = outcome.expect_err("GitHub no longer knows the installation");
            assert!(matches!(error, Error::Revoked), "{error:?}");
        }
        assert_eq!(connected.status().await, InstallationStatus::Revoked);
        assert_eq!(connected.provider.flights.len(), 0);
    }

    #[tokio::test]
    async fn a_personal_access_token_source_gets_its_stored_token() {
        let server = MockServer::start().await;
        let pool = pool().await;
        let provider = provider(&server, key());
        let encrypted = seal("ghp_personalaccesstoken", &key());

        let credential = provider
            .source(&pool, &source_row(None, None, Some(encrypted)))
            .await
            .expect("resolve")
            .expect("a stored token yields a credential");

        assert_eq!(credential.secret().expose(), "ghp_personalaccesstoken");
        assert_eq!(credential.scope(), None);
        assert!(
            provider
                .source(&pool, &source_row(None, None, None))
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
                &project_row(None, None, Some("ghp_legacyplaintext".to_string())),
            )
            .await
            .expect("resolve")
            .expect("a stored token yields a credential");

        assert_eq!(credential.secret().expose(), "ghp_legacyplaintext");
        assert_eq!(credential.scope(), None);
    }

    #[tokio::test]
    async fn a_project_with_an_installation_mints_instead_of_reading_its_token() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_projecttoken", 1).await;

        let credential = connected
            .provider
            .project(
                &connected.pool,
                &connected.project(Some("ghp_stale".to_string())),
            )
            .await
            .expect("resolve")
            .expect("an installation yields a credential");

        assert_eq!(credential.secret().expose(), "ghs_projecttoken");
    }

    #[tokio::test]
    async fn another_organizations_installation_is_not_connected_and_never_mints() {
        let server = MockServer::start().await;
        let theirs = Connected::new(&server).await;
        issuing(&server, theirs.number, "ghs_theirs", 0).await;
        let organization = seed_organization(&theirs.pool).await;
        let ours = seed_workspace(&theirs.pool, organization).await;

        let source = theirs
            .provider
            .source(
                &theirs.pool,
                &source_row(Some(ours), Some(theirs.installation), None),
            )
            .await
            .expect_err("a source must not mint from another organization's installation");
        let project = theirs
            .provider
            .project(
                &theirs.pool,
                &project_row(Some(ours), Some(theirs.installation), None),
            )
            .await
            .expect_err("a project must not mint from another organization's installation");
        let direct = theirs
            .provider
            .installation(&theirs.pool, ours, theirs.installation)
            .await
            .expect_err("a workspace must not mint from another organization's installation");

        for error in [source, project, direct] {
            assert!(matches!(error, Error::NotConnected), "{error:?}");
        }
    }

    #[tokio::test]
    async fn an_installation_on_a_row_without_a_workspace_is_not_connected() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_unused", 0).await;

        let error = connected
            .provider
            .source(
                &connected.pool,
                &source_row(None, Some(connected.installation), None),
            )
            .await
            .expect_err("no workspace, no organization to check the installation against");

        assert!(matches!(error, Error::NotConnected), "{error:?}");
    }

    #[tokio::test]
    async fn a_revoked_installation_fails_closed_without_asking_github() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_unused", 0).await;
        github_installations::mark_revoked(&connected.pool, connected.installation)
            .await
            .expect("revoke");
        let encrypted = seal("ghp_fallback", &key());

        let error = connected
            .provider
            .source(&connected.pool, &connected.source(Some(encrypted)))
            .await
            .expect_err("a revoked installation must not fall back to the stored token");

        assert!(matches!(error, Error::Revoked), "{error:?}");
    }

    #[tokio::test]
    async fn a_suspended_installation_fails_closed_without_asking_github() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_unused", 0).await;
        github_installations::mark_suspended(&connected.pool, connected.installation)
            .await
            .expect("suspend");

        let error = connected
            .provider
            .project(
                &connected.pool,
                &connected.project(Some("ghp_fallback".to_string())),
            )
            .await
            .expect_err("a suspended installation must not fall back to the stored token");

        assert!(matches!(error, Error::Suspended), "{error:?}");
    }

    #[tokio::test]
    async fn a_mint_not_found_that_github_confirms_marks_the_installation_revoked() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 404, 1).await;
        uninstalled(&server, connected.number).await;

        for _ in 0..2 {
            let error = connected
                .resolve()
                .await
                .expect_err("GitHub no longer knows the installation");
            assert!(matches!(error, Error::Revoked), "{error:?}");
        }
        assert_eq!(connected.status().await, InstallationStatus::Revoked);
    }

    #[tokio::test]
    async fn a_mint_forbidden_for_a_suspended_installation_marks_it_suspended() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 403, 1).await;
        confirming(
            &server,
            connected.number,
            installation_answer(connected.number, Some("2026-01-15T12:00:00Z")),
            1,
        )
        .await;

        for _ in 0..2 {
            let error = connected
                .resolve()
                .await
                .expect_err("GitHub refuses the suspended installation");
            assert!(matches!(error, Error::Suspended), "{error:?}");
        }
        assert_eq!(connected.status().await, InstallationStatus::Suspended);
    }

    #[tokio::test]
    async fn a_mint_forbidden_for_an_active_installation_marks_nothing() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 403, 1).await;
        confirming(
            &server,
            connected.number,
            installation_answer(connected.number, None),
            1,
        )
        .await;

        let error = connected
            .resolve()
            .await
            .expect_err("GitHub refused the mint, perhaps for a rate limit");

        assert!(
            matches!(error, Error::Application(ApplicationError::Status(403))),
            "{error:?}"
        );
        assert_eq!(connected.status().await, InstallationStatus::Active);
    }

    #[tokio::test]
    async fn a_refusal_github_cannot_confirm_marks_nothing() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 403, 1).await;
        confirming(&server, connected.number, ResponseTemplate::new(403), 1).await;

        let error = connected
            .resolve()
            .await
            .expect_err("GitHub refused the mint and the check");

        assert!(
            matches!(error, Error::Application(ApplicationError::Status(403))),
            "{error:?}"
        );
        assert_eq!(connected.status().await, InstallationStatus::Active);
    }

    #[tokio::test]
    async fn an_origin_that_answers_not_found_to_everything_marks_nothing() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 404, 1).await;
        confirming(&server, connected.number, ResponseTemplate::new(404), 1).await;
        listing(&server, ResponseTemplate::new(404), 1).await;

        let error = connected
            .resolve()
            .await
            .expect_err("the GitHub API origin is not GitHub's REST API");

        assert!(
            matches!(error, Error::Application(ApplicationError::Status(404))),
            "{error:?}"
        );
        assert_eq!(connected.status().await, InstallationStatus::Active);
    }

    #[tokio::test]
    async fn any_other_mint_failure_leaves_the_installation_active_unconfirmed() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        refusing(&server, connected.number, 500, 1).await;
        confirming(&server, connected.number, ResponseTemplate::new(404), 0).await;

        let error = connected.resolve().await.expect_err("GitHub failed");

        assert!(
            matches!(error, Error::Application(ApplicationError::Status(500))),
            "{error:?}"
        );
        assert_eq!(connected.status().await, InstallationStatus::Active);
    }

    #[tokio::test]
    async fn a_missing_installation_is_not_connected() {
        let server = MockServer::start().await;
        let pool = pool().await;
        let provider = provider(&server, key());
        let organization = seed_organization(&pool).await;
        let workspace = seed_workspace(&pool, organization).await;

        let error = provider
            .source(
                &pool,
                &source_row(Some(workspace), Some(Uuid::new_v4()), None),
            )
            .await
            .expect_err("no such installation");

        assert!(matches!(error, Error::NotConnected), "{error:?}");
    }

    #[tokio::test]
    async fn a_refused_installation_token_is_reminted_once_and_retried() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing_once(&server, connected.number, "ghs_first").await;
        issuing_once(&server, connected.number, "ghs_second").await;
        repository_answering(&server, "ghs_first", ResponseTemplate::new(401)).await;
        repository_answering(
            &server,
            "ghs_second",
            ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "trunk" })),
        )
        .await;
        let service = PullRequestService::configured_allowing_http(&server.uri()).expect("service");
        let repository = Repository::new(OWNER, REPOSITORY).expect("repository");
        let credential = connected.resolve().await.expect("mint");

        let branch = assert_send(default_branch(
            &connected.provider,
            &connected.pool,
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
        let connected = Connected::new(&server).await;
        issuing_once(&server, connected.number, "ghs_first").await;
        issuing_once(&server, connected.number, "ghs_second").await;
        repository_answering(&server, "ghs_first", ResponseTemplate::new(401)).await;
        repository_answering(
            &server,
            "ghs_second",
            ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "main" })),
        )
        .await;
        let service = PullRequestService::configured_allowing_http(&server.uri()).expect("service");
        let repository = Repository::new(OWNER, REPOSITORY).expect("repository");
        let credential = connected.resolve().await.expect("mint");

        let (branch, token) = assert_send(default_branch_and_token(
            &connected.provider,
            &connected.pool,
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
            .expect("open")
            .expect("credential");
        let calls = AtomicUsize::new(0);

        let error = provider
            .retrying(&pool, credential, |_token| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(Failure::Call(PullRequestError::AuthenticationFailed))
            })
            .await
            .expect_err("the stored token was refused");

        assert!(matches!(
            error,
            Failure::Call(PullRequestError::AuthenticationFailed)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failure_other_than_a_refused_token_is_not_retried() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_only", 1).await;
        let credential = connected.resolve().await.expect("mint");
        let calls = AtomicUsize::new(0);

        let error = connected
            .provider
            .retrying(&connected.pool, credential, |_token| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(Failure::Call(PullRequestError::NotFound))
            })
            .await
            .expect_err("the repository is missing");

        assert!(matches!(error, Failure::Call(PullRequestError::NotFound)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_remint_returns_the_original_error() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing_once(&server, connected.number, "ghs_first").await;
        refusing(&server, connected.number, 500, 1).await;
        let credential = connected.resolve().await.expect("mint");
        let calls = AtomicUsize::new(0);

        let error = connected
            .provider
            .retrying(&connected.pool, credential, |_token| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(Failure::Call(PullRequestError::AuthenticationFailed))
            })
            .await
            .expect_err("the token was refused and no fresh one could be minted");

        assert!(matches!(
            error,
            Failure::Call(PullRequestError::AuthenticationFailed)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_remint_refused_by_a_revocation_surfaces_the_revocation() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing_once(&server, connected.number, "ghs_first").await;
        refusing(&server, connected.number, 404, 1).await;
        uninstalled(&server, connected.number).await;
        let credential = connected.resolve().await.expect("mint");

        let error = connected
            .provider
            .retrying(&connected.pool, credential, |_token| async {
                Err::<(), _>(Failure::Call(PullRequestError::AuthenticationFailed))
            })
            .await
            .expect_err("the installation was removed");

        assert!(
            matches!(error, Failure::Connection(Error::Revoked)),
            "{error:?}"
        );
        assert_eq!(connected.status().await, InstallationStatus::Revoked);
    }

    #[tokio::test]
    async fn a_remint_of_an_installation_suspended_meanwhile_surfaces_the_suspension() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_first", 1).await;
        let credential = connected.resolve().await.expect("mint");
        github_installations::mark_suspended(&connected.pool, connected.installation)
            .await
            .expect("suspend");

        let error = connected
            .provider
            .retrying(&connected.pool, credential, |_token| async {
                Err::<(), _>(Error::Application(ApplicationError::Status(401)))
            })
            .await
            .expect_err("the installation was suspended");

        assert!(matches!(error, Error::Suspended), "{error:?}");
    }

    #[tokio::test]
    async fn a_remint_of_an_installation_removed_meanwhile_surfaces_not_connected() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_first", 1).await;
        let credential = connected.resolve().await.expect("mint");
        github_installations::delete(
            &connected.pool,
            connected.organization,
            connected.installation,
        )
        .await
        .expect("remove the installation");

        let error = connected
            .provider
            .retrying(&connected.pool, credential, |_token| async {
                Err::<(), _>(Failure::Call(PullRequestError::AuthenticationFailed))
            })
            .await
            .expect_err("the installation is gone");

        assert!(
            matches!(error, Failure::Connection(Error::NotConnected)),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn concurrent_refusals_of_one_token_remint_once() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing_once(&server, connected.number, "ghs_first").await;
        issuing(&server, connected.number, "ghs_second", 1).await;
        let credential = connected.resolve().await.expect("mint");

        let retried = join_all((0..4u64).map(|position| {
            let credential = credential.clone();
            let connected = &connected;
            async move {
                connected
                    .provider
                    .retrying(&connected.pool, credential, move |token| async move {
                        if token.expose() == "ghs_first" {
                            tokio::time::sleep(Duration::from_millis(100 * position)).await;
                            return Err(Error::Application(ApplicationError::Status(401)));
                        }
                        Ok(token)
                    })
                    .await
            }
        }))
        .await;

        for outcome in retried {
            assert_eq!(outcome.expect("retried").expose(), "ghs_second");
        }
    }

    fn app_row(provider: &Provider, app_id: i64) -> AppRow {
        let now = chrono::Utc::now();
        AppRow {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            app_id,
            slug: "zone-app".to_string(),
            name: "Zone".to_string(),
            owner_login: "acme".to_string(),
            owner_type: crate::db::github_apps::OwnerType::Organization,
            html_url: "https://github.com/apps/zone-app".to_string(),
            client_id: "Iv1.client".to_string(),
            client_secret: "ENC[v1:client-secret]".to_string(),
            private_key: sealed_key(provider),
            webhook_secret: None,
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn the_application_jwt_for_an_app_names_its_github_id() {
        let provider = Provider::new(&crate::state::test_config(), key());

        let jwt = provider
            .application_jwt_for(&app_row(&provider, 12345))
            .expect("sign");

        let claims = jwt.expose().split('.').nth(1).expect("claims part");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).expect("base64")).expect("json");
        assert_eq!(claims["iss"], "12345");
    }

    #[test]
    fn the_application_jwt_for_an_app_with_a_negative_id_is_refused() {
        let provider = Provider::new(&crate::state::test_config(), key());

        let error = provider
            .application_jwt_for(&app_row(&provider, -1))
            .expect_err("GitHub never numbers an App below zero");

        assert!(matches!(error, Error::NotConnected), "{error:?}");
    }

    #[test]
    fn applications_are_available_only_from_a_usable_origin() {
        let available = Provider::new(&crate::state::test_config(), key());
        let unusable = Provider::new(
            &Config {
                github_api_url: "http://127.0.0.1:1".to_string(),
                ..crate::state::test_config()
            },
            key(),
        );

        assert!(available.applications().is_ok());
        assert!(matches!(
            unusable.applications(),
            Err(Error::Application(ApplicationError::InvalidOrigin))
        ));
    }

    #[tokio::test]
    async fn an_organizations_own_installation_mints_with_an_organization_scope() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_organizationtoken", 1).await;

        let credential = connected
            .resolve_for_organization()
            .await
            .expect("the organization's own installation mints");

        assert_eq!(credential.secret().expose(), "ghs_organizationtoken");
        assert_eq!(
            credential.scope(),
            Some(Scope {
                tenant: Tenant::Organization(connected.organization),
                installation: connected.installation,
            })
        );
    }

    #[tokio::test]
    async fn another_organizations_installation_never_mints_for_an_organization() {
        let server = MockServer::start().await;
        let theirs = Connected::new(&server).await;
        issuing(&server, theirs.number, "ghs_theirs", 0).await;
        let ours = seed_organization(&theirs.pool).await;

        for organization in [ours, Uuid::new_v4()] {
            let error = theirs
                .provider
                .organization_installation(&theirs.pool, organization, theirs.installation)
                .await
                .expect_err("an organization must not mint from another's installation");
            assert!(matches!(error, Error::NotConnected), "{error:?}");
        }
    }

    #[tokio::test]
    async fn a_revoked_installation_fails_closed_for_an_organization_without_asking_github() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing(&server, connected.number, "ghs_unused", 0).await;
        github_installations::mark_revoked(&connected.pool, connected.installation)
            .await
            .expect("revoke");

        let error = connected
            .resolve_for_organization()
            .await
            .expect_err("a revoked installation must not mint");

        assert!(matches!(error, Error::Revoked), "{error:?}");
    }

    #[tokio::test]
    async fn a_refused_organization_scoped_token_is_reminted_once_and_retried() {
        let server = MockServer::start().await;
        let connected = Connected::new(&server).await;
        issuing_once(&server, connected.number, "ghs_first").await;
        issuing_once(&server, connected.number, "ghs_second").await;
        let credential = connected.resolve_for_organization().await.expect("mint");
        let tokens = Mutex::new(Vec::new());

        let answered = connected
            .provider
            .retrying(&connected.pool, credential, |token| {
                let tokens = &tokens;
                async move {
                    let mut seen = tokens.lock().expect("tokens");
                    seen.push(token.expose().to_string());
                    match seen.len() {
                        1 => Err(Error::Application(ApplicationError::Status(401))),
                        _ => Ok(token),
                    }
                }
            })
            .await
            .expect("the retry succeeds with the fresh token");

        assert_eq!(answered.expose(), "ghs_second");
        assert_eq!(*tokens.lock().expect("tokens"), ["ghs_first", "ghs_second"]);
    }
}
