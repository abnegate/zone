//! Webhook endpoints for external issue tracker synchronization

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{TimeDelta, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::crypto;
use crate::db::sync_config::{
    self, NewSyncedTask, SyncConfigRow, SyncDirection, SyncEventDirection, SyncEventType,
    SyncedItemRow,
};
use crate::db::{projects, tasks};
use crate::state::AppState;
use crate::sync::{
    Delivery, IssueState, Provider, Settings, SyncError, WebhookEvent, WebhookPayload,
};

/// The largest delivery body either webhook route reads (1 MB)
pub const MAX_WEBHOOK_BODY_SIZE: usize = 1024 * 1024;

/// Maximum allowed title length
const MAX_TITLE_LENGTH: usize = 500;

/// Maximum allowed description length
const MAX_DESCRIPTION_LENGTH: usize = 50_000;

/// The oldest issue a delivery can still turn into a new task
const MAX_NEW_ISSUE_AGE: TimeDelta = TimeDelta::hours(24);

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct WebhookResponse {
    success: bool,
    message: String,
}

fn success(message: &str) -> Response {
    (
        StatusCode::OK,
        Json(WebhookResponse {
            success: true,
            message: message.to_string(),
        }),
    )
        .into_response()
}

/// Why a delivery was refused before anything in it was applied
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rejection {
    /// Not shown to come from the sync's provider. Every such refusal answers
    /// alike, so a caller without the secret cannot tell a missing, disabled
    /// or secretless sync from a bad signature.
    Unverified,
    InvalidPayload,
    Unprocessed,
    Internal,
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unverified => (StatusCode::UNAUTHORIZED, "Webhook verification failed"),
            Self::InvalidPayload => (StatusCode::BAD_REQUEST, "Invalid webhook payload"),
            Self::Unprocessed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to process webhook",
            ),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };
        (status, Json(ErrorResponse::new(message))).into_response()
    }
}

/// What a verified issue delivery did
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Created,
    AlreadyLinked,
    Updated,
    Unlinked,
    AlreadyProcessed,
    Stale,
    NotLinked,
    OutboundOnly,
    OtherSource,
    NotEligible,
    NoWorkspace,
    OpenedLongAgo,
    PreviouslyUnlinked,
}

impl Outcome {
    fn message(self) -> &'static str {
        match self {
            Self::Created => "Task created from the issue",
            Self::AlreadyLinked => "Issue is already linked to a task",
            Self::Updated => "Task updated from the issue",
            Self::Unlinked => "Issue unlinked from its task",
            Self::AlreadyProcessed => "Delivery already processed",
            Self::Stale => "Delivery is no newer than the last one applied",
            Self::NotLinked => "Issue is not linked to a task",
            Self::OutboundOnly => "Sync is outbound-only, ignoring inbound event",
            Self::OtherSource => "Issue is not in the configured repository or project",
            Self::NotEligible => "Issue is not one this sync creates tasks for",
            Self::NoWorkspace => "Project has no workspace to hold a task",
            Self::OpenedLongAgo => "Issue was opened too long ago to become a task",
            Self::PreviouslyUnlinked => "Issue was unlinked from its task and is not linked again",
        }
    }
}

/// Whether an issue no task is linked to yet would become one, and where
enum Admission {
    Admitted { workspace_id: Uuid },
    Refused(Outcome),
}

/// POST /api/webhooks/sync/{sync_config_id}/github
pub async fn github_webhook(
    State(state): State<AppState>,
    Path(sync_config_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    receive(&state, sync_config_id, Provider::GitHub, &headers, &body).await
}

/// POST /api/webhooks/sync/{sync_config_id}/linear
pub async fn linear_webhook(
    State(state): State<AppState>,
    Path(sync_config_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    receive(&state, sync_config_id, Provider::Linear, &headers, &body).await
}

async fn receive(
    state: &AppState,
    sync_config_id: Uuid,
    provider: Provider,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    let provider_name = provider.as_str();
    let (config, delivery) = match verify(state, sync_config_id, provider, headers, body).await {
        Ok(verified) => verified,
        Err(rejection) => return rejection.into_response(),
    };

    log_received(state, sync_config_id, &delivery).await;

    let event = match delivery {
        Delivery::Issue(event) => event,
        Delivery::Ignored(ignored) => {
            tracing::info!("Ignoring {provider_name} webhook for {sync_config_id}: {ignored:?}");
            return success(&ignored.message());
        }
    };

    match process(state, &config, event).await {
        Ok(outcome) => success(outcome.message()),
        Err(error) => {
            tracing::error!(
                "Failed to process {provider_name} webhook for {sync_config_id}: {error}"
            );
            if let Err(logging) = sync_config::create_sync_event(
                state.db(),
                sync_config_id,
                None,
                SyncEventType::SyncError,
                SyncEventDirection::Inbound,
                None,
                Some(&error.to_string()),
            )
            .await
            {
                tracing::error!("Failed to log sync error event for {sync_config_id}: {logging}");
            }
            Rejection::Unprocessed.into_response()
        }
    }
}

/// The sync a delivery is addressed to and what the delivery holds, once its
/// signature proves it came from that sync's provider.
async fn verify(
    state: &AppState,
    sync_config_id: Uuid,
    provider: Provider,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(SyncConfigRow, Delivery), Rejection> {
    let provider_name = provider.as_str();
    let unverified = |reason: &str| {
        tracing::warn!("Refused {provider_name} webhook for {sync_config_id}: {reason}");
        Rejection::Unverified
    };

    let config = match sync_config::get_sync_config(state.db(), sync_config_id).await {
        Ok(Some(config)) => config,
        Ok(None) => return Err(unverified("no such sync")),
        Err(error) => {
            tracing::error!("Database error looking up sync config {sync_config_id}: {error}");
            return Err(Rejection::Internal);
        }
    };
    if !config.enabled {
        return Err(unverified("the sync is disabled"));
    }
    if config.provider != provider_name {
        return Err(unverified("the sync is for another provider"));
    }
    let Some(encrypted) = config.webhook_secret_encrypted.as_deref() else {
        return Err(unverified("the sync has no webhook secret"));
    };
    let secret = crypto::decrypt(state.encryption_key(), encrypted).map_err(|error| {
        tracing::error!("Failed to decrypt webhook secret for {sync_config_id}: {error}");
        Rejection::Internal
    })?;
    let tracker = state
        .sync_registry()
        .get_provider(provider_name)
        .map_err(|error| {
            tracing::error!("Failed to get {provider_name} provider: {error}");
            Rejection::Internal
        })?;

    match tracker.parse_webhook(headers, body, &secret) {
        Ok(delivery) => Ok((config, delivery)),
        Err(SyncError::WebhookVerificationFailed(reason)) => Err(unverified(&reason)),
        Err(error) => {
            tracing::error!(
                "Failed to parse {provider_name} webhook for {sync_config_id}: {error}"
            );
            Err(Rejection::InvalidPayload)
        }
    }
}

async fn log_received(state: &AppState, sync_config_id: Uuid, delivery: &Delivery) {
    let logged_payload = match delivery {
        Delivery::Issue(event) => serde_json::to_value(&event.payload),
        Delivery::Ignored(ignored) => serde_json::to_value(ignored),
    };
    if let Err(error) = sync_config::create_sync_event(
        state.db(),
        sync_config_id,
        None,
        SyncEventType::WebhookReceived,
        SyncEventDirection::Inbound,
        Some(logged_payload.unwrap_or_default()),
        None,
    )
    .await
    {
        tracing::error!("Failed to log webhook event for {sync_config_id}: {error}");
    }
}

/// Apply an issue delivery from the configured source once: to the task its
/// issue is linked to, or as a new linked task. Everything it writes,
/// including the record that it was received, commits together or not at all.
async fn process(
    state: &AppState,
    config: &SyncConfigRow,
    event: WebhookEvent,
) -> Result<Outcome, BoxError> {
    let settings = Settings::from_config(&config.config);
    if !event.origin.is_configured_source(&settings) {
        tracing::info!(
            "Ignoring issue {} for {}: not in the configured repository or project",
            event.external_id,
            config.id
        );
        return Ok(Outcome::OtherSource);
    }
    let admission = admit(state, config, &settings, &event).await?;

    let mut transaction = state.db().begin().await?;
    sync_config::lock_external_issue(&mut transaction, config.id, &event.external_id).await?;
    if let Some(delivery_id) = event.delivery_id.as_deref()
        && !sync_config::record_delivery(&mut transaction, config.id, delivery_id).await?
    {
        tracing::info!(
            "Ignoring delivery {delivery_id} for {}: already processed",
            config.id
        );
        return Ok(Outcome::AlreadyProcessed);
    }
    let linked = sync_config::lock_synced_item_by_external_id(
        &mut transaction,
        config.id,
        &event.external_id,
    )
    .await?;
    let outcome = match (linked, admission) {
        (Some(item), _) => apply_to_linked(&mut transaction, config.id, item, event).await?,
        (None, Admission::Admitted { workspace_id }) => {
            if sync_config::was_unlinked(&mut transaction, config.id, &event.external_id).await? {
                tracing::info!(
                    "Ignoring new issue {} for {}: it was unlinked from its task before",
                    event.external_id,
                    config.id
                );
                Outcome::PreviouslyUnlinked
            } else {
                link_new_issue(&mut transaction, config, &settings, workspace_id, event).await?
            }
        }
        (None, Admission::Refused(outcome)) => outcome,
    };
    transaction.commit().await?;
    Ok(outcome)
}

/// Whether the delivery's issue would become a task if no task is linked to
/// it yet: only a newly opened issue, from someone the sync takes new issues
/// from, into a project with a workspace to hold it, opened within the last
/// day so a captured body cannot be replayed into a task later.
async fn admit(
    state: &AppState,
    config: &SyncConfigRow,
    settings: &Settings,
    event: &WebhookEvent,
) -> Result<Admission, BoxError> {
    if event.event_type != SyncEventType::Create {
        return Ok(Admission::Refused(Outcome::NotLinked));
    }
    if event
        .created_at
        .is_some_and(|created_at| Utc::now() - created_at > MAX_NEW_ISSUE_AGE)
    {
        tracing::info!(
            "Ignoring new issue {} for {}: opened more than a day ago",
            event.external_id,
            config.id
        );
        return Ok(Admission::Refused(Outcome::OpenedLongAgo));
    }
    if settings.direction()? == SyncDirection::Outbound {
        return Ok(Admission::Refused(Outcome::OutboundOnly));
    }
    if !event.origin.may_become_task(settings) {
        tracing::info!(
            "Ignoring new issue {} for {}: not opened by someone with write access",
            event.external_id,
            config.id
        );
        return Ok(Admission::Refused(Outcome::NotEligible));
    }
    let workspace_id = projects::get_project(state.db(), config.project_id)
        .await?
        .and_then(|project| project.workspace_id);
    Ok(
        workspace_id.map_or(Admission::Refused(Outcome::NoWorkspace), |workspace_id| {
            Admission::Admitted { workspace_id }
        }),
    )
}

/// Follow the linked issue: a deletion unlinks it for good and leaves its
/// task as it is; anything else newer than what was last applied updates the
/// task's title and description, and its status when the issue changed state.
/// A status a live run owns is left for a later delivery to catch up, so the
/// state stored for the issue stays the one the task last followed.
async fn apply_to_linked(
    connection: &mut PgConnection,
    sync_config_id: Uuid,
    item: SyncedItemRow,
    event: WebhookEvent,
) -> Result<Outcome, BoxError> {
    if item.sync_direction == SyncDirection::Outbound {
        tracing::info!("Ignoring inbound webhook for outbound-only sync");
        return Ok(Outcome::OutboundOnly);
    }
    let previous = item
        .last_external_state
        .as_ref()
        .and_then(WebhookPayload::from_stored);
    let stale = previous.as_ref().is_some_and(|previous| {
        if event.event_type == SyncEventType::Unlink {
            event.payload.is_older_than(previous)
        } else {
            event.payload.is_stale_against(previous)
        }
    });
    if stale {
        tracing::info!(
            "Ignoring a delivery for issue {} of {sync_config_id} no newer than the last one applied",
            item.external_id
        );
        return Ok(Outcome::Stale);
    }

    if event.event_type == SyncEventType::Unlink {
        sync_config::create_sync_event(
            &mut *connection,
            sync_config_id,
            None,
            SyncEventType::Unlink,
            SyncEventDirection::Inbound,
            Some(serde_json::to_value(&event.payload)?),
            None,
        )
        .await?;
        sync_config::unlink_synced_item(&mut *connection, &item).await?;
        tracing::info!(
            "Unlinked deleted issue {} from task {} for {sync_config_id}",
            item.external_id,
            item.task_id
        );
        return Ok(Outcome::Unlinked);
    }

    let title = event
        .payload
        .title
        .as_deref()
        .map(|title| truncate("title", title, MAX_TITLE_LENGTH));
    let description = event
        .payload
        .description
        .as_deref()
        .map(|description| truncate("description", description, MAX_DESCRIPTION_LENGTH));
    let status = event
        .state_change
        .target(&event.payload, previous.as_ref())
        .map(task_status);
    let mut patch = tasks::Patch {
        id: item.task_id,
        title,
        description,
        acceptance_criteria: None,
        status,
        priority: None,
        project_ids: None,
        require_plan_approval: None,
    };
    let mut applied = event.payload.clone();
    let mut event_type = event.event_type;
    if tasks::update_task_in(&mut *connection, &patch)
        .await?
        .is_none()
        && patch.status.is_some()
    {
        tracing::info!(
            "Task {} of issue {} for {sync_config_id} has a live run, so its status waits for a later delivery",
            item.task_id,
            item.external_id
        );
        patch.status = None;
        tasks::update_task_in(&mut *connection, &patch).await?;
        applied.state = previous.and_then(|previous| previous.state);
        event_type = SyncEventType::Update;
    }

    let payload = serde_json::to_value(&applied)?;
    sync_config::update_synced_item(&mut *connection, item.id, Some(payload.clone())).await?;
    sync_config::create_sync_event(
        &mut *connection,
        sync_config_id,
        Some(item.id),
        event_type,
        SyncEventDirection::Inbound,
        Some(payload),
        None,
    )
    .await?;

    Ok(Outcome::Updated)
}

/// Turn a newly opened external issue into a linked, non-agentic task.
async fn link_new_issue(
    connection: &mut PgConnection,
    config: &SyncConfigRow,
    settings: &Settings,
    workspace_id: Uuid,
    event: WebhookEvent,
) -> Result<Outcome, BoxError> {
    let external_id = &event.external_id;
    let title = event.payload.title.as_deref().unwrap_or(external_id);
    let description = event.payload.description.as_deref().unwrap_or_default();
    let external_state = serde_json::to_value(&event.payload)?;
    let item = sync_config::create_synced_task(
        &mut *connection,
        NewSyncedTask {
            sync_config_id: config.id,
            workspace_id,
            project_id: config.project_id,
            title: truncate("title", title, MAX_TITLE_LENGTH),
            description: truncate("description", description, MAX_DESCRIPTION_LENGTH),
            external_id,
            external_url: event.url.as_deref(),
            sync_direction: settings.direction()?,
            last_external_state: Some(external_state.clone()),
        },
    )
    .await?;
    let Some(item) = item else {
        return Ok(Outcome::AlreadyLinked);
    };

    sync_config::create_sync_event(
        &mut *connection,
        config.id,
        Some(item.id),
        SyncEventType::Create,
        SyncEventDirection::Inbound,
        Some(external_state),
        None,
    )
    .await?;

    Ok(Outcome::Created)
}

fn task_status(state: IssueState) -> &'static str {
    match state {
        IssueState::Closed => "complete",
        IssueState::InProgress => "in_progress",
        IssueState::Open => "created",
    }
}

fn truncate<'a>(field: &str, text: &'a str, limit: usize) -> &'a str {
    if text.len() <= limit {
        return text;
    }
    tracing::warn!(
        "Webhook {field} too long ({} bytes), truncating",
        text.len()
    );
    &text[..text.floor_char_boundary(limit)]
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn text_within_the_limit_is_kept_whole() {
        assert_eq!(truncate("title", "héllo", 6), "héllo");
    }

    #[test]
    fn a_limit_inside_a_character_cuts_before_that_character() {
        let text = format!("a{}", "é".repeat(250));

        let truncated = truncate("title", &text, 500);

        assert_eq!(truncated, format!("a{}", "é".repeat(249)));
    }

    #[test]
    fn a_limit_on_a_character_boundary_keeps_every_byte_before_it() {
        assert_eq!(truncate("title", "日本語", 6), "日本");
    }
}
