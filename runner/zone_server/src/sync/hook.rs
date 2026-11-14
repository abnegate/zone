//! The GitHub webhook that delivers a sync's issue events to Zone
//!
//! A project connected to GitHub already holds a token for its repository, so
//! Zone registers the webhook itself rather than asking for it to be added by
//! hand, keeps it signing with the current secret, and removes it with the
//! sync or the project's GitHub connection. The webhook's identifier, and
//! whether it was last given the secret Zone keeps, live in the sync's
//! configuration.

use std::future::Future;

use abnegate_secret::SecretValue;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestService;
use abnegate_vcs::pull_request::Repository;
use abnegate_vcs::pull_request::WebhookEvent;
use reqwest::Url;
use serde_json::Value;
use thiserror::Error;

use super::Provider;
use super::webhook_path;
use crate::db::projects;
use crate::db::sync_config;
use crate::db::sync_config::SyncConfigRow;
use crate::services::github::refusal::Refusal;
use crate::services::github_app;
use crate::services::github_app::Credential;
use crate::services::github_app::Unauthenticated;
use crate::state::AppState;
use uuid::Uuid;

/// Where a sync's configuration keeps the identifier GitHub gave its webhook.
pub const HOOK_ID: &str = "github_webhook_id";

/// Where a sync's configuration keeps whether its webhook was given the
/// secret Zone verifies deliveries with.
pub const HOOK_SIGNED: &str = "github_webhook_signed";

const EVENTS: [WebhookEvent; 1] = [WebhookEvent::Issues];

#[derive(Debug, Error)]
pub enum HookError {
    #[error("the project holds no GitHub token to update the webhook with")]
    Tokenless,

    #[error("the project's GitHub credential could not be resolved: {0}")]
    Credential(#[from] github_app::Error),

    #[error("a credential is sealed under a key this deployment does not hold")]
    Unopenable,

    #[error("the GitHub origin is misconfigured: {0}")]
    Origin(PullRequestError),

    #[error("the sync's repository URL is not one Zone can reach: {0}")]
    Repository(PullRequestError),

    #[error("the console URL {0} cannot receive webhooks")]
    Address(String),

    #[error("GitHub refused: {}", Refusal(.0))]
    GitHub(#[from] PullRequestError),

    #[error("the sync was removed while its webhook was being registered")]
    Deleted,

    #[error("the sync changed while its webhook was being registered; try again")]
    Changed,

    #[error("the sync could not be read or recorded: {0}")]
    Database(#[from] sqlx::Error),
}

impl Unauthenticated for HookError {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::GitHub(error) if error.unauthenticated())
    }
}

/// The webhook a GitHub sync's repository delivers to Zone through.
pub struct Hook {
    service: PullRequestService,
    repository: Repository,
    credential: Credential,
    url: Url,
}

impl Hook {
    /// The webhook for `row`, or `None` when it is not a GitHub sync or its
    /// project has no credential to register one with.
    pub async fn of(state: &AppState, row: &SyncConfigRow) -> Result<Option<Self>, HookError> {
        if !matches!(row.provider.parse(), Ok(Provider::GitHub)) {
            return Ok(None);
        }
        let Some(project) = projects::get_project(state.db(), row.project_id).await? else {
            return Ok(None);
        };
        let Some(repository_url) = row
            .config
            .get("external_repo_url")
            .and_then(|value| value.as_str())
        else {
            return Ok(None);
        };
        let Some(credential) = state.github().project(state.db(), &project).await? else {
            return Ok(None);
        };
        let service = state.config().pull_requests().map_err(HookError::Origin)?;
        let repository = service
            .parse_github_url(repository_url)
            .map_err(HookError::Repository)?;
        let address = format!(
            "{}{}",
            state.config().app_base_url.trim_end_matches('/'),
            webhook_path(row.id, &row.provider)
        );
        let url = Url::parse(&address).map_err(|_| HookError::Address(address))?;
        Ok(Some(Self {
            service,
            repository,
            credential,
            url,
        }))
    }

    /// Have GitHub sign `row`'s deliveries with `secret`: the webhook Zone
    /// registered is updated, or a new one registered and recorded when there
    /// is none or it was deleted on GitHub. Answers the row as stored.
    pub async fn sign_with(
        &self,
        state: &AppState,
        row: SyncConfigRow,
        secret: &str,
    ) -> Result<SyncConfigRow, HookError> {
        let secret = &SecretValue::new(secret.to_string());
        if let Some(id) = id_of(&row) {
            let updated = self
                .authenticated(state, |token| async move {
                    self.service
                        .update_webhook(&self.repository, &token, id, &self.url, secret)
                        .await
                })
                .await;
            match updated {
                Ok(()) if registered(&row) => return Ok(row),
                Ok(()) => return record(state, &row, Some(id), true).await,
                Err(HookError::GitHub(PullRequestError::NotFound)) => {}
                Err(error) => return Err(error),
            }
        }
        let id = self
            .authenticated(state, |token| async move {
                self.service
                    .create_webhook(&self.repository, &token, &self.url, secret, &EVENTS)
                    .await
            })
            .await?;
        let recorded = record(state, &row, Some(id), true).await;
        if recorded.is_err() {
            self.delete(state, id).await;
        }
        recorded
    }

    /// Run `call` with the project's token, minted again and run once more
    /// if GitHub refuses it.
    async fn authenticated<T, Call, Answer>(
        &self,
        state: &AppState,
        call: Call,
    ) -> Result<T, HookError>
    where
        Call: Fn(SecretValue) -> Answer,
        Answer: Future<Output = Result<T, PullRequestError>>,
    {
        state
            .github()
            .retrying(state.db(), self.credential.clone(), |token| {
                let answer = call(token);
                async move { Ok(answer.await?) }
            })
            .await
    }

    /// Whether GitHub no longer holds webhook `id`.
    async fn delete(&self, state: &AppState, id: u64) -> bool {
        let deleted = self
            .authenticated(state, |token| async move {
                self.service
                    .delete_webhook(&self.repository, &token, id)
                    .await
            })
            .await;
        match deleted {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    repository = %self.repository,
                    id,
                    %error,
                    "Failed to remove a sync's GitHub webhook"
                );
                false
            }
        }
    }
}

/// The webhooks Zone registered for some syncs, resolved while their project
/// still holds the token to remove them with, so they can be removed once the
/// change that orphans them has been made.
pub struct Removal(Vec<(Hook, u64, Uuid)>);

impl Removal {
    /// The webhooks of every sync in workspace `workspace_id`.
    pub async fn of_workspace(state: &AppState, workspace_id: Uuid) -> Result<Self, sqlx::Error> {
        let rows = sync_config::list_sync_configs_in_workspace(state.db(), workspace_id).await?;
        Ok(Self::of(state, &rows).await)
    }

    /// The webhooks of every sync in organization `organization_id`.
    pub async fn of_organization(
        state: &AppState,
        organization_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        let rows =
            sync_config::list_sync_configs_in_organization(state.db(), organization_id).await?;
        Ok(Self::of(state, &rows).await)
    }

    pub async fn of(state: &AppState, rows: &[SyncConfigRow]) -> Self {
        let mut hooks = Vec::new();
        for row in rows {
            let Some(id) = id_of(row) else {
                continue;
            };
            match Hook::of(state, row).await {
                Ok(Some(hook)) => hooks.push((hook, id, row.id)),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(config = %row.id, %error, "Failed to resolve a sync's webhook");
                }
            }
        }
        Self(hooks)
    }

    /// Remove each webhook and forget it. A webhook that cannot be removed
    /// only delivers to a path that no longer verifies, so a failure is
    /// logged rather than refused.
    pub async fn run(self, state: &AppState) {
        for (hook, id, config) in self.0 {
            if hook.delete(state, id).await {
                forget(state, config, id).await;
            }
        }
    }
}

/// Remove the webhook Zone registered for `row`, if it did, and forget it.
pub async fn remove(state: &AppState, row: &SyncConfigRow) {
    Removal::of(state, std::slice::from_ref(row))
        .await
        .run(state)
        .await;
}

async fn forget(state: &AppState, config: Uuid, id: u64) {
    let Ok(Some(row)) = sync_config::get_sync_config(state.db(), config).await else {
        return;
    };
    if id_of(&row) != Some(id) {
        return;
    }
    if let Err(error) = record(state, &row, None, false).await
        && !matches!(error, HookError::Deleted)
    {
        tracing::warn!(%config, %error, "Failed to forget a sync's webhook");
    }
}

/// Record that the webhook of configuration `row` could not be given the
/// secret `failed`, so the console stops calling it registered, unless a newer
/// secret was committed since. Answers the row as stored, or as it would read
/// had the write landed.
pub async fn unsigned(state: &AppState, row: SyncConfigRow, failed: Option<&str>) -> SyncConfigRow {
    let current = match sync_config::get_sync_config(state.db(), row.id).await {
        Ok(Some(current)) => current,
        Ok(None) | Err(_) => row,
    };
    if current.webhook_secret_encrypted.as_deref() != failed {
        return current;
    }
    let Some(id) = id_of(&current) else {
        return current;
    };
    match record(state, &current, Some(id), false).await {
        Ok(updated) => updated,
        Err(error) => {
            tracing::warn!(config = %current.id, %error, "Failed to record a sync's stale webhook");
            let mut stale = current;
            if let Some(fields) = stale.config.as_object_mut() {
                fields.insert(HOOK_SIGNED.to_string(), false.into());
            }
            stale
        }
    }
}

/// The identifier of the webhook Zone registered for `row`.
pub fn id_of(row: &SyncConfigRow) -> Option<u64> {
    row.config.get(HOOK_ID).and_then(Value::as_u64)
}

/// Whether Zone registered `row`'s webhook and last gave it the secret Zone
/// verifies deliveries with.
pub fn registered(row: &SyncConfigRow) -> bool {
    id_of(row).is_some() && row.config.get(HOOK_SIGNED).and_then(Value::as_bool) == Some(true)
}

async fn record(
    state: &AppState,
    row: &SyncConfigRow,
    id: Option<u64>,
    signed: bool,
) -> Result<SyncConfigRow, HookError> {
    let mut config = row.config.clone();
    let Some(fields) = config.as_object_mut() else {
        return Ok(row.clone());
    };
    match id {
        Some(id) => {
            fields.insert(HOOK_ID.to_string(), id.into());
            fields.insert(HOOK_SIGNED.to_string(), signed.into());
        }
        None => {
            fields.remove(HOOK_ID);
            fields.remove(HOOK_SIGNED);
        }
    }
    if let Some(updated) =
        sync_config::replace_config(state.db(), row.id, &row.config, &config).await?
    {
        return Ok(updated);
    }
    match sync_config::get_sync_config(state.db(), row.id).await? {
        Some(_) => Err(HookError::Changed),
        None => Err(HookError::Deleted),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use super::*;
    use crate::config::Config;
    use crate::db::github_apps::testing::pool;
    use crate::db::github_apps::testing::seed_organization;
    use crate::db::github_apps::testing::seed_workspace;
    use crate::services::github_app::testing::installation_number;
    use crate::services::github_app::testing::installed;
    use crate::services::github_app::testing::issuing;
    use crate::services::github_app::testing::issuing_once;
    use crate::services::github_app::testing::provider;

    const WEBHOOK: u64 = 7;

    struct Hooked {
        github: MockServer,
        pool: sqlx::PgPool,
        state: AppState,
        organization: Uuid,
        number: u64,
    }

    impl Hooked {
        async fn new() -> Self {
            let github = MockServer::start().await;
            let pool = pool().await;
            let state = AppState::new(
                Config {
                    github_api_url: github.uri(),
                    github_api_allows_http: true,
                    app_base_url: "https://console.example.test".to_string(),
                    ..crate::state::test_config()
                },
                pool.clone(),
                None,
            );
            state.install_github(provider(&github, std::sync::Arc::clone(state.master_key())));
            let organization = seed_organization(&pool).await;
            Self {
                github,
                pool,
                state,
                organization,
                number: installation_number(),
            }
        }

        async fn sync(&self, webhook: Option<u64>) -> SyncConfigRow {
            let workspace = seed_workspace(&self.pool, self.organization).await;
            let installation = installed(
                &self.pool,
                self.state.github(),
                self.organization,
                self.number,
            )
            .await;
            let project = projects::create_project(&self.pool, "Hooked", None, Some(workspace))
                .await
                .unwrap();
            projects::link_github(
                &self.pool,
                project.id,
                "http://127.0.0.1/acme/project",
                None,
                Some(installation),
            )
            .await
            .unwrap();
            sync_config::create_sync_config(
                &self.pool,
                project.id,
                "github",
                true,
                match webhook {
                    Some(id) => json!({
                        "external_repo_url": "http://127.0.0.1/acme/project",
                        HOOK_ID: id,
                        HOOK_SIGNED: true,
                    }),
                    None => json!({"external_repo_url": "http://127.0.0.1/acme/project"}),
                },
                None,
            )
            .await
            .unwrap()
        }

        async fn cleanup(&self) {
            sqlx::query("DELETE FROM organizations WHERE id=$1")
                .bind(self.organization)
                .execute(&self.pool)
                .await
                .unwrap();
        }
    }

    async fn answering(github: &MockServer, verb: &str, token: &str, status: u16) {
        Mock::given(method(verb))
            .and(path(format!("/repos/acme/project/hooks/{WEBHOOK}/config")))
            .and(header("authorization", format!("Bearer {token}").as_str()))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({})))
            .expect(1)
            .mount(github)
            .await;
    }

    #[tokio::test]
    async fn an_installation_project_signs_its_webhook_with_a_minted_token() {
        let hooked = Hooked::new().await;
        issuing(&hooked.github, hooked.number, "ghs_hooktoken", 1).await;
        answering(&hooked.github, "PATCH", "ghs_hooktoken", 200).await;
        let row = hooked.sync(Some(WEBHOOK)).await;

        let hook = Hook::of(&hooked.state, &row)
            .await
            .unwrap()
            .expect("an installation project has a credential for its webhook");
        let signed = hook.sign_with(&hooked.state, row.clone(), "rotated").await;

        hooked.cleanup().await;
        assert_eq!(signed.unwrap().id, row.id);
    }

    #[tokio::test]
    async fn a_refused_installation_token_is_minted_again_to_sign_the_webhook() {
        let hooked = Hooked::new().await;
        issuing_once(&hooked.github, hooked.number, "ghs_refused").await;
        issuing(&hooked.github, hooked.number, "ghs_reminted", 1).await;
        answering(&hooked.github, "PATCH", "ghs_refused", 401).await;
        answering(&hooked.github, "PATCH", "ghs_reminted", 200).await;
        let row = hooked.sync(Some(WEBHOOK)).await;

        let hook = Hook::of(&hooked.state, &row)
            .await
            .unwrap()
            .expect("an installation project has a credential for its webhook");
        let signed = hook.sign_with(&hooked.state, row.clone(), "rotated").await;

        hooked.cleanup().await;
        assert_eq!(signed.unwrap().id, row.id);
    }

    #[tokio::test]
    async fn a_refused_installation_token_is_minted_again_to_register_the_webhook() {
        let hooked = Hooked::new().await;
        issuing_once(&hooked.github, hooked.number, "ghs_refused").await;
        issuing(&hooked.github, hooked.number, "ghs_reminted", 1).await;
        for (token, status) in [("ghs_refused", 401), ("ghs_reminted", 201)] {
            Mock::given(method("POST"))
                .and(path("/repos/acme/project/hooks"))
                .and(header("authorization", format!("Bearer {token}").as_str()))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({"id": WEBHOOK})))
                .expect(1)
                .mount(&hooked.github)
                .await;
        }
        let row = hooked.sync(None).await;

        let hook = Hook::of(&hooked.state, &row)
            .await
            .unwrap()
            .expect("an installation project has a credential for its webhook");
        let signed = hook.sign_with(&hooked.state, row, "first").await;

        hooked.cleanup().await;
        let signed = signed.unwrap();
        assert_eq!(id_of(&signed), Some(WEBHOOK));
        assert!(registered(&signed));
    }

    #[tokio::test]
    async fn a_refused_installation_token_is_minted_again_to_remove_the_webhook() {
        let hooked = Hooked::new().await;
        issuing_once(&hooked.github, hooked.number, "ghs_refused").await;
        issuing(&hooked.github, hooked.number, "ghs_reminted", 1).await;
        for (token, status) in [("ghs_refused", 401), ("ghs_reminted", 204)] {
            Mock::given(method("DELETE"))
                .and(path(format!("/repos/acme/project/hooks/{WEBHOOK}")))
                .and(header("authorization", format!("Bearer {token}").as_str()))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&hooked.github)
                .await;
        }
        let row = hooked.sync(Some(WEBHOOK)).await;

        remove(&hooked.state, &row).await;
        let forgotten = sync_config::get_sync_config(&hooked.pool, row.id)
            .await
            .unwrap()
            .expect("the sync survives its webhook");

        hooked.cleanup().await;
        assert_eq!(id_of(&forgotten), None, "the removed webhook is forgotten");
    }

    #[test]
    fn a_github_refusal_of_the_webhook_is_said_once_with_its_status() {
        let error = HookError::from(PullRequestError::refused(
            422,
            "Validation Failed: Hook already exists on this repository",
        ));
        assert_eq!(
            error.to_string(),
            "GitHub refused: 422 Unprocessable Entity: Validation Failed: Hook already exists on this repository"
        );
    }
}
