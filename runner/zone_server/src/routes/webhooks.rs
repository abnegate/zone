//! Webhook endpoints for external issue tracker synchronization

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Serialize;
use uuid::Uuid;

use crate::crypto;
use crate::db::{sync_config, tasks};
use crate::state::AppState;
use crate::sync::{IssueState, SyncError};

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

/// POST /api/webhooks/sync/{sync_config_id}/github
pub async fn github_webhook(
    State(state): State<AppState>,
    Path(sync_config_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Check body size to prevent DoS
    if body.len() > MAX_WEBHOOK_BODY_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse::new("Request body too large")),
        )
            .into_response();
    }

    // Get sync config
    let sync_config_row = match sync_config::get_sync_config(state.db(), sync_config_id).await {
        Ok(Some(config)) => config,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Sync config not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(
                "Database error looking up sync config {}: {}",
                sync_config_id,
                e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Check if enabled
    if !sync_config_row.enabled {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Sync config is disabled")),
        )
            .into_response();
    }

    // Verify provider
    if sync_config_row.provider != "github" {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid provider for this endpoint")),
        )
            .into_response();
    }

    // Decrypt webhook secret
    let webhook_secret = match sync_config_row.webhook_secret_encrypted {
        Some(encrypted) => {
            let encryption_key_bytes = state.encryption_key();

            match crypto::decrypt(encryption_key_bytes, &encrypted) {
                Ok(secret) => secret,
                Err(e) => {
                    tracing::error!(
                        "Failed to decrypt webhook secret for {}: {}",
                        sync_config_id,
                        e
                    );
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Internal server error")),
                    )
                        .into_response();
                }
            }
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Webhook secret not configured")),
            )
                .into_response();
        }
    };

    // Get GitHub provider
    let provider = match state.sync_registry().get_provider("github") {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to get GitHub provider: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Parse webhook
    let webhook_event = match provider.parse_webhook(&headers, &body, &webhook_secret) {
        Ok(event) => event,
        Err(SyncError::WebhookVerificationFailed(msg)) => {
            tracing::warn!(
                "GitHub webhook verification failed for {}: {}",
                sync_config_id,
                msg
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Webhook verification failed")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(
                "Failed to parse GitHub webhook for {}: {}",
                sync_config_id,
                e
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Invalid webhook payload")),
            )
                .into_response();
        }
    };

    // Log webhook event (log errors but continue processing)
    if let Err(e) = sync_config::create_sync_event(
        state.db(),
        sync_config_id,
        None,
        "webhook_received",
        "inbound",
        Some(serde_json::to_value(&webhook_event.payload).unwrap_or_default()),
        None,
    )
    .await
    {
        tracing::error!("Failed to log webhook event for {}: {}", sync_config_id, e);
    }

    // Process webhook event
    match process_webhook_event(
        &state,
        sync_config_id,
        &sync_config_row.project_id,
        webhook_event,
    )
    .await
    {
        Ok(message) => (
            StatusCode::OK,
            Json(WebhookResponse {
                success: true,
                message,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(
                "Failed to process GitHub webhook for {}: {}",
                sync_config_id,
                e
            );

            // Log error event
            if let Err(log_err) = sync_config::create_sync_event(
                state.db(),
                sync_config_id,
                None,
                "sync_error",
                "inbound",
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

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to process webhook")),
            )
                .into_response()
        }
    }
}

/// POST /api/webhooks/sync/{sync_config_id}/linear
pub async fn linear_webhook(
    State(state): State<AppState>,
    Path(sync_config_id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Check body size to prevent DoS
    if body.len() > MAX_WEBHOOK_BODY_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse::new("Request body too large")),
        )
            .into_response();
    }

    // Get sync config
    let sync_config_row = match sync_config::get_sync_config(state.db(), sync_config_id).await {
        Ok(Some(config)) => config,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("Sync config not found")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(
                "Database error looking up sync config {}: {}",
                sync_config_id,
                e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Check if enabled
    if !sync_config_row.enabled {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Sync config is disabled")),
        )
            .into_response();
    }

    // Verify provider
    if sync_config_row.provider != "linear" {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid provider for this endpoint")),
        )
            .into_response();
    }

    // Decrypt webhook secret
    let webhook_secret = match sync_config_row.webhook_secret_encrypted {
        Some(encrypted) => {
            let encryption_key_bytes = state.encryption_key();

            match crypto::decrypt(encryption_key_bytes, &encrypted) {
                Ok(secret) => secret,
                Err(e) => {
                    tracing::error!(
                        "Failed to decrypt webhook secret for {}: {}",
                        sync_config_id,
                        e
                    );
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Internal server error")),
                    )
                        .into_response();
                }
            }
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Webhook secret not configured")),
            )
                .into_response();
        }
    };

    // Get Linear provider
    let provider = match state.sync_registry().get_provider("linear") {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to get Linear provider: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Parse webhook
    let webhook_event = match provider.parse_webhook(&headers, &body, &webhook_secret) {
        Ok(event) => event,
        Err(SyncError::WebhookVerificationFailed(msg)) => {
            tracing::warn!(
                "Linear webhook verification failed for {}: {}",
                sync_config_id,
                msg
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse::new("Webhook verification failed")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(
                "Failed to parse Linear webhook for {}: {}",
                sync_config_id,
                e
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Invalid webhook payload")),
            )
                .into_response();
        }
    };

    // Log webhook event (log errors but continue processing)
    if let Err(e) = sync_config::create_sync_event(
        state.db(),
        sync_config_id,
        None,
        "webhook_received",
        "inbound",
        Some(serde_json::to_value(&webhook_event.payload).unwrap_or_default()),
        None,
    )
    .await
    {
        tracing::error!("Failed to log webhook event for {}: {}", sync_config_id, e);
    }

    // Process webhook event
    match process_webhook_event(
        &state,
        sync_config_id,
        &sync_config_row.project_id,
        webhook_event,
    )
    .await
    {
        Ok(message) => (
            StatusCode::OK,
            Json(WebhookResponse {
                success: true,
                message,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(
                "Failed to process Linear webhook for {}: {}",
                sync_config_id,
                e
            );

            // Log error event
            if let Err(log_err) = sync_config::create_sync_event(
                state.db(),
                sync_config_id,
                None,
                "sync_error",
                "inbound",
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

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to process webhook")),
            )
                .into_response()
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
    // Find synced item by external ID
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

    // Check sync direction
    if synced_item.sync_direction == "outbound" {
        tracing::info!("Ignoring inbound webhook for outbound-only sync");
        return Ok("Sync is outbound-only, ignoring inbound event".to_string());
    }

    // Update task based on webhook payload with validation
    let mut title = None;
    let mut description = None;
    let mut status = None;

    if let Some(ref t) = event.payload.title {
        // Validate and truncate title if too long
        if t.len() > MAX_TITLE_LENGTH {
            tracing::warn!("Webhook title too long ({} chars), truncating", t.len());
            title = Some(&t[..MAX_TITLE_LENGTH]);
        } else {
            title = Some(t.as_str());
        }
    }

    if let Some(ref d) = event.payload.description {
        // Validate and truncate description if too long
        if d.len() > MAX_DESCRIPTION_LENGTH {
            tracing::warn!(
                "Webhook description too long ({} chars), truncating",
                d.len()
            );
            description = Some(&d[..MAX_DESCRIPTION_LENGTH]);
        } else {
            description = Some(d.as_str());
        }
    }

    // Map external state to task status
    if let Some(state_value) = event.payload.state {
        status = Some(match state_value {
            IssueState::Closed => "complete",
            IssueState::InProgress => "in_progress",
            IssueState::Open => "created",
        });
    }

    // Update the task
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

    // Update synced item
    sync_config::update_synced_item(
        state.db(),
        synced_item.id,
        Some(serde_json::to_value(&event.payload)?),
    )
    .await?;

    // Log sync event
    sync_config::create_sync_event(
        state.db(),
        sync_config_id,
        Some(synced_item.id),
        &event.event_type,
        "inbound",
        Some(serde_json::to_value(&event.payload)?),
        None,
    )
    .await?;

    Ok(format!("Task {} updated from webhook", synced_item.task_id))
}
