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

/// How to sort browse results
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSort {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    SizeAsc,
    SizeDesc,
    ParamsAsc,
    ParamsDesc,
    UpdatedDesc,
    UpdatedAsc,
}

/// Parameter-size buckets for browse filtering
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSizeFilter {
    #[default]
    All,
    Small,
    Medium,
    Large,
    Xl,
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
    /// Sort order for browse results
    pub sort: Option<ModelSort>,
    /// Filter by model family (llama, mistral, qwen, ...)
    pub family: Option<String>,
    /// Filter by parameter-size bucket
    pub size: Option<ModelSizeFilter>,
}

/// Options passed to a model provider search
#[derive(Debug, Clone, Copy)]
pub struct BrowseQuery<'a> {
    pub query: Option<&'a str>,
    pub cursor: Option<&'a str>,
    pub limit: usize,
    pub sort: ModelSort,
    pub family: Option<&'a str>,
    pub size: ModelSizeFilter,
}

impl ListModelsQuery {
    pub fn to_browse_query(&self, limit: usize) -> BrowseQuery<'_> {
        let family = self
            .family
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"));

        BrowseQuery {
            query: self.search.as_deref(),
            cursor: self.cursor.as_deref(),
            limit,
            sort: self.sort.unwrap_or_default(),
            family,
            size: self.size.unwrap_or_default(),
        }
    }
}

/// Response for browsing models with pagination info
#[derive(Debug, Serialize, PartialEq)]
pub struct BrowseResponse {
    pub models: Vec<ModelResponse>,
    /// Cursor for the next page (if more results available)
    pub next_cursor: Option<String>,
}
