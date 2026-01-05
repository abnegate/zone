//! Model management endpoints
//!
//! Handles listing, pulling, and deleting models from various sources.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::state::AppState;

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

/// Model info response
#[derive(Debug, Serialize)]
pub struct ModelResponse {
    name: String,
    size: Option<u64>,
    digest: Option<String>,
    modified_at: Option<String>,
    details: Option<ModelDetails>,
}

#[derive(Debug, Serialize)]
pub struct ModelDetails {
    format: Option<String>,
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

/// Query parameters for listing models
#[derive(Debug, Deserialize)]
pub struct ListModelsQuery {
    /// Source to list from (ollama, huggingface, modelscope)
    source: Option<String>,
    /// Search query for browsing
    search: Option<String>,
}

/// GET /api/models
pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<ListModelsQuery>,
) -> impl IntoResponse {
    let source = query.source.as_deref().unwrap_or("ollama");

    match source {
        "ollama" => list_ollama_models(state).await,
        "huggingface" => list_huggingface_models(query.search).await,
        "modelscope" => list_modelscope_models(query.search).await,
        _ => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!("Unknown source: {}", source))),
        )
            .into_response(),
    }
}

/// List models from local Ollama installation
async fn list_ollama_models(state: AppState) -> axum::response::Response {
    let ollama_host = &state.config().litellm_host;

    // Try to fetch from Ollama API
    let client = reqwest::Client::new();
    let url = format!("{}/api/tags", ollama_host);

    match client.get(&url).send().await {
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
                                }),
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

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
    size: u64,
    digest: String,
    modified_at: String,
    details: Option<OllamaModelDetails>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelDetails {
    format: Option<String>,
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

/// List models from HuggingFace (placeholder - returns sample data)
async fn list_huggingface_models(search: Option<String>) -> axum::response::Response {
    // In production, this would call the HuggingFace API
    let _ = search;
    Json(vec![
        ModelResponse {
            name: "meta-llama/Llama-2-7b-chat-hf".to_string(),
            size: None,
            digest: None,
            modified_at: None,
            details: Some(ModelDetails {
                format: Some("safetensors".to_string()),
                family: Some("llama".to_string()),
                parameter_size: Some("7B".to_string()),
                quantization_level: None,
            }),
        },
        ModelResponse {
            name: "mistralai/Mistral-7B-Instruct-v0.2".to_string(),
            size: None,
            digest: None,
            modified_at: None,
            details: Some(ModelDetails {
                format: Some("safetensors".to_string()),
                family: Some("mistral".to_string()),
                parameter_size: Some("7B".to_string()),
                quantization_level: None,
            }),
        },
    ])
    .into_response()
}

/// List models from ModelScope (placeholder - returns sample data)
async fn list_modelscope_models(search: Option<String>) -> axum::response::Response {
    // In production, this would call the ModelScope API
    let _ = search;
    Json(vec![ModelResponse {
        name: "qwen/Qwen-7B-Chat".to_string(),
        size: None,
        digest: None,
        modified_at: None,
        details: Some(ModelDetails {
            format: None,
            family: Some("qwen".to_string()),
            parameter_size: Some("7B".to_string()),
            quantization_level: None,
        }),
    }])
    .into_response()
}

/// GET /api/models/:name
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let ollama_host = &state.config().litellm_host;

    let client = reqwest::Client::new();
    let url = format!("{}/api/show", ollama_host);

    #[derive(Serialize)]
    struct ShowRequest {
        name: String,
    }

    match client
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
    let ollama_host = &state.config().litellm_host;

    let client = reqwest::Client::new();
    let url = format!("{}/api/delete", ollama_host);

    #[derive(Serialize)]
    struct DeleteRequest {
        name: String,
    }

    match client
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
