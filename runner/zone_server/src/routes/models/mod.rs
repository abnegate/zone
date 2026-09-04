//! Model management endpoints
//!
//! Handles listing, pulling, and deleting models from various sources.

mod providers;
mod types;

pub use providers::{
    DEFAULT_PAGE_SIZE, Gpt4AllProvider, MAX_PAGE_SIZE, ModelProvider, ProviderError, get_provider,
};
pub use types::{
    BrowseQuery, BrowseResponse, ErrorResponse, ListModelsQuery, ModelDetails, ModelResponse,
    ModelSizeFilter, ModelSort,
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use once_cell::sync::Lazy;
use serde::Serialize;
use std::time::Duration;

use crate::auth::AuthUser;
use crate::state::AppState;

// =============================================================================
// Constants
// =============================================================================

const MAX_MODEL_NAME_LENGTH: usize = 256;

// =============================================================================
// Shared HTTP Client for Ollama API calls
// =============================================================================

static OLLAMA_HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build Ollama HTTP client")
});

// =============================================================================
// Validation
// =============================================================================

/// Validate model name to prevent injection attacks
fn validate_model_name(name: &str) -> Result<(), ErrorResponse> {
    if name.is_empty() || name.len() > MAX_MODEL_NAME_LENGTH {
        return Err(ErrorResponse::new("Invalid model name length"));
    }

    // Allow alphanumeric, hyphens, underscores, dots, colons, and forward slashes
    // These are common in model names like "llama3.2", "user/model", "model:tag"
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
    {
        return Err(ErrorResponse::new("Invalid characters in model name"));
    }

    Ok(())
}

/// GET /api/models
pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<ListModelsQuery>,
) -> impl IntoResponse {
    let source = query.source.as_deref().unwrap_or("ollama");
    let limit = query.limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);

    // Check if we're in "browse" mode (source param explicitly provided)
    let is_browse_mode = query.source.is_some();

    match source {
        "ollama" => {
            if is_browse_mode {
                // Browse the Ollama library for available models
                match get_provider("ollama") {
                    Ok(provider) => match provider.search(query.to_browse_query(limit)).await {
                        Ok(response) => Json(response).into_response(),
                        Err(e) => e.into_response(),
                    },
                    Err(e) => e.into_response(),
                }
            } else {
                // List locally installed models
                list_ollama_models(state).await
            }
        }
        "gpt4all" => {
            let provider = Gpt4AllProvider::new(state.config().gpt4all_models_url.clone());
            match provider.search(query.to_browse_query(limit)).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => e.into_response(),
            }
        }
        "huggingface" | "openrouter" => match get_provider(source) {
            Ok(provider) => match provider.search(query.to_browse_query(limit)).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => e.into_response(),
            },
            Err(e) => e.into_response(),
        },
        _ => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!("Unknown source: {}", source))),
        )
            .into_response(),
    }
}

/// List models from local Ollama installation
async fn list_ollama_models(state: AppState) -> axum::response::Response {
    let ollama_host = &state.config().ollama_host;

    // Try to fetch from Ollama API
    let url = format!("{}/api/tags", ollama_host);

    match OLLAMA_HTTP_CLIENT.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<OllamaTagsResponse>().await {
                    Ok(tags) => {
                        let models: Vec<ModelResponse> = tags
                            .models
                            .into_iter()
                            .map(|m| ModelResponse {
                                name: m.name,
                                size: Some(m.size),
                                digest: Some(m.digest),
                                modified_at: Some(m.modified_at),
                                details: m.details.map(|d| ModelDetails {
                                    format: d.format,
                                    family: d.family,
                                    parameter_size: d.parameter_size,
                                    quantization_level: d.quantization_level,
                                    ..Default::default()
                                }),
                                ..Default::default()
                            })
                            .collect();

                        Json(models).into_response()
                    }
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new(format!(
                            "Failed to parse response: {}",
                            e
                        ))),
                    )
                        .into_response(),
                }
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse::new("Ollama service unavailable")),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse::new(format!(
                "Failed to connect to Ollama: {}",
                e
            ))),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, serde::Deserialize)]
struct OllamaModel {
    name: String,
    size: u64,
    digest: String,
    modified_at: String,
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, serde::Deserialize)]
struct OllamaModelDetails {
    format: Option<String>,
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

// =============================================================================
// Model Details & Management
// =============================================================================

/// GET /api/models/:name
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    // Validate model name
    if let Err(e) = validate_model_name(&name) {
        return (StatusCode::BAD_REQUEST, Json(e)).into_response();
    }

    let ollama_host = &state.config().ollama_host;
    let url = format!("{}/api/show", ollama_host);

    #[derive(Serialize)]
    struct ShowRequest {
        name: String,
    }

    match OLLAMA_HTTP_CLIENT
        .post(&url)
        .json(&ShowRequest { name: name.clone() })
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(info) => Json(info).into_response(),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new(format!(
                            "Failed to parse response: {}",
                            e
                        ))),
                    )
                        .into_response(),
                }
            } else if response.status() == StatusCode::NOT_FOUND {
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new(format!("Model not found: {}", name))),
                )
                    .into_response()
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse::new("Ollama service error")),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse::new(format!(
                "Failed to connect to Ollama: {}",
                e
            ))),
        )
            .into_response(),
    }
}

/// DELETE /api/models/:name
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    // Validate model name
    if let Err(e) = validate_model_name(&name) {
        return (StatusCode::BAD_REQUEST, Json(e)).into_response();
    }

    let ollama_host = &state.config().ollama_host;
    let url = format!("{}/api/delete", ollama_host);

    #[derive(Serialize)]
    struct DeleteRequest {
        name: String,
    }

    match OLLAMA_HTTP_CLIENT
        .delete(&url)
        .json(&DeleteRequest { name: name.clone() })
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                StatusCode::NO_CONTENT.into_response()
            } else if response.status() == StatusCode::NOT_FOUND {
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse::new(format!("Model not found: {}", name))),
                )
                    .into_response()
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse::new("Ollama service error")),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse::new(format!(
                "Failed to connect to Ollama: {}",
                e
            ))),
        )
            .into_response(),
    }
}
