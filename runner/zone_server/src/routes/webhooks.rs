//! Webhook endpoints for external issue tracker synchronization

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use uuid::Uuid;

use crate::crypto;
use crate::db::sync_config::{self, SyncDirection, SyncEventDirection, SyncEventType};
use crate::db::tasks;
use crate::state::AppState;
use crate::sync::{Delivery, IssueState, SyncError, github, linear};

/// Maximum allowed webhook body size (1MB)
const MAX_WEBHOOK_BODY_SIZE: usize = 1024 * 1024;

/// Maximum allowed title length
const MAX_TITLE_LENGTH: usize = 500;

/// Maximum allowed description length
const MAX_DESCRIPTION_LENGTH: usize = 50_000;

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

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorResponse::new(message))).into_response()
}

fn success(message: String) -> Response {
    (
        StatusCode::OK,
        Json(WebhookResponse {
            success: true,
            message,
        }),
    )
        .into_response()
}

/// POST /api/webhooks/sync/{sync_config_id}/github
pub async fn github_webhook(
    State(state): State<AppState>,
    Path(sync_config_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    receive(
        &state,
        sync_config_id,
        github::PROVIDER_NAME,
        &headers,
        &body,
    )
    .await
}

/// POST /api/webhooks/sync/{sync_config_id}/linear
pub async fn linear_webhook(
    State(state): State<AppState>,
    Path(sync_config_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    receive(
        &state,
        sync_config_id,
        linear::PROVIDER_NAME,
        &headers,
        &body,
    )
    .await
}

async fn receive(
    state: &AppState,
    sync_config_id: Uuid,
    provider_name: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    if body.len() > MAX_WEBHOOK_BODY_SIZE {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "Request body too large");
    }

    let sync_config_row = match sync_config::get_sync_config(state.db(), sync_config_id).await {
        Ok(Some(config)) => config,
        Ok(None) => return error(StatusCode::NOT_FOUND, "Sync config not found"),
        Err(e) => {
            tracing::error!(
                "Database error looking up sync config {}: {}",
                sync_config_id,
                e
            );
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    if !sync_config_row.enabled {
        return error(StatusCode::BAD_REQUEST, "Sync config is disabled");
    }

    if sync_config_row.provider != provider_name {
        return error(
            StatusCode::BAD_REQUEST,
            "Invalid provider for this endpoint",
        );
    }

    let webhook_secret = match sync_config_row.webhook_secret_encrypted {
        Some(encrypted) => match crypto::decrypt(state.encryption_key(), &encrypted) {
            Ok(secret) => secret,
            Err(e) => {
                tracing::error!(
                    "Failed to decrypt webhook secret for {}: {}",
                    sync_config_id,
                    e
                );
                return error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
            }
        },
        None => return error(StatusCode::BAD_REQUEST, "Webhook secret not configured"),
    };

    let provider = match state.sync_registry().get_provider(provider_name) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to get {} provider: {}", provider_name, e);
            return error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
        }
    };

    let delivery = match provider.parse_webhook(headers, body, &webhook_secret) {
        Ok(delivery) => delivery,
        Err(SyncError::WebhookVerificationFailed(msg)) => {
            tracing::warn!(
                "{} webhook verification failed for {}: {}",
                provider_name,
                sync_config_id,
                msg
            );
            return error(StatusCode::UNAUTHORIZED, "Webhook verification failed");
        }
        Err(e) => {
            tracing::error!(
                "Failed to parse {} webhook for {}: {}",
                provider_name,
                sync_config_id,
                e
            );
            return error(StatusCode::BAD_REQUEST, "Invalid webhook payload");
        }
    };

    let logged_payload = match &delivery {
        Delivery::Issue(event) => serde_json::to_value(&event.payload),
        Delivery::Ignored(ignored) => serde_json::to_value(ignored),
    };
    if let Err(e) = sync_config::create_sync_event(
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
        tracing::error!("Failed to log webhook event for {}: {}", sync_config_id, e);
    }

    let webhook_event = match delivery {
        Delivery::Issue(event) => event,
        Delivery::Ignored(ignored) => {
            tracing::info!(
                "Ignoring {} webhook for {}: {:?}",
                provider_name,
                sync_config_id,
                ignored
            );
            return success(ignored.message());
        }
    };

    match process_webhook_event(
        state,
        sync_config_id,
        &sync_config_row.project_id,
        webhook_event,
    )
    .await
    {
        Ok(message) => success(message),
        Err(e) => {
            tracing::error!(
                "Failed to process {} webhook for {}: {}",
                provider_name,
                sync_config_id,
                e
            );

            if let Err(log_err) = sync_config::create_sync_event(
                state.db(),
                sync_config_id,
                None,
                SyncEventType::SyncError,
                SyncEventDirection::Inbound,
                None,
                Some(&e.to_string()),
            )
            .await
            {
                tracing::error!(
                    "Failed to log sync error event for {}: {}",
                    sync_config_id,
                    log_err
                );
            }

            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to process webhook",
            )
        }
    }
}

/// Process a webhook event by updating the corresponding task
async fn process_webhook_event(
    state: &AppState,
    sync_config_id: Uuid,
    _project_id: &Uuid,
    event: crate::sync::WebhookEvent,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let synced_item =
        sync_config::get_synced_item_by_external_id(state.db(), sync_config_id, &event.external_id)
            .await?;

    let synced_item = match synced_item {
        Some(item) => item,
        None => {
            // If no synced item exists and event is "created", we might want to create a task
            // For now, just log and ignore
            tracing::info!(
                "Received webhook for external ID {} but no synced item found",
                event.external_id
            );
            return Ok(format!(
                "No synced item found for external ID {}",
                event.external_id
            ));
        }
    };

    if synced_item.sync_direction == SyncDirection::Outbound {
        tracing::info!("Ignoring inbound webhook for outbound-only sync");
        return Ok("Sync is outbound-only, ignoring inbound event".to_string());
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
    let status = event.payload.state.map(|state| match state {
        IssueState::Closed => "complete",
        IssueState::InProgress => "in_progress",
        IssueState::Open => "created",
    });

    tasks::update_task(
        state.db(),
        synced_item.task_id,
        title,
        description,
        None, // acceptance_criteria
        status,
        None, // priority
        None, // project_ids
    )
    .await?;

    sync_config::update_synced_item(
        state.db(),
        synced_item.id,
        Some(serde_json::to_value(&event.payload)?),
    )
    .await?;

    sync_config::create_sync_event(
        state.db(),
        sync_config_id,
        Some(synced_item.id),
        event.event_type,
        SyncEventDirection::Inbound,
        Some(serde_json::to_value(&event.payload)?),
        None,
    )
    .await?;

    Ok(format!("Task {} updated from webhook", synced_item.task_id))
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
