//! Shared types for model management

use abnegate_llm::catalog::{
    BrowseQuery, ModelEntry, ModelMediumFilter, ModelPage, ModelSize, ModelSizeFilter, ModelSort,
};
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

/// A model as `/api/models` lists it: the catalogue's entry plus what Zone
/// knows of it once installed.
#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct ModelResponse {
    #[serde(flatten)]
    pub entry: ModelEntry,
    /// Known completion support; absent when engine metadata is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<bool>,
    /// Whether the engine advertised tool calling for this installed model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    /// Whether chats with this model should offer a character card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_character: Option<bool>,
    /// Whether a Comfy adapter's required base files are present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_files: Option<Vec<String>>,
}

impl From<ModelEntry> for ModelResponse {
    fn from(entry: ModelEntry) -> Self {
        Self {
            entry,
            ..Self::default()
        }
    }
}

/// HuggingFace repository details used when a GGUF repo is not installed locally.
#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct HuggingFaceModelInfo {
    pub content: Option<String>,
    pub gguf_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Vec<ModelSize>>,
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
    /// Filter by medium (text, image, video, tools, ...)
    pub medium: Option<ModelMediumFilter>,
    /// The workspace whose chats pick from the installed models: an endpoint
    /// its AI settings save runs only the models they save.
    pub workspace_id: Option<Uuid>,
}

impl ListModelsQuery {
    pub fn to_browse_query(&self, limit: usize) -> BrowseQuery<'_> {
        let family = self
            .family
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"));

        let mut browse = BrowseQuery::default();
        browse.query = self.search.as_deref();
        browse.cursor = self.cursor.as_deref();
        browse.limit = limit;
        browse.sort = self.sort.unwrap_or_default();
        browse.family = family;
        browse.size = self.size.unwrap_or_default();
        browse.medium = self.medium.unwrap_or_default();
        browse
    }
}

/// Response for browsing models with pagination info
#[derive(Debug, Serialize, PartialEq)]
pub struct BrowseResponse {
    pub models: Vec<ModelResponse>,
    /// Cursor for the next page (if more results available)
    pub next_cursor: Option<String>,
}

impl From<ModelPage> for BrowseResponse {
    fn from(page: ModelPage) -> Self {
        Self {
            models: page.models.into_iter().map(ModelResponse::from).collect(),
            next_cursor: page.next_cursor,
        }
    }
}

/// Host filesystem usage for the volume that stores models.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct DiskUsage {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub percent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_query_defaults_blank_filters() {
        let query = ListModelsQuery {
            source: Some("ollama".into()),
            search: Some("qwen".into()),
            cursor: None,
            limit: Some(20),
            sort: Some(ModelSort::NameAscending),
            family: Some("all".into()),
            size: None,
            medium: None,
            workspace_id: None,
        };
        let browse = query.to_browse_query(20);
        assert_eq!(browse.query, Some("qwen"));
        assert_eq!(browse.family, None);
        assert_eq!(browse.size, ModelSizeFilter::All);
        assert_eq!(browse.medium, ModelMediumFilter::All);
    }

    #[test]
    fn browse_query_passes_medium() {
        let query = ListModelsQuery {
            source: Some("huggingface".into()),
            search: None,
            cursor: None,
            limit: None,
            sort: None,
            family: Some("llama".into()),
            size: Some(ModelSizeFilter::Small),
            medium: Some(ModelMediumFilter::Image),
            workspace_id: None,
        };
        let browse = query.to_browse_query(20);
        assert_eq!(browse.family, Some("llama"));
        assert_eq!(browse.size, ModelSizeFilter::Small);
        assert_eq!(browse.medium, ModelMediumFilter::Image);
    }

    #[test]
    fn the_query_keeps_zones_wire_names_for_sorts_and_sizes() {
        let uri: axum::http::Uri =
            "/api/models?source=ollama&q=qwen&sort=params_desc&size=xl&medium=image_generation"
                .parse()
                .expect("a request URI");
        let axum::extract::Query(query) =
            axum::extract::Query::<ListModelsQuery>::try_from_uri(&uri).expect("a models query");
        let browse = query.to_browse_query(5);
        assert_eq!(browse.query, Some("qwen"));
        assert_eq!(browse.limit, 5);
        assert_eq!(browse.sort, ModelSort::ParametersDescending);
        assert_eq!(browse.size, ModelSizeFilter::ExtraLarge);
        assert_eq!(browse.medium, ModelMediumFilter::ImageGeneration);
    }

    #[test]
    fn a_model_lists_its_entry_and_zones_fields_side_by_side() {
        let mut entry = ModelEntry::default();
        entry.name = "flux-lora.safetensors".to_string();
        entry.size = Some(7);
        let model = ModelResponse {
            entry,
            completion: Some(false),
            ready: Some(true),
            recipe_id: Some("flux".to_string()),
            ..ModelResponse::default()
        };
        assert_eq!(
            serde_json::to_value(&model).expect("a model"),
            serde_json::json!({
                "name": "flux-lora.safetensors",
                "size": 7,
                "completion": false,
                "ready": true,
                "recipe_id": "flux",
            })
        );
    }
}
