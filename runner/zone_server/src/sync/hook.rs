//! The GitHub webhook that delivers a sync's issue events to Zone
//!
//! A project connected to GitHub already holds a token for its repository, so
//! Zone registers the webhook itself rather than asking for it to be added by
//! hand, keeps it signing with the current secret, and removes it with the
//! sync. The webhook's identifier is kept in the sync's configuration.

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{Repository, WebhookEvent};
use abnegate_vcs::{PullRequestError, PullRequestService};
use reqwest::Url;
use thiserror::Error;

use super::{Provider, webhook_path};
use crate::db::projects;
use crate::db::sync_config::{self, SyncConfigRow};
use crate::state::AppState;

/// Where a sync's configuration keeps the identifier GitHub gave its webhook.
pub const HOOK_ID: &str = "github_webhook_id";

const EVENTS: [WebhookEvent; 1] = [WebhookEvent::Issues];

#[derive(Debug, Error)]
pub enum HookError {
    #[error("the GitHub origin is misconfigured: {0}")]
    Origin(PullRequestError),

    #[error("the sync's repository URL is not one Zone can reach: {0}")]
    Repository(PullRequestError),

    #[error("the console URL {0} cannot receive webhooks")]
    Address(String),

    #[error("GitHub refused: {0}")]
    GitHub(#[from] PullRequestError),

    #[error("the sync could not be read or recorded: {0}")]
    Database(#[from] sqlx::Error),
}

/// The webhook a GitHub sync's repository delivers to Zone through.
pub struct Hook {
    service: PullRequestService,
    repository: Repository,
    token: SecretValue,
    url: Url,
}

impl Hook {
    /// The webhook for `row`, or `None` when it is not a GitHub sync or its
    /// project holds no token to register one with.
    pub async fn of(state: &AppState, row: &SyncConfigRow) -> Result<Option<Self>, HookError> {
        if !matches!(row.provider.parse(), Ok(Provider::GitHub)) {
            return Ok(None);
        }
        let Some(stored) = projects::get_project(state.db(), row.project_id)
            .await?
            .and_then(|project| project.github_access_token)
        else {
            return Ok(None);
        };
        let Some(repository_url) = row
            .config
            .get("external_repo_url")
            .and_then(|value| value.as_str())
        else {
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
            token: SecretValue::new(crate::crypto::open(state.encryption_key(), &stored)),
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
        let secret = SecretValue::new(secret.to_string());
        if let Some(id) = id_of(&row) {
            match self
                .service
                .update_webhook(&self.repository, &self.token, id, &self.url, &secret)
                .await
            {
                Ok(()) => return Ok(row),
                Err(PullRequestError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let id = self
            .service
            .create_webhook(&self.repository, &self.token, &self.url, &secret, &EVENTS)
            .await?;
        let mut config = row.config.clone();
        config[HOOK_ID] = id.into();
        match sync_config::update_sync_config(state.db(), row.id, None, Some(config), None).await {
            Ok(Some(updated)) => Ok(updated),
            Ok(None) => {
                self.remove(id).await;
                Ok(row)
            }
            Err(error) => {
                self.remove(id).await;
                Err(error.into())
            }
        }
    }

    /// Remove the webhook Zone registered for `row`, if it registered one.
    pub async fn remove_for(&self, row: &SyncConfigRow) {
        if let Some(id) = id_of(row) {
            self.remove(id).await;
        }
    }

    async fn remove(&self, id: u64) {
        if let Err(error) = self
            .service
            .delete_webhook(&self.repository, &self.token, id)
            .await
        {
            tracing::warn!(
                repository = %self.repository,
                id,
                %error,
                "Failed to remove a sync's GitHub webhook"
            );
        }
    }
}

/// The identifier of the webhook Zone registered for `row`.
pub fn id_of(row: &SyncConfigRow) -> Option<u64> {
    row.config.get(HOOK_ID).and_then(|value| value.as_u64())
}
