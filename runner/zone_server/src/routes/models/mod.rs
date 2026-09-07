//! Model management endpoints
//!
//! Handles listing, pulling, and deleting models from various sources.

mod providers;
mod types;

pub use providers::{
    DEFAULT_PAGE_SIZE, Gpt4AllProvider, HuggingFaceProvider, MAX_PAGE_SIZE, ModelProvider,
    ProviderError, get_provider, get_provider_with_proxy, huggingface_hub_origin,
};
pub use types::{
    BrowseQuery, BrowseResponse, DiskUsage, ErrorResponse, ListModelsQuery, ModelDetails,
    ModelMediumFilter, ModelResponse, ModelSizeFilter, ModelSort,
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
use types::ModelCapability;
use zone_comfy::caption::{CaptionRequest, Captioner, Draft};
use zone_comfy::lora::{self, TrainRequest};
use zone_comfy::recipe::RecipeCatalog;
use zone_comfy::video::{self, FrameRequest};

// Constants

const MAX_MODEL_NAME_LENGTH: usize = 256;

// Shared HTTP Client for Ollama API calls

static OLLAMA_HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build Ollama HTTP client")
});

// Validation

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
    let proxy_url = state.config().model_search_proxy_url.clone();

    // Check if we're in "browse" mode (source param explicitly provided)
    let is_browse_mode = query.source.is_some();

    match source {
        "ollama" => {
            if is_browse_mode {
                // Browse the Ollama library for available models
                match get_provider_with_proxy("ollama", proxy_url.as_deref()) {
                    Ok(provider) => match provider.search(query.to_browse_query(limit)).await {
                        Ok(response) => Json(response).into_response(),
                        Err(e) => e.into_response(),
                    },
                    Err(e) => e.into_response(),
                }
            } else {
                list_installed_models(state).await
            }
        }
        "comfy" => Json(list_comfy_models(&state)).into_response(),
        "gpt4all" => {
            let provider = match Gpt4AllProvider::with_proxy(
                state.config().gpt4all_models_url.clone(),
                proxy_url.as_deref(),
            ) {
                Ok(provider) => provider,
                Err(error) => return error.into_response(),
            };
            match provider.search(query.to_browse_query(limit)).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => e.into_response(),
            }
        }
        "huggingface" => {
            let provider = match HuggingFaceProvider::with_proxy(
                state.config().huggingface_models_url.clone(),
                proxy_url.as_deref(),
            ) {
                Ok(provider) => provider,
                Err(error) => return error.into_response(),
            };
            let opts = query.to_browse_query(limit);
            match browse_huggingface(&provider, &state, opts).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => e.into_response(),
            }
        }
        "openrouter" => match get_provider_with_proxy(source, proxy_url.as_deref()) {
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

async fn browse_huggingface(
    provider: &HuggingFaceProvider,
    state: &AppState,
    opts: BrowseQuery<'_>,
) -> Result<BrowseResponse, ProviderError> {
    let include_adapters = opts.medium == ModelMediumFilter::ImageGeneration
        || (opts.medium == ModelMediumFilter::All
            && opts
                .query
                .map(str::trim)
                .is_some_and(|query| !query.is_empty()));
    let mut response = provider.search(opts).await?;
    if include_adapters {
        let catalog = RecipeCatalog::load(Some(state.config().comfyui.workflow_path.as_path()))
            .unwrap_or_else(|_| RecipeCatalog::packaged().expect("packaged recipes"));
        let adapters = provider.search_adapters(opts, &catalog.hf_bases()).await?;
        if opts.medium == ModelMediumFilter::ImageGeneration {
            response.models = adapters;
            response.next_cursor = None;
        } else {
            let mut combined = adapters;
            combined.extend(response.models);
            response.models = combined;
        }
    }
    Ok(response)
}

async fn list_installed_models(state: AppState) -> axum::response::Response {
    let comfy = list_comfy_models(&state);
    match list_ollama_model_rows(&state).await {
        Ok(mut models) => {
            models.extend(comfy);
            Json(models).into_response()
        }
        Err(error) => {
            if comfy.is_empty() {
                *error
            } else {
                Json(comfy).into_response()
            }
        }
    }
}

fn list_comfy_models(state: &AppState) -> Vec<ModelResponse> {
    let catalog = RecipeCatalog::load(Some(state.config().comfyui.workflow_path.as_path()))
        .or_else(|_| RecipeCatalog::packaged())
        .ok();
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    zone_comfy::inventory::scan(&state.config().comfyui.models_dir, &catalog)
        .into_iter()
        .map(|item| ModelResponse {
            name: item.filename,
            completion: Some(false),
            size: Some(item.size),
            modified_at: item
                .modified_at
                .or_else(|| Some(chrono::Utc::now().to_rfc3339())),
            description: Some(item.label),
            capabilities: Some(vec![ModelCapability::ImageGeneration]),
            details: Some(ModelDetails {
                format: Some(item.kind),
                family: Some(item.recipe_id.clone()),
                ..Default::default()
            }),
            ready: Some(item.ready),
            recipe_id: Some(item.recipe_id),
            required_files: Some(item.required_files),
            ..Default::default()
        })
        .collect()
}

/// List models from local Ollama installation
async fn list_ollama_model_rows(
    state: &AppState,
) -> Result<Vec<ModelResponse>, Box<axum::response::Response>> {
    let ollama_host = &state.config().ollama_host;

    // Try to fetch from Ollama API
    let url = format!("{}/api/tags", ollama_host);

    match OLLAMA_HTTP_CLIENT.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<OllamaTagsResponse>().await {
                    Ok(tags) => {
                        let mut models: Vec<ModelResponse> = tags
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

                        futures::future::join_all(models.iter_mut().map(|model| async {
                            let profile =
                                crate::services::model::Model::profile(ollama_host, &model.name)
                                    .await;
                            model.capabilities =
                                installed_capabilities(profile.capabilities.as_deref());
                            model.completion = profile.completion;
                            model.tools = profile.tools;
                            model.needs_character = Some(profile.needs_character);
                        }))
                        .await;
                        Ok(models)
                    }
                    Err(e) => Err(Box::new(
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse::new(format!(
                                "Failed to parse response: {}",
                                e
                            ))),
                        )
                            .into_response(),
                    )),
                }
            } else {
                Err(Box::new(
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(ErrorResponse::new("Ollama service unavailable")),
                    )
                        .into_response(),
                ))
            }
        }
        Err(e) => Err(Box::new(
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse::new(format!(
                    "Failed to connect to Ollama: {}",
                    e
                ))),
            )
                .into_response(),
        )),
    }
}

/// Normalize only capabilities declared by the installed Ollama engine.
fn installed_capabilities(declared: Option<&[String]>) -> Option<Vec<ModelCapability>> {
    let mut capabilities = Vec::new();
    for capability in declared? {
        let capability = match capability.as_str() {
            "completion" => ModelCapability::Text,
            "vision" => ModelCapability::ImageInput,
            "image" => ModelCapability::ImageGeneration,
            "audio" => ModelCapability::Audio,
            "tools" => ModelCapability::Tools,
            "embedding" => ModelCapability::Embeddings,
            "thinking" => ModelCapability::Reasoning,
            _ => continue,
        };
        if !capabilities.contains(&capability) {
            capabilities.push(capability);
        }
    }
    (!capabilities.is_empty()).then_some(capabilities)
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

// Model Details & Management

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
                if let Some(info) = huggingface_details_or_none(&state, &name).await {
                    return Json(info).into_response();
                }
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
        Err(e) => {
            if let Some(info) = huggingface_details_or_none(&state, &name).await {
                return Json(info).into_response();
            }
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse::new(format!(
                    "Failed to connect to Ollama: {}",
                    e
                ))),
            )
                .into_response()
        }
    }
}

async fn huggingface_details_or_none(
    state: &AppState,
    name: &str,
) -> Option<types::HuggingFaceModelInfo> {
    let repo = providers::huggingface_repo_id(name)?;
    let info = providers::huggingface_repo_downloads(
        &state.config().huggingface_models_url,
        state.config().model_search_proxy_url.as_deref(),
        repo,
    )
    .await
    .ok()?;
    (info.sizes.is_some() || info.gguf_size.is_some()).then_some(info)
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

    if delete_comfy_weight(&state, &name) {
        return StatusCode::NO_CONTENT.into_response();
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

/// GET /api/models/train/bases
pub async fn train_bases(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    let catalog = RecipeCatalog::load(Some(state.config().comfyui.workflow_path.as_path()))
        .or_else(|_| RecipeCatalog::packaged());
    match catalog {
        Ok(catalog) => Json(lora::available_bases(
            &catalog,
            &state.config().comfyui.models_dir,
        ))
        .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("recipe catalog is not readable")),
        )
            .into_response(),
    }
}

/// POST /api/models/train/captions
pub async fn captions(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(request): Json<CaptionRequest>,
) -> impl IntoResponse {
    let config = state.config();
    let captioner = Captioner::new(
        &config.comfyui,
        config.litellm_host.clone(),
        config.litellm_key.clone(),
    );
    if !captioner.available() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse::new(
                "set COMFYUI_CAPTION_MODEL to a vision model to auto-caption training images",
            )),
        )
            .into_response();
    }
    let clips = request
        .images
        .iter()
        .filter_map(|image| image.group)
        .max()
        .map_or(0, |last| last + 1);
    let mut drafts: Vec<Draft> = request
        .images
        .iter()
        .enumerate()
        .map(|(index, image)| {
            Draft::new(
                &image.filename,
                &image.bytes_base64,
                &image.caption,
                image.group.unwrap_or(clips + index),
            )
        })
        .collect();
    let trigger = request.trigger.unwrap_or_default();
    captioner.fill(&mut drafts, trigger.trim()).await;
    Json(serde_json::json!({
        "captions": drafts.into_iter().map(|draft| draft.caption).collect::<Vec<_>>(),
    }))
    .into_response()
}

/// POST /api/models/train/frames
pub async fn frames(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(request): Json<FrameRequest>,
) -> impl IntoResponse {
    use base64::Engine;
    let config = state.config();
    let resolution = match zone_comfy::train::packaged_config() {
        Ok(settings) => settings.resolution(),
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(error.to_string())),
            )
                .into_response();
        }
    };
    let Ok(video) = base64::engine::general_purpose::STANDARD.decode(request.bytes_base64.trim())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("video is not valid base64")),
        )
            .into_response();
    };
    let options = video::Options {
        fps: request.fps.unwrap_or(config.comfyui.frame_fps),
        resolution,
        mirror: request.mirror.unwrap_or(true),
        limit: config.comfyui.frame_limit as usize,
    };
    match video::extract(&config.comfyui, &video, &request.filename, options).await {
        Ok(clip) => Json(clip).into_response(),
        Err(lora::TrainError::Invalid(message)) => {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response()
        }
        Err(lora::TrainError::Disabled) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse::new(format!(
                "{} is not installed on this server, so a video cannot be turned into training frames",
                config.comfyui.ffmpeg
            ))),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(error.to_string())),
        )
            .into_response(),
    }
}

/// POST /api/models/train
pub async fn train(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(request): Json<TrainRequest>,
) -> impl IntoResponse {
    match lora::train(
        &state.config().comfyui,
        state.config().litellm_host.clone(),
        state.config().litellm_key.clone(),
        request,
    )
    .await
    {
        Ok(path) => Json(serde_json::json!({
            "filename": path.file_name().and_then(|name| name.to_str()),
        }))
        .into_response(),
        Err(lora::TrainError::Disabled) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse::new(
                "LoRA training is not configured on this server",
            )),
        )
            .into_response(),
        Err(lora::TrainError::Invalid(message)) => {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response()
        }
        Err(lora::TrainError::Failed(message)) => {
            (StatusCode::BAD_GATEWAY, Json(ErrorResponse::new(message))).into_response()
        }
    }
}

/// GET /api/models/disk
pub async fn disk(_auth: AuthUser) -> impl IntoResponse {
    let path = std::env::var("ZONE_DISK_PATH").unwrap_or_else(|_| "/".to_string());
    match filesystem_usage(&path) {
        Some(usage) => Json(usage).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("Unable to read disk usage")),
        )
            .into_response(),
    }
}

fn delete_comfy_weight(state: &AppState, name: &str) -> bool {
    let Ok(filename) = zone_comfy::recipe::sanitize_weight_filename(name) else {
        return false;
    };
    let catalog = RecipeCatalog::load(Some(state.config().comfyui.workflow_path.as_path()))
        .or_else(|_| RecipeCatalog::packaged())
        .ok();
    let Some(catalog) = catalog else {
        return false;
    };
    let items = zone_comfy::inventory::scan(&state.config().comfyui.models_dir, &catalog);
    let Some(item) = zone_comfy::inventory::find(&items, &filename) else {
        return false;
    };
    let path = state
        .config()
        .comfyui
        .models_dir
        .join(&item.directory)
        .join(&item.filename);
    let sidecar = path.with_file_name(format!("{}.zone.json", item.filename));
    let removed = std::fs::remove_file(&path).is_ok();
    let _ = std::fs::remove_file(sidecar);
    removed
}

pub(crate) fn filesystem_usage(path: &str) -> Option<DiskUsage> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    let fragment_size = stat.fragment_size() as u64;
    let total_bytes = (stat.blocks() as u64).saturating_mul(fragment_size);
    if total_bytes == 0 {
        return None;
    }
    let available_bytes = (stat.blocks_available() as u64).saturating_mul(fragment_size);
    let used_bytes = total_bytes.saturating_sub(available_bytes);
    Some(DiskUsage {
        used_bytes,
        total_bytes,
        available_bytes,
        percent: (used_bytes as f64 / total_bytes as f64) * 100.0,
    })
}

#[cfg(test)]
mod disk_tests {
    use super::filesystem_usage;

    #[test]
    fn reads_root_filesystem() {
        let usage = filesystem_usage("/").expect("root filesystem");
        assert!(usage.total_bytes > 0);
        assert!(usage.percent >= 0.0 && usage.percent <= 100.0);
        assert_eq!(usage.used_bytes + usage.available_bytes, usage.total_bytes);
    }

    #[test]
    fn missing_path_returns_none() {
        assert!(filesystem_usage("/this/path/does/not/exist").is_none());
    }
}
