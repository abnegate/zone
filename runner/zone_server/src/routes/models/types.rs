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
#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct ModelResponse {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_cases: Option<Vec<String>>,
    /// Distinct downloadable sizes when a catalogue entry ships more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Vec<ModelSize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ModelDetails>,
}

/// A concrete downloadable size for a browsed model (e.g. `llama3.2:1b`).
#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct ModelSize {
    /// Name passed to `ollama pull`
    pub name: String,
    /// Human label, e.g. `1B`
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct ModelDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_required_gb: Option<u64>,
}

/// How to sort browse results
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSort {
    #[default]
    Relevance,
    DownloadsDesc,
    DownloadsAsc,
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
