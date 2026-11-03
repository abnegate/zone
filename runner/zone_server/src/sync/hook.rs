//! The GitHub webhook that delivers a sync's issue events to Zone
//!
//! A project connected to GitHub already holds a token for its repository, so
//! Zone registers the webhook itself rather than asking for it to be added by
//! hand, keeps it signing with the current secret, and removes it with the
//! sync or the project's GitHub connection. The webhook's identifier, and
//! whether it was last given the secret Zone keeps, live in the sync's
//! configuration.

use abnegate_secret::SecretValue;
use abnegate_vcs::pull_request::{Repository, WebhookEvent};
use abnegate_vcs::{PullRequestError, PullRequestService};
use reqwest::Url;
use serde_json::Value;
use thiserror::Error;

use super::{Provider, webhook_path};
use crate::db::projects;
use crate::db::sync_config::{self, SyncConfigRow};
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

    #[error("the GitHub origin is misconfigured: {0}")]
    Origin(PullRequestError),

    #[error("the sync's repository URL is not one Zone can reach: {0}")]
    Repository(PullRequestError),

    #[error("the console URL {0} cannot receive webhooks")]
    Address(String),

    #[error("GitHub refused: {0}")]
    GitHub(#[from] PullRequestError),

    #[error("the sync was removed while its webhook was being registered")]
    Deleted,

    #[error("the sync changed while its webhook was being registered; try again")]
    Changed,

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
                Ok(()) if registered(&row) => return Ok(row),
                Ok(()) => return record(state, &row, Some(id), true).await,
                Err(PullRequestError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let id = self
            .service
            .create_webhook(&self.repository, &self.token, &self.url, &secret, &EVENTS)
            .await?;
        let recorded = record(state, &row, Some(id), true).await;
        if recorded.is_err() {
            self.delete(id).await;
        }
        recorded
    }

    /// Whether GitHub no longer holds webhook `id`.
    async fn delete(&self, id: u64) -> bool {
        match self
            .service
            .delete_webhook(&self.repository, &self.token, id)
            .await
        {
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
            if hook.delete(id).await {
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
