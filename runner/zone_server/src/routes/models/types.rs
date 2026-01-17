//! Shared types for model management

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(self)).into_response()
    }
}

/// Model info response
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ModelResponse {
    pub name: String,
    pub size: Option<u64>,
    pub digest: Option<String>,
    pub modified_at: Option<String>,
    pub details: Option<ModelDetails>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ModelDetails {
    pub format: Option<String>,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
}

/// Query parameters for listing models
#[derive(Debug, Deserialize)]
pub struct ListModelsQuery {
    /// Source to list from (ollama, huggingface, modelscope)
    pub source: Option<String>,
    /// Search query for browsing
    #[serde(alias = "q")]
    pub search: Option<String>,
    /// Pagination cursor (for providers that support cursor-based pagination)
    pub cursor: Option<String>,
    /// Pagination limit (default 20, max 100)
    pub limit: Option<usize>,
}

/// Response for browsing models with pagination info
#[derive(Debug, Serialize, PartialEq)]
pub struct BrowseResponse {
    pub models: Vec<ModelResponse>,
    /// Cursor for the next page (if more results available)
    pub next_cursor: Option<String>,
}
