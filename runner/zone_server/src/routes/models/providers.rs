//! Model provider trait and implementations

use async_trait::async_trait;
use axum::{Json, http::StatusCode, response::IntoResponse};
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use std::time::Duration;

use super::types::{
    BrowseQuery, BrowseResponse, ErrorResponse, ModelDetails, ModelResponse, ModelSize,
    ModelSizeFilter, ModelSort,
};

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

static PARAM_SIZE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:^|[^A-Za-z0-9])(\d+(?:\.\d+)?)\s*[bB]\b").expect("param size regex")
});
static PARAM_SIZE_CHIP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^\d+(?:\.\d+)?[bB]$").expect("param size chip regex"));
static BILLION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*billion").expect("billion regex"));
static PULLS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)([\d,.]+[KMBkmb]?)\s*Pulls").expect("pulls regex"));

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

    /// Search for models with pagination, sorting, and filtering
    async fn search(&self, opts: BrowseQuery<'_>) -> Result<BrowseResponse, ProviderError>;
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

    async fn search(&self, opts: BrowseQuery<'_>) -> Result<BrowseResponse, ProviderError> {
        let offset = parse_cursor_offset(opts.cursor)?;

        let search_query = opts.query.unwrap_or_default();
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
        let mut models = parse_ollama_library_html(&html);
        if matches!(opts.sort, ModelSort::SizeAsc | ModelSort::SizeDesc) {
            models = attach_ollama_download_sizes(models).await;
            models = refine_models(models, &opts);
            return Ok(paginate_models(models, offset, opts.limit));
        }

        models = refine_models(models, &opts);
        let mut page = paginate_models(models, offset, opts.limit);
        page.models = attach_ollama_download_sizes(page.models).await;
        Ok(page)
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
            if let Some(sizes) = model.sizes.as_mut() {
                for variant in sizes.iter_mut() {
                    variant.size = fetch_ollama_manifest_size(&variant.name).await;
                }
            }
            model
        })
        .buffered(OLLAMA_SIZE_LOOKUP_CONCURRENCY)
        .collect()
        .await
}

/// Split `llama3.2:1b` into repository + tag. Untagged names use `latest`.
fn ollama_manifest_ref(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some((repo, tag)) if !repo.is_empty() && !tag.is_empty() => (repo, tag),
        _ => (name, "latest"),
    }
}

/// Sum the layer sizes in a model's manifest to get its download size.
async fn fetch_ollama_manifest_size(name: &str) -> Option<u64> {
    let (repo, tag) = ollama_manifest_ref(name);
    let url = format!("{}/{}/manifests/{}", OLLAMA_REGISTRY_URL, repo, tag);

    let response = HTTP_CLIENT
        .get(&url)
        .header(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json",
        )
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
    let document = Html::parse_document(html);
    let mut models = Vec::new();

    // Ollama uses <a> elements with href="/library/modelname" for model cards
    let card_selector =
        Selector::parse("a[href^='/library/']").expect("Static selector should always parse");
    let paragraph_selector = Selector::parse("p").expect("Static selector should always parse");
    let span_selector = Selector::parse("span").expect("Static selector should always parse");

    for element in document.select(&card_selector) {
        if let Some(href) = element.value().attr("href") {
            // Extract model name from href like "/library/llama3.2"
            let name = href.strip_prefix("/library/").unwrap_or(href).to_string();

            if name.is_empty() || name.contains('/') {
                continue;
            }

            let text = collapse_whitespace(&element.text().collect::<Vec<_>>().join(" "));
            let description = element
                .select(&paragraph_selector)
                .map(|p| collapse_whitespace(&p.text().collect::<String>()))
                .find(|t| !t.is_empty() && !is_ollama_stat_line(t));
            let capability_tags = element
                .select(&span_selector)
                .map(|s| collapse_whitespace(&s.text().collect::<String>()))
                .filter(|t| is_capability_tag(t))
                .collect::<Vec<_>>();
            let chip_sizes = element
                .select(&span_selector)
                .map(|s| collapse_whitespace(&s.text().collect::<String>()))
                .filter(|t| is_param_size_chip(t))
                .collect::<Vec<_>>();

            let size_labels = collect_param_size_labels(&text, chip_sizes);
            let param_size = format_param_sizes(size_labels.clone())
                .or_else(|| description.as_deref().and_then(extract_param_size));
            let sizes = ollama_size_variants(&name, &size_labels);
            let family = extract_model_family(&name);
            let use_cases = nonempty_vec(infer_use_cases(&[
                description.as_deref().unwrap_or(""),
                &capability_tags.join(" "),
                &name,
            ]));
            let tags = nonempty_vec(capability_tags);
            let downloads = extract_pulls(&text);
            let url = Some(format!("https://ollama.com/library/{}", name));

            models.push(ModelResponse {
                name,
                description,
                url,
                downloads,
                tags,
                use_cases,
                sizes,
                details: Some(ModelDetails {
                    format: Some("gguf".to_string()),
                    family,
                    parameter_size: param_size,
                    ..Default::default()
                }),
                ..Default::default()
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
    // Longer / more specific names first so "codellama" is not classified as "llama".
    let families = [
        ("codellama", "codellama"),
        ("codegemma", "codegemma"),
        ("starcoder", "starcoder"),
        ("deepseek", "deepseek"),
        ("nemotron", "nemotron"),
        ("snowflake", "snowflake"),
        ("minimax", "minimax"),
        ("mixtral", "mixtral"),
        ("mistral", "mistral"),
        ("command", "command"),
        ("granite", "granite"),
        ("smollm", "smollm"),
        ("ornith", "ornith"),
        ("llama", "llama"),
        ("qwen", "qwen"),
        ("gemma", "gemma"),
        ("vicuna", "vicuna"),
        ("falcon", "falcon"),
        ("nomic", "nomic"),
        ("mxbai", "mxbai"),
        ("muse", "muse"),
        ("kimi", "kimi"),
        ("phi", "phi"),
        ("glm", "glm"),
        ("yi", "yi"),
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
        (
            "llama3.2",
            "llama",
            "3B",
            "Meta's compact Llama 3.2 for multilingual chat and on-device assistants.",
            &["Chat", "Agents"] as &[&str],
        ),
        (
            "llama3.1",
            "llama",
            "8B",
            "Meta Llama 3.1 8B — a strong general-purpose local chat and instruction model.",
            &["Chat", "Tool use"],
        ),
        (
            "llama3.1:70b",
            "llama",
            "70B",
            "Meta Llama 3.1 70B for higher-quality reasoning, writing, and complex assistants.",
            &["Chat", "Reasoning"],
        ),
        (
            "mistral",
            "mistral",
            "7B",
            "Mistral 7B — fast, capable instruction following for chat and lightweight agents.",
            &["Chat"],
        ),
        (
            "mixtral",
            "mixtral",
            "47B",
            "Mixtral 8x7B mixture-of-experts for high-quality chat at a modest active-parameter cost.",
            &["Chat", "Reasoning"],
        ),
        (
            "qwen2.5",
            "qwen",
            "7B",
            "Qwen2.5 7B — strong multilingual chat, coding, and instruction following.",
            &["Chat", "Coding"],
        ),
        (
            "qwen2.5:72b",
            "qwen",
            "72B",
            "Qwen2.5 72B for demanding reasoning, coding, and long-form generation.",
            &["Chat", "Coding", "Reasoning"],
        ),
        (
            "phi3",
            "phi",
            "3.8B",
            "Microsoft Phi-3 Mini — small, efficient model for constrained hardware.",
            &["Chat"],
        ),
        (
            "gemma2",
            "gemma",
            "9B",
            "Google Gemma 2 9B for lightweight chat and instruction workloads.",
            &["Chat"],
        ),
        (
            "deepseek-r1",
            "deepseek",
            "7B",
            "DeepSeek-R1 distill focused on step-by-step reasoning and math.",
            &["Reasoning"],
        ),
        (
            "deepseek-r1:32b",
            "deepseek",
            "32B",
            "Larger DeepSeek-R1 distill for harder reasoning and analysis tasks.",
            &["Reasoning"],
        ),
        (
            "codellama",
            "codellama",
            "7B",
            "Code Llama 7B for code completion, explanation, and light refactoring.",
            &["Coding"],
        ),
        (
            "starcoder2",
            "starcoder",
            "7B",
            "StarCoder2 7B trained on permissively licensed code for completion and generation.",
            &["Coding"],
        ),
        (
            "nomic-embed-text",
            "nomic",
            "137M",
            "Nomic Embed Text — compact embedding model for search and retrieval.",
            &["Embeddings"],
        ),
        (
            "mxbai-embed-large",
            "mxbai",
            "335M",
            "mixedbread embed-large for higher-quality retrieval embeddings.",
            &["Embeddings"],
        ),
        (
            "command-r",
            "command",
            "35B",
            "Cohere Command R for retrieval-augmented chat and tool-using agents.",
            &["Chat", "Tool use", "Agents"],
        ),
        (
            "yi",
            "yi",
            "34B",
            "01.AI Yi 34B bilingual chat model for English and Chinese workloads.",
            &["Chat"],
        ),
        (
            "granite3-dense",
            "granite",
            "8B",
            "IBM Granite 3 Dense 8B for enterprise chat and instruction following.",
            &["Chat"],
        ),
        (
            "smollm2",
            "smollm",
            "1.7B",
            "SmolLM2 — very small on-device model for simple chat and experiments.",
            &["Chat"],
        ),
        (
            "dolphin-mixtral",
            "mixtral",
            "47B",
            "Uncensored Dolphin fine-tune of Mixtral for creative chat and roleplay.",
            &["Chat"],
        ),
    ];

    popular
        .iter()
        .map(
            |(name, family, size, description, use_cases)| ModelResponse {
                name: name.to_string(),
                description: Some(description.to_string()),
                url: Some(format!("https://ollama.com/library/{}", name)),
                use_cases: Some(use_cases.iter().map(|s| s.to_string()).collect()),
                details: Some(ModelDetails {
                    format: Some("gguf".to_string()),
                    family: Some(family.to_string()),
                    parameter_size: Some(size.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
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

    async fn search(&self, opts: BrowseQuery<'_>) -> Result<BrowseResponse, ProviderError> {
        if !huggingface_uses_local_window(&opts) {
            let (models, next_cursor) =
                fetch_huggingface_page(&opts, opts.cursor, opts.limit).await?;
            return Ok(BrowseResponse {
                models: refine_models(models, &opts),
                next_cursor,
            });
        }

        // Size filters and name/size/parameter sorts cannot be applied to a
        // single HuggingFace downloads page. Gather a window first, refine it,
        // then paginate with an offset cursor.
        let offset = parse_cursor_offset(opts.cursor).unwrap_or(0);
        let needs_sorted_window = huggingface_uses_local_sort(opts.sort);
        let max_pages = huggingface_window_pages(needs_sorted_window);
        let mut accumulated = Vec::new();
        let mut hf_cursor: Option<String> = None;
        let mut pages = 0;

        loop {
            let (page, next) =
                fetch_huggingface_page(&opts, hf_cursor.as_deref(), MAX_PAGE_SIZE).await?;
            pages += 1;
            accumulated.extend(page);
            hf_cursor = next;

            let exhausted = hf_cursor.is_none() || pages >= max_pages;
            if exhausted {
                break;
            }

            // Size-only refinement can stop once this window can fill the page.
            // Name/size/parameter sorts need the full window before ordering.
            if !needs_sorted_window
                && count_matching_models(&accumulated, &opts) >= offset.saturating_add(opts.limit)
            {
                break;
            }
        }

        let mut response = paginate_models(refine_models(accumulated, &opts), offset, opts.limit);
        response.next_cursor = huggingface_window_next_cursor(
            response.next_cursor,
            hf_cursor.is_some(),
            needs_sorted_window,
            pages >= max_pages,
            offset,
            response.models.len(),
        );
        Ok(response)
    }
}

/// Pages fetched when HuggingFace cannot apply the requested sort natively.
const HF_WINDOW_PAGES: usize = 5;
/// Extra scan budget for size filters, which can skip most downloads-ranked rows.
const HF_FILTER_MAX_PAGES: usize = 15;

fn huggingface_window_pages(needs_sorted_window: bool) -> usize {
    if needs_sorted_window {
        HF_WINDOW_PAGES
    } else {
        HF_FILTER_MAX_PAGES
    }
}

/// Keep pagination inside the fetched window for local sorts. Size filters may
/// continue only while we stopped early with more HuggingFace pages available.
fn huggingface_window_next_cursor(
    page_next: Option<String>,
    hf_has_more: bool,
    needs_sorted_window: bool,
    hit_page_cap: bool,
    offset: usize,
    page_len: usize,
) -> Option<String> {
    if page_next.is_some() {
        return page_next;
    }
    if hf_has_more && !needs_sorted_window && !hit_page_cap {
        return Some(format!("offset:{}", offset + page_len));
    }
    None
}

fn huggingface_uses_local_sort(sort: ModelSort) -> bool {
    matches!(
        sort,
        ModelSort::NameAsc
            | ModelSort::NameDesc
            | ModelSort::SizeAsc
            | ModelSort::SizeDesc
            | ModelSort::ParamsAsc
            | ModelSort::ParamsDesc
    )
}

fn huggingface_uses_local_window(opts: &BrowseQuery<'_>) -> bool {
    opts.size != ModelSizeFilter::All || huggingface_uses_local_sort(opts.sort)
}

async fn fetch_huggingface_page(
    opts: &BrowseQuery<'_>,
    cursor: Option<&str>,
    limit: usize,
) -> Result<(Vec<ModelResponse>, Option<String>), ProviderError> {
    let (sort_field, direction) = huggingface_sort_params(opts.sort);
    let mut url = format!(
        "https://huggingface.co/api/models?filter=gguf&sort={}&direction={}&limit={}",
        sort_field, direction, limit
    );
    for field in [
        "cardData",
        "gguf",
        "downloads",
        "likes",
        "tags",
        "pipeline_tag",
        "createdAt",
        "author",
        "lastModified",
    ] {
        url.push_str("&expand%5B%5D=");
        url.push_str(field);
    }

    if let Some(c) = cursor {
        url.push_str(&format!("&cursor={}", c));
    }

    if let Some(q) = opts.query
        && !q.is_empty()
    {
        url.push_str(&format!("&search={}", urlencoding::encode(q)));
    }

    if let Some(family) = opts.family {
        url.push_str(&format!("&filter={}", urlencoding::encode(family)));
    }

    let response = HTTP_CLIENT.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(ProviderError::Unavailable(format!(
            "HuggingFace API returned status: {}",
            response.status()
        )));
    }

    let next_cursor = extract_cursor_from_link_header(response.headers());
    let body = response.text().await?;
    let hf_models: Vec<HuggingFaceModel> = serde_json::from_str(&body).map_err(|e| {
        tracing::error!(
            "HuggingFace JSON parse error: {}. Body preview: {}",
            e,
            &body[..body.len().min(500)]
        );
        ProviderError::ParseError(format!("{}", e))
    })?;

    Ok((
        hf_models.into_iter().map(huggingface_to_model).collect(),
        next_cursor,
    ))
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
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "modelId", default)]
    model_id: Option<String>,
    #[serde(default)]
    sha: Option<String>,
    #[serde(rename = "lastModified", default)]
    last_modified: Option<String>,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    likes: Option<u64>,
    #[serde(default)]
    author: Option<String>,
    #[serde(rename = "pipeline_tag", default)]
    pipeline_tag: Option<String>,
    #[serde(rename = "cardData", default)]
    card_data: Option<HuggingFaceCardData>,
    #[serde(default)]
    gguf: Option<HuggingFaceGguf>,
}

#[derive(Debug, serde::Deserialize)]
struct HuggingFaceCardData {
    #[serde(default)]
    license: Option<String>,
    #[serde(rename = "pipeline_tag", default)]
    pipeline_tag: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct HuggingFaceGguf {
    #[serde(default)]
    total: Option<u64>,
    #[serde(rename = "totalFileSize", default)]
    total_file_size: Option<u64>,
    #[serde(default)]
    architecture: Option<String>,
    #[serde(rename = "context_length", default)]
    context_length: Option<u64>,
}

fn huggingface_model_id(m: &HuggingFaceModel) -> String {
    m.model_id
        .clone()
        .or_else(|| m.id.clone())
        .unwrap_or_default()
}

fn huggingface_to_model(m: HuggingFaceModel) -> ModelResponse {
    let model_id = huggingface_model_id(&m);
    let tags = m.tags.clone().unwrap_or_default();
    let pipeline = m
        .pipeline_tag
        .clone()
        .or_else(|| m.card_data.as_ref().and_then(|c| c.pipeline_tag.clone()));
    let family = m
        .gguf
        .as_ref()
        .and_then(|g| g.architecture.clone())
        .or_else(|| {
            tags.iter()
                .find_map(|t| extract_model_family(t))
                .or_else(|| extract_model_family(&model_id))
        });
    let param_size = extract_param_size(&model_id).or_else(|| {
        tags.iter().find_map(|t| {
            if is_param_size_chip(t) {
                Some(t.to_uppercase())
            } else {
                extract_param_size(t)
            }
        })
    });
    let size = m.gguf.as_ref().and_then(|g| g.total_file_size.or(g.total));
    let context_length = m.gguf.as_ref().and_then(|g| g.context_length);
    let license = m.card_data.as_ref().and_then(|c| c.license.clone());
    let author = m
        .author
        .clone()
        .or_else(|| model_id.split_once('/').map(|(owner, _)| owner.to_string()));
    let description = huggingface_description(&m, pipeline.as_deref(), family.as_deref());
    let use_cases = nonempty_vec(use_cases_from_pipeline(pipeline.as_deref(), &tags));
    let public_tags = nonempty_vec(
        tags.into_iter()
            .filter(|t| {
                let lower = t.to_lowercase();
                !lower.starts_with("arxiv:")
                    && !lower.starts_with("base_model:")
                    && !lower.starts_with("license:")
                    && !lower.starts_with("region:")
                    && lower != "endpoints_compatible"
                    && lower != "transformers"
            })
            .take(8)
            .collect(),
    );

    ModelResponse {
        name: model_id.clone(),
        size,
        digest: m.sha,
        modified_at: m.last_modified.or(m.created_at),
        description,
        author,
        url: Some(format!("https://huggingface.co/{}", model_id)),
        downloads: m.downloads,
        likes: m.likes,
        tags: public_tags,
        use_cases,
        details: Some(ModelDetails {
            format: Some("gguf".to_string()),
            family,
            parameter_size: param_size,
            context_length,
            license,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn huggingface_description(
    m: &HuggingFaceModel,
    pipeline: Option<&str>,
    family: Option<&str>,
) -> Option<String> {
    let mut sentence = match pipeline {
        Some(tag) => format!("{} GGUF model", humanize_label(tag)),
        None => "GGUF model".to_string(),
    };
    if let Some(author) = m.author.as_deref() {
        sentence.push_str(" by ");
        sentence.push_str(author);
    }
    if let Some(family) = family {
        sentence.push_str(". ");
        sentence.push_str(&humanize_label(family));
        sentence.push_str(" architecture");
    }
    if let Some(ctx) = m.gguf.as_ref().and_then(|g| g.context_length) {
        sentence.push_str(" with ");
        sentence.push_str(&format_context_tokens(ctx));
        sentence.push_str(" context");
    }
    sentence.push('.');
    Some(sentence)
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

    async fn search(&self, opts: BrowseQuery<'_>) -> Result<BrowseResponse, ProviderError> {
        // GPT4All uses a static JSON catalog, so we fetch all and paginate client-side
        let offset = parse_cursor_offset(opts.cursor)?;

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
        let filtered: Vec<_> = if let Some(q) = opts.query {
            let q_lower = q.to_lowercase();
            gpt4all_models
                .into_iter()
                .filter(|m| {
                    m.name.to_lowercase().contains(&q_lower)
                        || m.filename.to_lowercase().contains(&q_lower)
                        || m.description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&q_lower))
                            .unwrap_or(false)
                        || m.model_type
                            .as_ref()
                            .map(|t| t.to_lowercase().contains(&q_lower))
                            .unwrap_or(false)
                })
                .collect()
        } else {
            gpt4all_models
        };

        let models: Vec<ModelResponse> = filtered.into_iter().map(gpt4all_to_model).collect();

        Ok(paginate_models(
            refine_models(models, &opts),
            offset,
            opts.limit,
        ))
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
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    quant: Option<String>,
    #[serde(rename = "ramrequired", default)]
    ram_required: Option<serde_json::Value>,
    #[serde(default)]
    url: Option<String>,
}

fn gpt4all_to_model(m: Gpt4AllModel) -> ModelResponse {
    let description = m
        .description
        .as_deref()
        .map(html_to_plain_text)
        .filter(|s| !s.is_empty());
    let param_size = m
        .parameters
        .as_deref()
        .map(normalize_parameter_label)
        .or_else(|| extract_param_size(&m.filename));
    let quantization = m
        .quant
        .as_deref()
        .map(|q| q.to_uppercase())
        .or_else(|| extract_quantization(&m.filename));
    let ram_required_gb = m.ram_required.as_ref().and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    });
    let use_cases = nonempty_vec(infer_use_cases(&[
        description.as_deref().unwrap_or(""),
        m.model_type.as_deref().unwrap_or(""),
        &m.name,
        m.description.as_deref().unwrap_or(""),
    ]));

    ModelResponse {
        name: m.name.clone(),
        size: Some(m.filesize),
        description,
        url: m.url,
        use_cases,
        details: Some(ModelDetails {
            format: Some("gguf".to_string()),
            family: m
                .model_type
                .clone()
                .or_else(|| extract_model_family(&m.filename)),
            parameter_size: param_size,
            quantization_level: quantization,
            ram_required_gb,
            ..Default::default()
        }),
        ..Default::default()
    }
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

    async fn search(&self, opts: BrowseQuery<'_>) -> Result<BrowseResponse, ProviderError> {
        let offset = parse_cursor_offset(opts.cursor)?;

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
        let filtered: Vec<_> = if let Some(q) = opts.query {
            let q_lower = q.to_lowercase();
            or_response
                .data
                .into_iter()
                .filter(|m| {
                    m.id.to_lowercase().contains(&q_lower)
                        || m.name.to_lowercase().contains(&q_lower)
                        || m.description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&q_lower))
                            .unwrap_or(false)
                })
                .collect()
        } else {
            or_response.data
        };

        let models: Vec<ModelResponse> = filtered.into_iter().map(openrouter_to_model).collect();

        Ok(paginate_models(
            refine_models(models, &opts),
            offset,
            opts.limit,
        ))
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
    description: Option<String>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    architecture: Option<OpenRouterArchitecture>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    tokenizer: Option<String>,
    #[serde(default)]
    modality: Option<String>,
    #[serde(default)]
    input_modalities: Option<Vec<String>>,
}

fn openrouter_to_model(m: OpenRouterModel) -> ModelResponse {
    let param_size = extract_param_size(&m.id).or_else(|| extract_param_size(&m.name));
    let family = extract_model_family(&m.id)
        .or_else(|| extract_model_family(&m.name))
        .or_else(|| m.architecture.as_ref().and_then(|a| a.tokenizer.clone()));
    let description = m
        .description
        .as_deref()
        .map(collapse_whitespace)
        .filter(|s| !s.is_empty());
    let mut capability_bits = vec![
        m.name.as_str(),
        m.id.as_str(),
        description.as_deref().unwrap_or(""),
        m.architecture
            .as_ref()
            .and_then(|a| a.modality.as_deref())
            .unwrap_or(""),
    ];
    let input_modalities = m
        .architecture
        .as_ref()
        .and_then(|a| a.input_modalities.as_ref())
        .cloned()
        .unwrap_or_default();
    let supported = m.supported_parameters.clone().unwrap_or_default();
    let joined_inputs = input_modalities.join(" ");
    let joined_params = supported.join(" ");
    capability_bits.push(&joined_inputs);
    capability_bits.push(&joined_params);
    let use_cases = nonempty_vec(infer_use_cases(&capability_bits));
    let author = m.id.split_once('/').map(|(owner, _)| owner.to_string());
    let display_name = (m.name != m.id).then_some(m.name.clone());

    ModelResponse {
        name: m.id.clone(),
        display_name,
        description,
        author,
        url: Some(format!("https://openrouter.ai/{}", m.id)),
        use_cases,
        details: Some(ModelDetails {
            format: Some("api".to_string()),
            family,
            parameter_size: param_size,
            context_length: m.context_length,
            ..Default::default()
        }),
        ..Default::default()
    }
}

// =============================================================================
// Utility functions
// =============================================================================

fn huggingface_sort_params(sort: ModelSort) -> (&'static str, i8) {
    match sort {
        ModelSort::UpdatedAsc => ("lastModified", 1),
        ModelSort::UpdatedDesc => ("lastModified", -1),
        ModelSort::Relevance
        | ModelSort::NameAsc
        | ModelSort::NameDesc
        | ModelSort::SizeAsc
        | ModelSort::SizeDesc
        | ModelSort::ParamsAsc
        | ModelSort::ParamsDesc => ("downloads", -1),
    }
}

fn refine_models(mut models: Vec<ModelResponse>, opts: &BrowseQuery<'_>) -> Vec<ModelResponse> {
    models.retain(|model| model_matches_query(model, opts));
    sort_models(&mut models, opts.sort);
    models
}

fn count_matching_models(models: &[ModelResponse], opts: &BrowseQuery<'_>) -> usize {
    models
        .iter()
        .filter(|model| model_matches_query(model, opts))
        .count()
}

fn model_matches_query(model: &ModelResponse, opts: &BrowseQuery<'_>) -> bool {
    if let Some(family) = opts.family
        && !model_matches_family(model, family)
    {
        return false;
    }

    opts.size == ModelSizeFilter::All || model_matches_size(model, opts.size)
}

fn paginate_models(models: Vec<ModelResponse>, offset: usize, limit: usize) -> BrowseResponse {
    let total = models.len();
    let page: Vec<_> = models.into_iter().skip(offset).take(limit).collect();
    let next_offset = offset + page.len();
    let next_cursor = if next_offset < total {
        Some(format!("offset:{}", next_offset))
    } else {
        None
    };

    BrowseResponse {
        models: page,
        next_cursor,
    }
}

fn model_matches_family(model: &ModelResponse, family: &str) -> bool {
    let needle = family.to_lowercase();
    if needle.is_empty() {
        return true;
    }

    if let Some(details) = &model.details
        && let Some(model_family) = &details.family
        && model_family.to_lowercase().contains(&needle)
    {
        return true;
    }

    model.name.to_lowercase().contains(&needle)
}

fn model_matches_size(model: &ModelResponse, size: ModelSizeFilter) -> bool {
    let Some(billions) = model
        .details
        .as_ref()
        .and_then(|details| details.parameter_size.as_deref())
        .and_then(parse_param_billions)
    else {
        return false;
    };

    match size {
        ModelSizeFilter::All => true,
        ModelSizeFilter::Small => billions < 4.0,
        ModelSizeFilter::Medium => (4.0..16.0).contains(&billions),
        ModelSizeFilter::Large => (16.0..40.0).contains(&billions),
        ModelSizeFilter::Xl => billions >= 40.0,
    }
}

fn parse_param_billions(raw: &str) -> Option<f64> {
    let lower = raw.to_lowercase();

    if let Some(prefix) = lower.split("billion").next()
        && lower.contains("billion")
    {
        return prefix.split_whitespace().rev().find_map(|token| {
            token
                .trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
                .parse()
                .ok()
        });
    }

    if let Some(prefix) = lower.split("million").next()
        && lower.contains("million")
    {
        return prefix
            .split_whitespace()
            .rev()
            .find_map(|token| {
                token
                    .trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
                    .parse::<f64>()
                    .ok()
            })
            .map(|millions| millions / 1000.0);
    }

    let mut number = String::new();
    let mut unit = None::<char>;

    for ch in lower.chars() {
        if ch.is_ascii_digit() || (ch == '.' && !number.contains('.')) {
            number.push(ch);
            continue;
        }

        if !number.is_empty() {
            if ch == 'b' || ch == 'm' {
                unit = Some(ch);
            }
            break;
        }
    }

    if number.is_empty() {
        return None;
    }

    let value: f64 = number.parse().ok()?;
    match unit {
        Some('m') => Some(value / 1000.0),
        Some('b') | None => Some(value),
        _ => None,
    }
}

fn sort_models(models: &mut [ModelResponse], sort: ModelSort) {
    match sort {
        ModelSort::Relevance => {}
        ModelSort::NameAsc => {
            models.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
        ModelSort::NameDesc => {
            models.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase()))
        }
        ModelSort::SizeAsc => models.sort_by(|a, b| cmp_optional(a.size, b.size)),
        ModelSort::SizeDesc => models.sort_by(|a, b| cmp_optional(b.size, a.size)),
        ModelSort::ParamsAsc => {
            models.sort_by(|a, b| cmp_optional(param_billions(a), param_billions(b)))
        }
        ModelSort::ParamsDesc => {
            models.sort_by(|a, b| cmp_optional(param_billions(b), param_billions(a)))
        }
        ModelSort::UpdatedAsc => {
            models.sort_by(|a, b| cmp_optional(a.modified_at.as_deref(), b.modified_at.as_deref()))
        }
        ModelSort::UpdatedDesc => {
            models.sort_by(|a, b| cmp_optional(b.modified_at.as_deref(), a.modified_at.as_deref()))
        }
    }
}

fn param_billions(model: &ModelResponse) -> Option<f64> {
    model
        .details
        .as_ref()
        .and_then(|details| details.parameter_size.as_deref())
        .and_then(parse_param_billions)
}

fn cmp_optional<T: PartialOrd>(left: Option<T>, right: Option<T>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

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
    format_param_sizes(extract_all_param_sizes(name))
        .or_else(|| BILLION_RE.captures(name).map(|cap| format!("{}B", &cap[1])))
}

fn extract_all_param_sizes(text: &str) -> Vec<String> {
    let mut sizes = Vec::new();
    for cap in PARAM_SIZE_RE.captures_iter(text) {
        let formatted = format!("{}B", &cap[1]);
        if !sizes.iter().any(|existing| existing == &formatted) {
            sizes.push(formatted);
        }
    }
    sizes.sort_by(|a, b| {
        param_size_value(a)
            .partial_cmp(&param_size_value(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sizes
}

fn collect_param_size_labels(text: &str, chip_sizes: Vec<String>) -> Vec<String> {
    let mut sizes = extract_all_param_sizes(text);
    for chip in chip_sizes {
        let formatted = chip.to_uppercase();
        if !sizes.iter().any(|existing| existing == &formatted) {
            sizes.push(formatted);
        }
    }
    sizes.sort_by(|a, b| {
        param_size_value(a)
            .partial_cmp(&param_size_value(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sizes.dedup();
    sizes
}

fn ollama_size_tag(label: &str) -> String {
    label.trim().to_lowercase()
}

/// Build pullable size options when a library card lists more than one.
fn ollama_size_variants(model_name: &str, labels: &[String]) -> Option<Vec<ModelSize>> {
    if labels.len() < 2 || model_name.contains(':') {
        return None;
    }
    Some(
        labels
            .iter()
            .map(|label| ModelSize {
                name: format!("{}:{}", model_name, ollama_size_tag(label)),
                label: label.clone(),
                size: None,
            })
            .collect(),
    )
}

fn format_param_sizes(mut sizes: Vec<String>) -> Option<String> {
    sizes.sort_by(|a, b| {
        param_size_value(a)
            .partial_cmp(&param_size_value(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sizes.dedup();
    match sizes.len() {
        0 => None,
        1 => Some(sizes.remove(0)),
        2 | 3 => Some(sizes.join(" · ")),
        _ => Some(format!("{}–{}", sizes[0], sizes[sizes.len() - 1])),
    }
}

fn param_size_value(label: &str) -> f64 {
    label
        .trim()
        .trim_end_matches(['B', 'b'])
        .parse()
        .unwrap_or(0.0)
}

fn is_param_size_chip(text: &str) -> bool {
    PARAM_SIZE_CHIP_RE.is_match(text.trim())
}

fn is_capability_tag(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "tools" | "thinking" | "vision" | "embedding" | "embeddings" | "audio" | "code" | "cloud"
    )
}

fn is_ollama_stat_line(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("pulls") || lower.contains("updated") || lower.contains("tag")
}

fn extract_pulls(text: &str) -> Option<u64> {
    PULLS_RE
        .captures(text)
        .and_then(|cap| parse_compact_count(&cap[1]))
}

fn parse_compact_count(raw: &str) -> Option<u64> {
    let s = raw.trim().replace(',', "");
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.chars().last()? {
        'K' | 'k' => (&s[..s.len() - 1], 1_000.0),
        'M' | 'm' => (&s[..s.len() - 1], 1_000_000.0),
        _ => (s.as_str(), 1.0),
    };
    let parsed: f64 = num.trim().parse().ok()?;
    Some((parsed * mult).round() as u64)
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn html_to_plain_text(html: &str) -> String {
    let fragment = Html::parse_fragment(html);
    collapse_whitespace(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

fn nonempty_vec(values: Vec<String>) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn normalize_parameter_label(raw: &str) -> String {
    if let Some(cap) = BILLION_RE.captures(raw) {
        return format!("{}B", &cap[1]);
    }
    extract_param_size(raw).unwrap_or_else(|| raw.to_string())
}

fn humanize_label(raw: &str) -> String {
    raw.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_context_tokens(tokens: u64) -> String {
    const BINARY_WINDOWS: &[u64] = &[
        4_096, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144, 524_288, 1_048_576, 2_097_152,
        4_194_304, 8_388_608,
    ];
    if BINARY_WINDOWS.contains(&tokens) {
        if tokens >= 1_048_576 {
            return format!("{}M", tokens / 1_048_576);
        }
        return format!("{}K", tokens / 1024);
    }
    if tokens >= 1_000_000 && tokens % 1_000_000 == 0 {
        return format!("{}M", tokens / 1_000_000);
    }
    if tokens >= 1_000_000 {
        return format!("{:.1}M", tokens as f64 / 1_000_000.0);
    }
    if tokens >= 1000 {
        return format!("{}K", tokens / 1000);
    }
    tokens.to_string()
}

fn use_cases_from_pipeline(pipeline: Option<&str>, tags: &[String]) -> Vec<String> {
    let mut cases = Vec::new();
    if let Some(tag) = pipeline {
        let mapped = match tag {
            "text-generation" | "text2text-generation" | "conversational" => Some("Chat"),
            "feature-extraction" | "sentence-similarity" => Some("Embeddings"),
            "text-to-image" | "image-to-image" | "image-text-to-image" => Some("Image generation"),
            "automatic-speech-recognition" | "text-to-speech" | "audio-to-audio" => Some("Audio"),
            "image-text-to-text" | "image-to-text" => Some("Vision"),
            _ => None,
        };
        if let Some(label) = mapped {
            cases.push(label.to_string());
        }
    }
    let extras = infer_use_cases(
        &std::iter::once(pipeline.unwrap_or(""))
            .chain(tags.iter().map(String::as_str))
            .collect::<Vec<_>>(),
    );
    for label in extras {
        if !cases.iter().any(|existing| existing == &label) {
            cases.push(label);
        }
    }
    cases.truncate(5);
    cases
}

fn infer_use_cases(parts: &[&str]) -> Vec<String> {
    let blob = parts.join(" ").to_lowercase();
    let mut cases = Vec::new();
    let rules = [
        ("vision", "Vision"),
        ("image", "Vision"),
        ("multimodal", "Multimodal"),
        ("function call", "Tool use"),
        ("tool_choice", "Tool use"),
        ("tools", "Tool use"),
        ("tool use", "Tool use"),
        ("thinking", "Reasoning"),
        ("reasoning", "Reasoning"),
        ("include_reasoning", "Reasoning"),
        ("embedding", "Embeddings"),
        ("audio", "Audio"),
        ("speech", "Audio"),
        ("coding", "Coding"),
        ("code", "Coding"),
        ("agent", "Agents"),
        ("instruct", "Chat"),
        ("conversational", "Chat"),
        ("chat", "Chat"),
    ];
    for (needle, label) in rules {
        if blob.contains(needle) && !cases.iter().any(|existing| existing == label) {
            cases.push(label.to_string());
        }
    }
    cases.truncate(5);
    cases
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

    fn test_model(
        name: &str,
        size: Option<u64>,
        family: Option<&str>,
        params: Option<&str>,
        modified_at: Option<&str>,
    ) -> ModelResponse {
        ModelResponse {
            name: name.to_string(),
            size,
            digest: None,
            modified_at: modified_at.map(ToString::to_string),
            details: Some(ModelDetails {
                format: Some("gguf".to_string()),
                family: family.map(ToString::to_string),
                parameter_size: params.map(ToString::to_string),
                quantization_level: None,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn browse_opts<'a>(
        sort: ModelSort,
        family: Option<&'a str>,
        size: ModelSizeFilter,
    ) -> BrowseQuery<'a> {
        BrowseQuery {
            query: None,
            cursor: None,
            limit: 20,
            sort,
            family,
            size,
        }
    }

    #[test]
    fn test_parse_param_billions() {
        assert_eq!(parse_param_billions("7B"), Some(7.0));
        assert_eq!(parse_param_billions("3.8B"), Some(3.8));
        assert_eq!(parse_param_billions("7 billion"), Some(7.0));
        assert_eq!(parse_param_billions("137M"), Some(0.137));
        assert_eq!(parse_param_billions("335 million"), Some(0.335));
        assert_eq!(parse_param_billions("unknown"), None);
    }

    #[test]
    fn test_model_matches_family() {
        let llama = test_model("llama3.2", None, Some("llama"), Some("3B"), None);
        let mistral = test_model("mistral", None, Some("mistral"), Some("7B"), None);
        let code = test_model("codellama", None, Some("codellama"), Some("7B"), None);

        assert!(model_matches_family(&llama, "llama"));
        assert!(!model_matches_family(&mistral, "llama"));
        assert!(model_matches_family(&code, "code"));
        assert!(model_matches_family(&code, "llama"));
    }

    #[test]
    fn test_model_matches_size() {
        let small = test_model("tiny", None, Some("llama"), Some("3B"), None);
        let medium = test_model("mid", None, Some("mistral"), Some("7B"), None);
        let large = test_model("big", None, Some("qwen"), Some("32B"), None);
        let xl = test_model("huge", None, Some("llama"), Some("70B"), None);
        let unknown = test_model("mystery", None, Some("llama"), None, None);

        assert!(model_matches_size(&small, ModelSizeFilter::Small));
        assert!(model_matches_size(&medium, ModelSizeFilter::Medium));
        assert!(model_matches_size(&large, ModelSizeFilter::Large));
        assert!(model_matches_size(&xl, ModelSizeFilter::Xl));
        assert!(!model_matches_size(&unknown, ModelSizeFilter::Small));
    }

    #[test]
    fn test_refine_models_filters_and_sorts() {
        let models = vec![
            test_model(
                "mistral",
                Some(20),
                Some("mistral"),
                Some("7B"),
                Some("2024-01-01"),
            ),
            test_model(
                "llama-70b",
                Some(40),
                Some("llama"),
                Some("70B"),
                Some("2024-03-01"),
            ),
            test_model(
                "llama-3b",
                Some(10),
                Some("llama"),
                Some("3B"),
                Some("2024-02-01"),
            ),
        ];

        let filtered = refine_models(
            models.clone(),
            &browse_opts(ModelSort::NameAsc, Some("llama"), ModelSizeFilter::All),
        );
        assert_eq!(
            filtered.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["llama-3b", "llama-70b"]
        );

        let small = refine_models(
            models.clone(),
            &browse_opts(ModelSort::Relevance, Some("llama"), ModelSizeFilter::Small),
        );
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].name, "llama-3b");

        let by_params = refine_models(
            models,
            &browse_opts(ModelSort::ParamsDesc, None, ModelSizeFilter::All),
        );
        assert_eq!(by_params[0].name, "llama-70b");
        assert_eq!(by_params[2].name, "llama-3b");
    }

    #[test]
    fn test_paginate_models() {
        let models: Vec<_> = (0..5)
            .map(|i| test_model(&format!("m{}", i), None, None, None, None))
            .collect();

        let page = paginate_models(models, 2, 2);
        assert_eq!(page.models.len(), 2);
        assert_eq!(page.models[0].name, "m2");
        assert_eq!(page.next_cursor, Some("offset:4".to_string()));
    }

    #[test]
    fn test_huggingface_sort_params() {
        assert_eq!(
            huggingface_sort_params(ModelSort::Relevance),
            ("downloads", -1)
        );
        assert_eq!(
            huggingface_sort_params(ModelSort::UpdatedDesc),
            ("lastModified", -1)
        );
        assert_eq!(
            huggingface_sort_params(ModelSort::UpdatedAsc),
            ("lastModified", 1)
        );
    }

    #[test]
    fn test_huggingface_uses_local_window() {
        assert!(!huggingface_uses_local_window(&browse_opts(
            ModelSort::Relevance,
            None,
            ModelSizeFilter::All
        )));
        assert!(!huggingface_uses_local_window(&browse_opts(
            ModelSort::UpdatedDesc,
            Some("llama"),
            ModelSizeFilter::All
        )));
        assert!(huggingface_uses_local_window(&browse_opts(
            ModelSort::NameAsc,
            None,
            ModelSizeFilter::All
        )));
        assert!(huggingface_uses_local_window(&browse_opts(
            ModelSort::SizeDesc,
            None,
            ModelSizeFilter::All
        )));
        assert!(huggingface_uses_local_window(&browse_opts(
            ModelSort::Relevance,
            None,
            ModelSizeFilter::Small
        )));
        assert!(huggingface_uses_local_sort(ModelSort::ParamsDesc));
        assert!(!huggingface_uses_local_sort(ModelSort::UpdatedDesc));
        assert_eq!(huggingface_window_pages(true), HF_WINDOW_PAGES);
        assert_eq!(huggingface_window_pages(false), HF_FILTER_MAX_PAGES);
    }

    #[test]
    fn test_huggingface_window_next_cursor() {
        assert_eq!(
            huggingface_window_next_cursor(Some("offset:20".into()), true, true, true, 0, 20),
            Some("offset:20".into())
        );
        assert_eq!(
            huggingface_window_next_cursor(None, true, true, true, 480, 20),
            None
        );
        assert_eq!(
            huggingface_window_next_cursor(None, true, false, false, 0, 20),
            Some("offset:20".into())
        );
        assert_eq!(
            huggingface_window_next_cursor(None, true, false, true, 0, 20),
            None
        );
    }

    #[test]
    fn test_count_matching_models() {
        let models = vec![
            test_model("llama-3b", None, Some("llama"), Some("3B"), None),
            test_model("llama-70b", None, Some("llama"), Some("70B"), None),
            test_model("mistral", None, Some("mistral"), Some("7B"), None),
        ];

        assert_eq!(
            count_matching_models(
                &models,
                &browse_opts(ModelSort::Relevance, Some("llama"), ModelSizeFilter::Small)
            ),
            1
        );
        assert_eq!(
            count_matching_models(
                &models,
                &browse_opts(ModelSort::Relevance, None, ModelSizeFilter::All)
            ),
            3
        );
    }

    #[test]
    fn test_ollama_manifest_ref() {
        assert_eq!(ollama_manifest_ref("llama3.2"), ("llama3.2", "latest"));
        assert_eq!(ollama_manifest_ref("llama3.2:1b"), ("llama3.2", "1b"));
        assert_eq!(ollama_manifest_ref("llama3.1:70b"), ("llama3.1", "70b"));
        assert_eq!(ollama_manifest_ref("model:"), ("model:", "latest"));
    }

    #[test]
    fn test_ollama_size_variants() {
        let variants = ollama_size_variants("llama3.2", &["1B".into(), "3B".into()]).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name, "llama3.2:1b");
        assert_eq!(variants[1].name, "llama3.2:3b");
        assert!(ollama_size_variants("llama3.2", &["3B".into()]).is_none());
        assert!(ollama_size_variants("llama3.1:70b", &["8B".into(), "70B".into()]).is_none());
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
        assert_eq!(
            extract_param_size("llama-70b-chat"),
            Some("70B".to_string())
        );
        assert_eq!(
            extract_param_size("granite 3B 8B 30B"),
            Some("3B · 8B · 30B".to_string())
        );
        assert_eq!(extract_param_size("8 billion"), Some("8B".to_string()));
        assert_eq!(
            extract_param_size("Qwen3-Coder-30B-A3B-Instruct-GGUF"),
            Some("30B".to_string())
        );
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
        assert_eq!(
            extract_model_family("codellama"),
            Some("codellama".to_string())
        );
        assert_eq!(extract_model_family("glm-5.3"), Some("glm".to_string()));
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
        assert_eq!(huggingface_model_id(&models[0]), "author/model");
        assert_eq!(huggingface_model_id(&models[1]), "other/model2");
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

    #[test]
    fn test_parse_ollama_library_html_extracts_description_and_use_cases() {
        let html = r#"
            <ul role="list">
              <li>
                <a href="/library/llama3.2">
                  <h2><span>llama3.2</span></h2>
                  <p class="max-w-lg">Meta's compact Llama 3.2 for multilingual dialogue and agents.</p>
                  <span>tools</span>
                  <span>3B</span>
                  <span>1B</span>
                  <p><span>28.7K</span><span>Pulls</span></p>
                </a>
              </li>
            </ul>
        "#;

        let models = parse_ollama_library_html(html);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "llama3.2");
        assert_eq!(
            models[0].description.as_deref(),
            Some("Meta's compact Llama 3.2 for multilingual dialogue and agents.")
        );
        assert_eq!(
            models[0]
                .details
                .as_ref()
                .unwrap()
                .parameter_size
                .as_deref(),
            Some("1B · 3B")
        );
        let sizes = models[0].sizes.as_ref().expect("multiple sizes");
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes[0].name, "llama3.2:1b");
        assert_eq!(sizes[0].label, "1B");
        assert_eq!(sizes[1].name, "llama3.2:3b");
        assert_eq!(sizes[1].label, "3B");
        assert_eq!(models[0].downloads, Some(28_700));
        assert!(
            models[0]
                .use_cases
                .as_ref()
                .unwrap()
                .iter()
                .any(|c| c == "Tool use" || c == "Agents" || c == "Chat")
        );
    }

    #[test]
    fn test_huggingface_to_model_includes_catalog_metadata() {
        let json = r#"{
            "id": "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
            "modelId": "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
            "author": "unsloth",
            "likes": 12,
            "downloads": 1000,
            "pipeline_tag": "text-generation",
            "tags": ["gguf", "qwen", "text-generation", "conversational"],
            "gguf": {"totalFileSize": 17000000000, "architecture": "qwen3moe", "context_length": 262144},
            "cardData": {"license": "apache-2.0", "pipeline_tag": "text-generation"}
        }"#;
        let parsed: HuggingFaceModel = serde_json::from_str(json).unwrap();
        let model = huggingface_to_model(parsed);
        assert_eq!(model.size, Some(17_000_000_000));
        assert_eq!(
            model.details.as_ref().unwrap().parameter_size.as_deref(),
            Some("30B")
        );
        assert_eq!(model.details.as_ref().unwrap().context_length, Some(262144));
        assert_eq!(
            model.details.as_ref().unwrap().license.as_deref(),
            Some("apache-2.0")
        );
        assert!(
            model
                .description
                .as_ref()
                .unwrap()
                .contains("Text Generation")
        );
        assert!(
            model
                .use_cases
                .as_ref()
                .unwrap()
                .contains(&"Chat".to_string())
        );
        assert_eq!(model.author.as_deref(), Some("unsloth"));
    }

    #[test]
    fn test_gpt4all_to_model_strips_html_and_exposes_ram() {
        let json = r#"{
            "name":"Reasoner v1",
            "filename":"qwen2.5-coder-7b-instruct-q4_0.gguf",
            "filesize":"4431390720",
            "parameters":"8 billion",
            "type":"qwen2",
            "quant":"q4_0",
            "ramrequired":"8",
            "description":"<ul><li>Use for complex reasoning tasks</li><li>#reasoning</li></ul>",
            "url":"https://example.com/model.gguf"
        }"#;
        let parsed: Gpt4AllModel = serde_json::from_str(json).unwrap();
        let model = gpt4all_to_model(parsed);
        assert_eq!(
            model.details.as_ref().unwrap().parameter_size.as_deref(),
            Some("8B")
        );
        assert_eq!(
            model
                .details
                .as_ref()
                .unwrap()
                .quantization_level
                .as_deref(),
            Some("Q4_0")
        );
        assert_eq!(model.details.as_ref().unwrap().ram_required_gb, Some(8));
        assert!(
            model
                .description
                .as_ref()
                .unwrap()
                .contains("complex reasoning")
        );
        assert!(!model.description.as_ref().unwrap().contains("<li>"));
        assert!(
            model
                .use_cases
                .as_ref()
                .unwrap()
                .contains(&"Reasoning".to_string())
        );
    }

    #[test]
    fn test_openrouter_to_model_keeps_description_and_context() {
        let json = r#"{
            "id":"anthropic/claude-sonnet-4",
            "name":"Anthropic: Claude Sonnet 4",
            "description":"A balanced model for coding and agents.",
            "context_length":200000,
            "architecture":{"tokenizer":"Claude","modality":"text+image->text","input_modalities":["text","image"]},
            "supported_parameters":["tools","include_reasoning"]
        }"#;
        let parsed: OpenRouterModel = serde_json::from_str(json).unwrap();
        let model = openrouter_to_model(parsed);
        assert_eq!(model.name, "anthropic/claude-sonnet-4");
        assert_eq!(
            model.display_name.as_deref(),
            Some("Anthropic: Claude Sonnet 4")
        );
        assert_eq!(
            model.description.as_deref(),
            Some("A balanced model for coding and agents.")
        );
        assert_eq!(model.details.as_ref().unwrap().context_length, Some(200000));
        let cases = model.use_cases.unwrap();
        assert!(cases.contains(&"Coding".to_string()));
        assert!(cases.contains(&"Vision".to_string()));
        assert!(cases.contains(&"Tool use".to_string()));
    }

    #[test]
    fn test_parse_compact_count_and_context_formatting() {
        assert_eq!(parse_compact_count("28.7K"), Some(28_700));
        assert_eq!(parse_compact_count("1.4M"), Some(1_400_000));
        assert_eq!(parse_compact_count("1234"), Some(1234));
        assert_eq!(format_context_tokens(262144), "256K");
        assert_eq!(format_context_tokens(1048576), "1M");
        assert_eq!(format_context_tokens(128000), "128K");
    }
}
