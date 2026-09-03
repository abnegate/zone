//! Model provider trait and implementations

use async_trait::async_trait;
use axum::{Json, http::StatusCode, response::IntoResponse};
use once_cell::sync::Lazy;
use std::time::Duration;

use super::types::{BrowseResponse, ErrorResponse, ModelDetails, ModelResponse};

// =============================================================================
// Constants
// =============================================================================

pub const DEFAULT_PAGE_SIZE: usize = 20;
pub const MAX_PAGE_SIZE: usize = 100;
const HTTP_TIMEOUT_SECS: u64 = 30;
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 5;
const POOL_MAX_IDLE_PER_HOST: usize = 10;
const POOL_IDLE_TIMEOUT_SECS: u64 = 90;
/// Ollama publishes exact blob sizes in its registry manifests; the library
/// listing page carries no size at all, so each browsed model needs one lookup.
const OLLAMA_REGISTRY_URL: &str = "https://registry.ollama.ai/v2/library";
const OLLAMA_SIZE_LOOKUP_CONCURRENCY: usize = 8;

// =============================================================================
// Shared HTTP Client
// =============================================================================

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(POOL_IDLE_TIMEOUT_SECS))
        .user_agent("ZoneManager/1.0")
        .build()
        .expect("Failed to build HTTP client")
});

/// Error type for provider operations
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Failed to parse response: {0}")]
    ParseError(String),
    #[error("Provider unavailable: {0}")]
    Unavailable(String),
}

impl IntoResponse for ProviderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            ProviderError::HttpError(e) => {
                tracing::error!("HTTP error: {}", e);
                (StatusCode::BAD_GATEWAY, format!("Failed to connect: {}", e))
            }
            ProviderError::ParseError(e) => {
                tracing::error!("Parse error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to parse response: {}", e),
                )
            }
            ProviderError::Unavailable(e) => {
                tracing::error!("Provider unavailable: {}", e);
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Provider unavailable: {}", e),
                )
            }
        };

        (status, Json(ErrorResponse::new(message))).into_response()
    }
}

/// Trait for model providers
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Get the provider name
    fn name(&self) -> &'static str;

    /// Search for models with pagination
    ///
    /// # Arguments
    /// * `query` - Optional search query
    /// * `cursor` - Optional pagination cursor (provider-specific format)
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// A BrowseResponse containing models and optional next_cursor
    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BrowseResponse, ProviderError>;
}

/// Get a provider by name
pub fn get_provider(name: &str) -> Result<Box<dyn ModelProvider>, ProviderError> {
    match name {
        "ollama" => Ok(Box::new(OllamaLibraryProvider)),
        "huggingface" => Ok(Box::new(HuggingFaceProvider)),
        "gpt4all" => Ok(Box::new(Gpt4AllProvider)),
        "openrouter" => Ok(Box::new(OpenRouterProvider)),
        _ => Err(ProviderError::Unavailable(format!(
            "Unknown provider: {}",
            name
        ))),
    }
}

// =============================================================================
// Ollama Library Provider
// =============================================================================

pub struct OllamaLibraryProvider;

#[async_trait]
impl ModelProvider for OllamaLibraryProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BrowseResponse, ProviderError> {
        // Parse offset from cursor
        let offset = parse_cursor_offset(cursor)?;

        let search_query = query.unwrap_or_default();
        let url = if search_query.is_empty() {
            "https://ollama.com/search".to_string()
        } else {
            format!(
                "https://ollama.com/search?q={}",
                urlencoding::encode(search_query)
            )
        };

        let response = HTTP_CLIENT.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(ProviderError::Unavailable(format!(
                "Ollama library returned status: {}",
                response.status()
            )));
        }

        let html = response.text().await?;
        let all_models = parse_ollama_library_html(&html);
        let total = all_models.len();
        let models: Vec<_> = all_models.into_iter().skip(offset).take(limit).collect();
        let models = attach_ollama_download_sizes(models).await;

        // For offset-based pagination, encode next offset as cursor
        let next_offset = offset + models.len();
        let next_cursor = if next_offset < total {
            Some(format!("offset:{}", next_offset))
        } else {
            None
        };

        Ok(BrowseResponse {
            models,
            next_cursor,
        })
    }
}

/// Look up the download size of each model from the Ollama registry.
///
/// A model whose manifest cannot be fetched keeps `size: None` rather than
/// failing the whole listing.
async fn attach_ollama_download_sizes(models: Vec<ModelResponse>) -> Vec<ModelResponse> {
    use futures::stream::{self, StreamExt};

    stream::iter(models)
        .map(|mut model| async move {
            model.size = fetch_ollama_manifest_size(&model.name).await;
            model
        })
        .buffered(OLLAMA_SIZE_LOOKUP_CONCURRENCY)
        .collect()
        .await
}

/// Sum the layer sizes in a model's `latest` manifest to get its download size.
async fn fetch_ollama_manifest_size(name: &str) -> Option<u64> {
    let url = format!("{}/{}/manifests/latest", OLLAMA_REGISTRY_URL, name);

    let response = HTTP_CLIENT
        .get(&url)
        .header("Accept", "application/vnd.docker.distribution.manifest.v2+json")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let manifest: serde_json::Value = response.json().await.ok()?;
    let total: u64 = manifest
        .get("layers")?
        .as_array()?
        .iter()
        .filter_map(|layer| layer.get("size")?.as_u64())
        .sum();

    (total > 0).then_some(total)
}

/// Parse Ollama library HTML to extract model information
fn parse_ollama_library_html(html: &str) -> Vec<ModelResponse> {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);
    let mut models = Vec::new();

    // Ollama uses <a> elements with href="/library/modelname" for model cards
    let card_selector =
        Selector::parse("a[href^='/library/']").expect("Static selector should always parse");

    for element in document.select(&card_selector) {
        if let Some(href) = element.value().attr("href") {
            // Extract model name from href like "/library/llama3.2"
            let name = href.strip_prefix("/library/").unwrap_or(href).to_string();

            if name.is_empty() || name.contains('/') {
                continue;
            }

            // Try to extract additional info from the card text
            let text = element.text().collect::<Vec<_>>().join(" ");
            let param_size = extract_param_size(&text);
            let family = extract_model_family(&name);

            models.push(ModelResponse {
                name,
                size: None,
                digest: None,
                modified_at: None,
                details: Some(ModelDetails {
                    format: Some("gguf".to_string()),
                    family,
                    parameter_size: param_size,
                    quantization_level: None,
                }),
            });
        }
    }

    // Deduplicate by name
    models.sort_by(|a, b| a.name.cmp(&b.name));
    models.dedup_by(|a, b| a.name == b.name);

    // If parsing failed, return popular models as fallback
    if models.is_empty() {
        return get_popular_ollama_models();
    }

    models
}

/// Extract model family from name
fn extract_model_family(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();
    let families = [
        ("llama", "llama"),
        ("mistral", "mistral"),
        ("qwen", "qwen"),
        ("phi", "phi"),
        ("gemma", "gemma"),
        ("deepseek", "deepseek"),
        ("codellama", "codellama"),
        ("vicuna", "vicuna"),
        ("falcon", "falcon"),
        ("yi", "yi"),
        ("command", "command"),
        ("mixtral", "mixtral"),
        ("nomic", "nomic"),
        ("mxbai", "mxbai"),
        ("snowflake", "snowflake"),
        ("starcoder", "starcoder"),
        ("codegemma", "codegemma"),
        ("granite", "granite"),
        ("smollm", "smollm"),
    ];

    for (pattern, family) in families {
        if name_lower.contains(pattern) {
            return Some(family.to_string());
        }
    }
    None
}

/// Return a list of popular Ollama models as fallback
fn get_popular_ollama_models() -> Vec<ModelResponse> {
    let popular = [
        ("llama3.2", "llama", "3B"),
        ("llama3.1", "llama", "8B"),
        ("llama3.1:70b", "llama", "70B"),
        ("mistral", "mistral", "7B"),
        ("mixtral", "mixtral", "47B"),
        ("qwen2.5", "qwen", "7B"),
        ("qwen2.5:72b", "qwen", "72B"),
        ("phi3", "phi", "3.8B"),
        ("gemma2", "gemma", "9B"),
        ("deepseek-r1", "deepseek", "7B"),
        ("deepseek-r1:32b", "deepseek", "32B"),
        ("codellama", "codellama", "7B"),
        ("starcoder2", "starcoder", "7B"),
        ("nomic-embed-text", "nomic", "137M"),
        ("mxbai-embed-large", "mxbai", "335M"),
        ("command-r", "command", "35B"),
        ("yi", "yi", "34B"),
        ("granite3-dense", "granite", "8B"),
        ("smollm2", "smollm", "1.7B"),
        ("dolphin-mixtral", "mixtral", "47B"),
    ];

    popular
        .iter()
        .map(|(name, family, size)| ModelResponse {
            name: name.to_string(),
            size: None,
            digest: None,
            modified_at: None,
            details: Some(ModelDetails {
                format: Some("gguf".to_string()),
                family: Some(family.to_string()),
                parameter_size: Some(size.to_string()),
                quantization_level: None,
            }),
        })
        .collect()
}

// =============================================================================
// HuggingFace Provider
// =============================================================================

pub struct HuggingFaceProvider;

#[async_trait]
impl ModelProvider for HuggingFaceProvider {
    fn name(&self) -> &'static str {
        "huggingface"
    }

    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BrowseResponse, ProviderError> {
        // Build URL with cursor-based pagination
        let mut url = format!(
            "https://huggingface.co/api/models?filter=gguf&sort=downloads&direction=-1&limit={}",
            limit
        );

        // Add cursor if provided (for pagination)
        // Note: cursor from Link header is already URL-encoded, don't re-encode
        if let Some(c) = cursor {
            url.push_str(&format!("&cursor={}", c));
        }

        if let Some(q) = query
            && !q.is_empty()
        {
            url.push_str(&format!("&search={}", urlencoding::encode(q)));
        }

        let response = HTTP_CLIENT.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(ProviderError::Unavailable(format!(
                "HuggingFace API returned status: {}",
                response.status()
            )));
        }

        // Extract next cursor from Link header
        let next_cursor = extract_cursor_from_link_header(response.headers());

        // Read body as text first for better error messages
        let body = response.text().await?;
        let hf_models: Vec<HuggingFaceModel> = serde_json::from_str(&body).map_err(|e| {
            tracing::error!(
                "HuggingFace JSON parse error: {}. Body preview: {}",
                e,
                &body[..body.len().min(500)]
            );
            ProviderError::ParseError(format!("{}", e))
        })?;

        let models: Vec<ModelResponse> = hf_models
            .into_iter()
            .map(|m| {
                // Try to extract model info from tags
                let family = m.tags.as_ref().and_then(|tags| {
                    // Look for common model families in tags
                    let families = [
                        "llama",
                        "mistral",
                        "qwen",
                        "phi",
                        "gemma",
                        "falcon",
                        "mpt",
                        "yi",
                        "deepseek",
                        "codellama",
                        "vicuna",
                        "orca",
                    ];
                    tags.iter()
                        .find(|t| families.iter().any(|f| t.to_lowercase().contains(f)))
                        .cloned()
                });

                // Try to extract parameter size from model ID or tags
                let param_size = extract_param_size(&m.model_id).or_else(|| {
                    m.tags.as_ref().and_then(|tags| {
                        tags.iter()
                            .find(|t| t.ends_with('b') || t.ends_with('B') || t.contains("param"))
                            .cloned()
                    })
                });

                ModelResponse {
                    name: m.model_id,
                    size: None,
                    digest: m.sha.clone(),
                    modified_at: m.last_modified,
                    details: Some(ModelDetails {
                        format: Some("gguf".to_string()),
                        family,
                        parameter_size: param_size,
                        quantization_level: None,
                    }),
                }
            })
            .collect();

        Ok(BrowseResponse {
            models,
            next_cursor,
        })
    }
}

/// Extract cursor from Link header
/// Format: <https://huggingface.co/api/models?cursor=xyz123>; rel="next"
fn extract_cursor_from_link_header(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let link = headers.get("link")?.to_str().ok()?;

    // Parse Link header to find rel="next"
    for part in link.split(',') {
        let part = part.trim();
        if part.contains("rel=\"next\"") || part.contains("rel='next'") {
            // Extract URL between < and >
            if let Some(start) = part.find('<')
                && let Some(end) = part.find('>')
            {
                let url = &part[start + 1..end];
                // Extract cursor parameter from URL
                if let Some(cursor_start) = url.find("cursor=") {
                    let cursor_part = &url[cursor_start + 7..];
                    // Cursor ends at & or end of string
                    let cursor = cursor_part.split('&').next().unwrap_or(cursor_part);
                    return Some(cursor.to_string());
                }
            }
        }
    }

    None
}

#[derive(Debug, serde::Deserialize)]
struct HuggingFaceModel {
    #[serde(rename = "modelId")]
    model_id: String,
    #[serde(default)]
    sha: Option<String>,
    #[serde(rename = "lastModified", default)]
    last_modified: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    likes: Option<u64>,
}

// =============================================================================
// GPT4All Provider
// =============================================================================

pub struct Gpt4AllProvider;

const GPT4ALL_MODELS_URL: &str =
    "https://raw.githubusercontent.com/nomic-ai/gpt4all/main/gpt4all-chat/metadata/models3.json";

#[async_trait]
impl ModelProvider for Gpt4AllProvider {
    fn name(&self) -> &'static str {
        "gpt4all"
    }

    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BrowseResponse, ProviderError> {
        // GPT4All uses a static JSON catalog, so we fetch all and paginate client-side
        let offset = parse_cursor_offset(cursor)?;

        let response = HTTP_CLIENT.get(GPT4ALL_MODELS_URL).send().await?;

        if !response.status().is_success() {
            return Err(ProviderError::Unavailable(format!(
                "GPT4All API returned status: {}",
                response.status()
            )));
        }

        let body = response.text().await?;
        let gpt4all_models: Vec<Gpt4AllModel> = serde_json::from_str(&body).map_err(|e| {
            tracing::error!(
                "GPT4All JSON parse error: {}. Body preview: {}",
                e,
                &body[..body.len().min(500)]
            );
            ProviderError::ParseError(format!("{}", e))
        })?;

        // Filter by query if provided
        let filtered: Vec<_> = if let Some(q) = query {
            let q_lower = q.to_lowercase();
            gpt4all_models
                .into_iter()
                .filter(|m| {
                    m.name.to_lowercase().contains(&q_lower)
                        || m.filename.to_lowercase().contains(&q_lower)
                        || m.model_type
                            .as_ref()
                            .map(|t| t.to_lowercase().contains(&q_lower))
                            .unwrap_or(false)
                })
                .collect()
        } else {
            gpt4all_models
        };

        let total = filtered.len();
        let models: Vec<ModelResponse> = filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|m| {
                let param_size = m
                    .parameters
                    .clone()
                    .or_else(|| extract_param_size(&m.filename));

                ModelResponse {
                    name: m.name.clone(),
                    size: Some(m.filesize),
                    digest: None,
                    modified_at: None,
                    details: Some(ModelDetails {
                        format: Some("gguf".to_string()),
                        family: m.model_type.clone(),
                        parameter_size: param_size,
                        quantization_level: extract_quantization(&m.filename),
                    }),
                }
            })
            .collect();

        let next_offset = offset + models.len();
        let next_cursor = if next_offset < total {
            Some(format!("offset:{}", next_offset))
        } else {
            None
        };

        Ok(BrowseResponse {
            models,
            next_cursor,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct Gpt4AllModel {
    name: String,
    filename: String,
    #[serde(deserialize_with = "deserialize_string_or_number")]
    filesize: u64,
    #[serde(default)]
    parameters: Option<String>,
    #[serde(rename = "type", default)]
    model_type: Option<String>,
}

/// Deserialize a value that can be either a string or a number into u64
fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct StringOrNumber;

    impl<'de> Visitor<'de> for StringOrNumber {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number")
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v as u64)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.parse().map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_any(StringOrNumber)
}

/// Extract quantization level from filename (e.g., "Q4_0", "Q5_K_M")
fn extract_quantization(filename: &str) -> Option<String> {
    let patterns = [
        "Q8_0", "Q6_K", "Q5_K_M", "Q5_K_S", "Q5_1", "Q5_0", "Q4_K_M", "Q4_K_S", "Q4_1", "Q4_0",
        "Q3_K_M", "Q3_K_S", "Q2_K", "IQ4_XS", "IQ3_M", "IQ2_S",
    ];

    let filename_upper = filename.to_uppercase();
    for pattern in patterns {
        if filename_upper.contains(pattern) {
            return Some(pattern.to_string());
        }
    }
    None
}

// =============================================================================
// OpenRouter Provider
// =============================================================================

pub struct OpenRouterProvider;

const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    async fn search(
        &self,
        query: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<BrowseResponse, ProviderError> {
        let offset = parse_cursor_offset(cursor)?;

        let response = HTTP_CLIENT.get(OPENROUTER_MODELS_URL).send().await?;

        if !response.status().is_success() {
            return Err(ProviderError::Unavailable(format!(
                "OpenRouter API returned status: {}",
                response.status()
            )));
        }

        let body = response.text().await?;
        let or_response: OpenRouterResponse = serde_json::from_str(&body).map_err(|e| {
            tracing::error!(
                "OpenRouter JSON parse error: {}. Body preview: {}",
                e,
                &body[..body.len().min(500)]
            );
            ProviderError::ParseError(format!("{}", e))
        })?;

        // Filter by query if provided
        let filtered: Vec<_> = if let Some(q) = query {
            let q_lower = q.to_lowercase();
            or_response
                .data
                .into_iter()
                .filter(|m| {
                    m.id.to_lowercase().contains(&q_lower)
                        || m.name.to_lowercase().contains(&q_lower)
                })
                .collect()
        } else {
            or_response.data
        };

        let total = filtered.len();
        let models: Vec<ModelResponse> = filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|m| {
                let param_size = extract_param_size(&m.id);

                ModelResponse {
                    name: m.id,
                    size: None,
                    digest: None,
                    modified_at: None,
                    details: Some(ModelDetails {
                        format: Some("api".to_string()),
                        family: m.architecture.and_then(|a| a.tokenizer),
                        parameter_size: param_size,
                        quantization_level: None,
                    }),
                }
            })
            .collect();

        let next_offset = offset + models.len();
        let next_cursor = if next_offset < total {
            Some(format!("offset:{}", next_offset))
        } else {
            None
        };

        Ok(BrowseResponse {
            models,
            next_cursor,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterModel {
    id: String,
    name: String,
    #[serde(default)]
    architecture: Option<OpenRouterArchitecture>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    tokenizer: Option<String>,
}

// =============================================================================
// Utility functions
// =============================================================================

/// Parse cursor to extract offset for providers using offset-based pagination
fn parse_cursor_offset(cursor: Option<&str>) -> Result<usize, ProviderError> {
    let cursor = match cursor {
        None => return Ok(0),
        Some(c) => c,
    };

    if let Some(offset_str) = cursor.strip_prefix("offset:") {
        return offset_str
            .parse()
            .map_err(|_| ProviderError::ParseError("Invalid cursor offset format".into()));
    }

    if let Some(page_str) = cursor.strip_prefix("page:") {
        let page: usize = page_str
            .parse()
            .map_err(|_| ProviderError::ParseError("Invalid cursor page format".into()))?;
        // Return page-1 * DEFAULT_PAGE_SIZE as approximate offset
        return Ok(page.saturating_sub(1) * DEFAULT_PAGE_SIZE);
    }

    // Unknown cursor format
    Err(ProviderError::ParseError(format!(
        "Unknown cursor format: {}",
        cursor
    )))
}

/// Extract parameter size from model name (e.g., "7b", "13B", "70B")
fn extract_param_size(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();

    // Common patterns: 7b, 13b, 70b, 1.5b, 0.5b, etc.
    // Sorted by length (longest first) to match longer patterns first
    let patterns = [
        "180b", "72b", "70b", "65b", "34b", "33b", "32b", "30b", "14b", "13b", "8b", "7b", "4b",
        "3b", "2b", "1.5b", "1b", "0.5b",
    ];

    for pattern in patterns {
        if name_lower.contains(pattern) {
            return Some(pattern.to_uppercase());
        }
    }

    None
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test the trait interface
    #[tokio::test]
    async fn test_provider_trait_interface() {
        let provider: Box<dyn ModelProvider> = Box::new(OllamaLibraryProvider);
        assert_eq!(provider.name(), "ollama");
    }

    #[tokio::test]
    async fn test_get_provider() {
        let ollama = get_provider("ollama");
        assert!(ollama.is_ok());
        assert_eq!(ollama.unwrap().name(), "ollama");

        let hf = get_provider("huggingface");
        assert!(hf.is_ok());
        assert_eq!(hf.unwrap().name(), "huggingface");

        let gpt4all = get_provider("gpt4all");
        assert!(gpt4all.is_ok());
        assert_eq!(gpt4all.unwrap().name(), "gpt4all");

        let openrouter = get_provider("openrouter");
        assert!(openrouter.is_ok());
        assert_eq!(openrouter.unwrap().name(), "openrouter");

        let unknown = get_provider("unknown");
        assert!(unknown.is_err());
    }

    // Test utility functions
    #[test]
    fn test_parse_cursor_offset() {
        assert_eq!(parse_cursor_offset(None).unwrap(), 0);
        assert_eq!(parse_cursor_offset(Some("offset:20")).unwrap(), 20);
        assert_eq!(parse_cursor_offset(Some("offset:100")).unwrap(), 100);
        assert_eq!(parse_cursor_offset(Some("page:1")).unwrap(), 0);
        assert_eq!(parse_cursor_offset(Some("page:2")).unwrap(), 20);
        assert_eq!(parse_cursor_offset(Some("page:3")).unwrap(), 40);
        // Invalid cursor should return error
        assert!(parse_cursor_offset(Some("invalid")).is_err());
        assert!(parse_cursor_offset(Some("offset:abc")).is_err());
        assert!(parse_cursor_offset(Some("page:xyz")).is_err());
    }

    #[test]
    fn test_extract_param_size() {
        assert_eq!(extract_param_size("llama-7b"), Some("7B".to_string()));
        assert_eq!(
            extract_param_size("mistral-13B-v2"),
            Some("13B".to_string())
        );
        assert_eq!(extract_param_size("qwen-72b-chat"), Some("72B".to_string()));
        assert_eq!(extract_param_size("phi-3b"), Some("3B".to_string()));
        assert_eq!(extract_param_size("model-without-size"), None);
    }

    #[test]
    fn test_extract_quantization() {
        assert_eq!(
            extract_quantization("model-Q4_0.gguf"),
            Some("Q4_0".to_string())
        );
        assert_eq!(
            extract_quantization("llama-7b-Q5_K_M.gguf"),
            Some("Q5_K_M".to_string())
        );
        assert_eq!(
            extract_quantization("model-Q8_0.gguf"),
            Some("Q8_0".to_string())
        );
        assert_eq!(
            extract_quantization("model-IQ4_XS.gguf"),
            Some("IQ4_XS".to_string())
        );
        assert_eq!(extract_quantization("model.gguf"), None);
    }

    #[test]
    fn test_extract_model_family() {
        assert_eq!(extract_model_family("llama3.2"), Some("llama".to_string()));
        assert_eq!(
            extract_model_family("mistral-7b"),
            Some("mistral".to_string())
        );
        assert_eq!(extract_model_family("qwen2.5"), Some("qwen".to_string()));
        assert_eq!(extract_model_family("phi3"), Some("phi".to_string()));
        assert_eq!(
            extract_model_family("deepseek-r1"),
            Some("deepseek".to_string())
        );
        assert_eq!(extract_model_family("unknown-model"), None);
    }

    #[test]
    fn test_extract_cursor_from_link_header() {
        use reqwest::header::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            HeaderValue::from_static(
                r#"<https://huggingface.co/api/models?cursor=abc123>; rel="next""#,
            ),
        );

        let cursor = extract_cursor_from_link_header(&headers);
        assert_eq!(cursor, Some("abc123".to_string()));

        // Test with additional query params
        let mut headers2 = HeaderMap::new();
        headers2.insert(
            "link",
            HeaderValue::from_static(
                r#"<https://huggingface.co/api/models?cursor=xyz789&limit=20>; rel="next""#,
            ),
        );

        let cursor2 = extract_cursor_from_link_header(&headers2);
        assert_eq!(cursor2, Some("xyz789".to_string()));

        // Test with no link header
        let headers3 = HeaderMap::new();
        let cursor3 = extract_cursor_from_link_header(&headers3);
        assert_eq!(cursor3, None);
    }

    #[test]
    fn test_get_popular_ollama_models() {
        let models = get_popular_ollama_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.name == "llama3.2"));
        assert!(models.iter().any(|m| m.name == "mistral"));
        assert!(models.iter().any(|m| m.name == "qwen2.5"));

        // Verify all models have proper details
        for model in &models {
            assert!(model.details.is_some());
            let details = model.details.as_ref().unwrap();
            assert_eq!(details.format, Some("gguf".to_string()));
            assert!(details.family.is_some());
            assert!(details.parameter_size.is_some());
        }
    }

    #[test]
    fn test_parse_ollama_library_html_with_valid_html() {
        let html = r#"
            <html>
                <body>
                    <a href="/library/llama3.2">Llama 3.2 3B</a>
                    <a href="/library/mistral">Mistral 7B</a>
                    <a href="/library/qwen2.5">Qwen 2.5 7B</a>
                </body>
            </html>
        "#;

        let models = parse_ollama_library_html(html);
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].name, "llama3.2");
        assert_eq!(models[1].name, "mistral");
        assert_eq!(models[2].name, "qwen2.5");

        // Check that details are populated
        for model in &models {
            assert!(model.details.is_some());
            let details = model.details.as_ref().unwrap();
            assert_eq!(details.format, Some("gguf".to_string()));
        }
    }

    #[test]
    fn test_parse_ollama_library_html_with_empty_html() {
        let html = "<html><body></body></html>";
        let models = parse_ollama_library_html(html);
        // Should return popular models as fallback
        assert!(!models.is_empty());
    }

    #[test]
    fn test_parse_ollama_library_html_deduplication() {
        let html = r#"
            <html>
                <body>
                    <a href="/library/llama3.2">Llama 3.2</a>
                    <a href="/library/llama3.2">Llama 3.2 Duplicate</a>
                    <a href="/library/mistral">Mistral</a>
                </body>
            </html>
        "#;

        let models = parse_ollama_library_html(html);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "llama3.2");
        assert_eq!(models[1].name, "mistral");
    }

    // Test error handling
    #[test]
    fn test_provider_error_display() {
        let error = ProviderError::ParseError("Invalid JSON".to_string());
        assert_eq!(error.to_string(), "Failed to parse response: Invalid JSON");

        let error2 = ProviderError::Unavailable("Service down".to_string());
        assert_eq!(error2.to_string(), "Provider unavailable: Service down");
    }

    // Test JSON parsing with real API response formats
    #[test]
    fn test_huggingface_json_parsing() {
        // Real HuggingFace API response includes both id and modelId fields
        let json = r#"[
            {"_id":"123","id":"author/model","modelId":"author/model","likes":100,"downloads":1000,"tags":["gguf"]},
            {"_id":"456","id":"other/model2","modelId":"other/model2","likes":50,"downloads":500}
        ]"#;

        // We use serde flatten or ignore unknown fields to handle the extra `id` field
        let models: Vec<HuggingFaceModel> = serde_json::from_str(json).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_id, "author/model");
        assert_eq!(models[1].model_id, "other/model2");
    }

    #[test]
    fn test_gpt4all_json_parsing() {
        // GPT4All returns filesize as a string
        let json = r#"[
            {"name":"Test Model","filename":"test-q4_0.gguf","filesize":"4431390720","parameters":"7 billion","type":"llama"},
            {"name":"Model 2","filename":"model2.gguf","filesize":"1234567890"}
        ]"#;

        let models: Vec<Gpt4AllModel> = serde_json::from_str(json).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Test Model");
        assert_eq!(models[0].filesize, 4431390720);
        assert_eq!(models[0].parameters, Some("7 billion".to_string()));
        assert_eq!(models[1].filesize, 1234567890);
    }

    #[test]
    fn test_gpt4all_json_parsing_numeric_filesize() {
        // Also handle numeric filesize just in case
        let json = r#"[{"name":"Test","filename":"test.gguf","filesize":12345}]"#;
        let models: Vec<Gpt4AllModel> = serde_json::from_str(json).unwrap();
        assert_eq!(models[0].filesize, 12345);
    }

    #[test]
    fn test_openrouter_json_parsing() {
        let json = r#"{
            "data": [
                {"id":"openai/gpt-4","name":"GPT-4","architecture":{"tokenizer":"GPT"}},
                {"id":"anthropic/claude-3","name":"Claude 3"}
            ]
        }"#;

        let response: OpenRouterResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].id, "openai/gpt-4");
        assert_eq!(response.data[0].name, "GPT-4");
        assert_eq!(
            response.data[0].architecture.as_ref().unwrap().tokenizer,
            Some("GPT".to_string())
        );
        assert_eq!(response.data[1].id, "anthropic/claude-3");
        assert!(response.data[1].architecture.is_none());
    }
}
