//! Model management endpoints
//!
//! Handles listing, pulling, and deleting models from various sources.

mod catalog_endpoints;
mod catalog_failure;
mod method;
mod popular;
mod providers;
mod setup;
#[cfg(test)]
mod snapshots;
mod subject;
mod train_base;
mod train_form;
mod training;
mod types;
mod upload;

pub use method::Method;
pub use setup::{get_setup, start_setup};
pub use subject::Subject;
pub use train_base::TrainBase;
pub use train_form::TrainForm;
pub use types::{BrowseResponse, DiskUsage, ErrorResponse, ListModelsQuery, ModelResponse};

use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};
use std::time::Duration;
use uuid::Uuid;

use super::chats::check_workspace_read_access;
use crate::auth::AuthUser;
use crate::db::ai_settings;
use crate::error::ServerError;
use crate::pull::{ComfyPull, ManifestFile, Pull, PullStart};
use crate::services::comfy::{language, overlay, policy};
use crate::services::endpoint::Origin;
use crate::services::model::Model;
use crate::services::route::Route;
use crate::state::AppState;
use crate::train_jobs;
use crate::train_jobs::{TrainJobStatus, TrainJobView};
use abnegate_comfy::Captioner;
use abnegate_comfy::Config as ComfyConfig;
use abnegate_comfy::host;
use abnegate_comfy::inventory;
use abnegate_comfy::lora::{self, TrainError};
use abnegate_comfy::recipe::{self, RecipeCatalog};
use abnegate_comfy::train;
use abnegate_comfy::video;
use abnegate_llm::catalog::ModelCapability;
use abnegate_llm::catalog::ModelDetails;
use abnegate_llm::catalog::ModelEntry;
use abnegate_llm::catalog::huggingface_hub_origin;
use abnegate_secret::SecretValue;
use tokio::sync::watch;
use training::Training;

const MAX_MODEL_NAME_LENGTH: usize = 256;
const COMFY_SOURCE: &str = "comfy";
pub(crate) const TRAIN_RUNNING: &str = "a training job is already running";
const UNREADABLE_CATALOG: &str = "recipe catalog is not readable";

/// Holds a training-dataset permit for the whole request, refusing rather than
/// queueing. Clip extracts use [`queue_frame_extract`] so a dump of videos can
/// wait its turn instead of 429ing.
///
/// This is middleware rather than a line in each handler because a handler only
/// runs once the body has been read: taking the permit there would bound the
/// work but not the buffering, which is the larger half of what a training
/// upload holds.
pub async fn one_training_upload_at_a_time(
    State(state): State<AppState>,
    request: Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let Ok(_permit) = state.train_semaphore().clone().try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorResponse::new(
                "another training upload is in progress; retry when it finishes",
            )),
        )
            .into_response();
    };
    next.run(request).await
}

/// Queues clip extracts so a dump of hundreds of videos does not 429.
pub async fn queue_frame_extract(
    State(state): State<AppState>,
    request: Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let Ok(_permit) = state.frame_semaphore().clone().acquire_owned().await else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("frame extractor is shutting down")),
        )
            .into_response();
    };
    next.run(request).await
}

static OLLAMA_HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to build Ollama HTTP client")
});

/// Validate model name to prevent injection attacks
pub(crate) fn validate_model_name(name: &str) -> Result<(), ErrorResponse> {
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
///
/// Without a `source` this lists the installed models; with one it browses
/// that catalogue.
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListModelsQuery>,
) -> impl IntoResponse {
    match query.source.as_deref() {
        None => match query.workspace_id {
            Some(workspace) => list_workspace_models(state, &auth, workspace).await,
            None => list_installed_models(state).await,
        },
        Some(COMFY_SOURCE) => Json(list_comfy_models(&state)).into_response(),
        Some(source) => match providers::browse(&state, source, &query).await {
            Ok(response) => Json(response).into_response(),
            Err(failure) => failure.into_response(),
        },
    }
}

pub(crate) async fn browse_models(
    state: &AppState,
    source: &str,
    query: ListModelsQuery,
) -> Result<BrowseResponse, String> {
    if source == COMFY_SOURCE {
        return Ok(BrowseResponse {
            models: list_comfy_models(state),
            next_cursor: None,
        });
    }
    providers::browse(state, source, &query)
        .await
        .map_err(|failure| failure.message())
}

/// The installed inventory when one provider could not be read.
///
/// The rows that could be read are still listed, and the provider that could
/// not is named under `errors` so the console can say so beside them rather
/// than presenting a partial inventory as the whole one.
#[derive(Debug, Serialize)]
pub struct PartialInventory {
    pub models: Vec<ModelResponse>,
    pub errors: ProviderErrors,
}

#[derive(Debug, Serialize)]
pub struct ProviderErrors {
    pub ollama: String,
}

/// Why Ollama's inventory could not be read, with the status it earns on its own.
#[derive(Debug)]
pub(crate) struct OllamaFailure {
    status: StatusCode,
    pub message: String,
}

impl IntoResponse for OllamaFailure {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(ErrorResponse::new(self.message))).into_response()
    }
}

/// The models a chat in `workspace` may run: the Fast and Reasoning models its
/// AI settings save when they send its completions to an endpoint of their
/// own, and otherwise the instance's installed models.
async fn list_workspace_models(
    state: AppState,
    auth: &AuthUser,
    workspace: Uuid,
) -> axum::response::Response {
    if let Err(error) = check_workspace_read_access(&state, auth, workspace).await {
        return error.into_response();
    }
    let route = Route::for_workspace(&state, workspace).await;
    let origin = match route.endpoint() {
        Ok(endpoint) => endpoint.origin(),
        Err(unusable) => return ServerError::Conflict(unusable.to_string()).into_response(),
    };
    match origin {
        Origin::Instance => list_installed_models(state).await,
        Origin::Settings => {
            let preferences = route.preferences(&state.config().comfyui.classifier_model);
            let saved: Vec<ModelResponse> = preferences
                .completions()
                .into_iter()
                .map(saved_model)
                .collect();
            Json(saved).into_response()
        }
    }
}

/// A model the AI settings save, with only what is known of it without asking
/// the endpoint that runs it. Nothing of it is on this instance's disk.
fn saved_model(name: &str) -> ModelResponse {
    let profile = Model::unshown(name);
    let mut entry = ModelEntry::default();
    entry.name = name.to_string();
    entry.size = Some(0);
    entry.modified_at = Some(chrono::Utc::now().to_rfc3339());
    ModelResponse {
        entry,
        completion: profile.completion,
        tools: profile.tools,
        needs_character: Some(profile.needs_character),
        ..Default::default()
    }
}

async fn list_installed_models(state: AppState) -> axum::response::Response {
    let comfy = list_comfy_models(&state);
    match list_ollama_model_rows(&state).await {
        Ok(mut models) => {
            models.extend(comfy);
            Json(models).into_response()
        }
        Err(failure) if comfy.is_empty() => failure.into_response(),
        Err(failure) => Json(PartialInventory {
            models: comfy,
            errors: ProviderErrors {
                ollama: failure.message,
            },
        })
        .into_response(),
    }
}

fn recipe_catalog(config: &ComfyConfig) -> Result<RecipeCatalog, abnegate_comfy::Error> {
    overlay::catalog(config).or_else(|_| RecipeCatalog::packaged())
}

fn scan_inventory(config: &ComfyConfig, catalog: &RecipeCatalog) -> Vec<inventory::InventoryItem> {
    inventory::scan(&config.models_directory, catalog, &config.contract)
}

pub(crate) fn list_comfy_models(state: &AppState) -> Vec<ModelResponse> {
    let config = &state.config().comfyui;
    let Ok(catalog) = recipe_catalog(config) else {
        return Vec::new();
    };
    scan_inventory(config, &catalog)
        .into_iter()
        .map(|item| {
            let mut details = ModelDetails::default();
            details.format = Some(item.kind);
            details.family = Some(item.recipe_id.clone());
            let mut entry = ModelEntry::default();
            entry.name = item.filename;
            entry.size = Some(item.size);
            entry.modified_at = item
                .modified_at
                .or_else(|| Some(chrono::Utc::now().to_rfc3339()));
            entry.description = Some(item.label);
            entry.capabilities = Some(vec![ModelCapability::ImageGeneration]);
            entry.details = Some(details);
            ModelResponse {
                entry,
                completion: Some(false),
                ready: Some(item.ready),
                recipe_id: Some(item.recipe_id),
                required_files: Some(item.required_files),
                ..Default::default()
            }
        })
        .collect()
}

/// List models from local Ollama installation
pub(crate) async fn list_ollama_model_rows(
    state: &AppState,
) -> Result<Vec<ModelResponse>, OllamaFailure> {
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
                            .map(OllamaModel::into_model)
                            .collect();

                        futures::future::join_all(models.iter_mut().map(|model| async {
                            let profile = crate::services::model::Model::profile(
                                ollama_host,
                                &model.entry.name,
                            )
                            .await;
                            model.entry.capabilities =
                                installed_capabilities(profile.capabilities.as_deref());
                            model.completion = profile.completion;
                            model.tools = profile.tools;
                            model.needs_character = Some(profile.needs_character);
                            if let Some(length) = profile.context_length {
                                model
                                    .entry
                                    .details
                                    .get_or_insert_with(ModelDetails::default)
                                    .context_length = Some(length);
                            }
                        }))
                        .await;
                        Ok(models)
                    }
                    Err(e) => Err(OllamaFailure {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        message: format!("Failed to parse response: {}", e),
                    }),
                }
            } else {
                Err(OllamaFailure {
                    status: StatusCode::BAD_GATEWAY,
                    message: "Ollama service unavailable".to_string(),
                })
            }
        }
        Err(e) => Err(OllamaFailure {
            status: StatusCode::BAD_GATEWAY,
            message: format!("Failed to connect to Ollama: {}", e),
        }),
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

impl OllamaModel {
    fn into_model(self) -> ModelResponse {
        let mut entry = ModelEntry::default();
        entry.name = self.name;
        entry.size = Some(self.size);
        entry.digest = Some(self.digest);
        entry.modified_at = Some(self.modified_at);
        entry.details = self.details.map(|reported| {
            let mut details = ModelDetails::default();
            details.format = reported.format;
            details.family = reported.family;
            details.parameter_size = reported.parameter_size;
            details.quantization_level = reported.quantization_level;
            details
        });
        ModelResponse::from(entry)
    }
}

/// GET /api/models/:name
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match show_model(&state, &name).await {
        Ok(info) => Json(info).into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) enum CatalogError {
    InvalidName(ErrorResponse),
    NotFound(String),
    Unavailable(String),
    Parse(String),
}

impl CatalogError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidName(error) => error.error.clone(),
            Self::NotFound(name) => format!("Model not found: {name}"),
            Self::Unavailable(message) => message.clone(),
            Self::Parse(message) => message.clone(),
        }
    }
}

impl IntoResponse for CatalogError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidName(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
            Self::NotFound(name) => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new(format!("Model not found: {name}"))),
            )
                .into_response(),
            Self::Unavailable(message) => {
                (StatusCode::BAD_GATEWAY, Json(ErrorResponse::new(message))).into_response()
            }
            Self::Parse(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(message)),
            )
                .into_response(),
        }
    }
}

pub(crate) async fn show_model(
    state: &AppState,
    name: &str,
) -> Result<serde_json::Value, CatalogError> {
    if let Err(error) = validate_model_name(name) {
        return Err(CatalogError::InvalidName(error));
    }

    let ollama_host = &state.config().ollama_host;
    let url = format!("{}/api/show", ollama_host);

    #[derive(Serialize)]
    struct ShowRequest {
        name: String,
    }

    match OLLAMA_HTTP_CLIENT
        .post(&url)
        .json(&ShowRequest {
            name: name.to_string(),
        })
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(info) => Ok(info),
                    Err(error) => Err(CatalogError::Parse(format!(
                        "Failed to parse response: {error}"
                    ))),
                }
            } else if response.status() == StatusCode::NOT_FOUND {
                if let Some(info) = providers::huggingface_details(state, name).await {
                    return serde_json::to_value(info)
                        .map_err(|error| CatalogError::Parse(error.to_string()));
                }
                Err(CatalogError::NotFound(name.to_string()))
            } else {
                Err(CatalogError::Unavailable("Ollama service error".into()))
            }
        }
        Err(error) => {
            if let Some(info) = providers::huggingface_details(state, name).await {
                return serde_json::to_value(info)
                    .map_err(|error| CatalogError::Parse(error.to_string()));
            }
            Err(CatalogError::Unavailable(format!(
                "Failed to connect to Ollama: {error}"
            )))
        }
    }
}

/// DELETE /api/models/:name
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match delete_named(&state, &name).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn delete_named(state: &AppState, name: &str) -> Result<(), CatalogError> {
    if let Err(error) = validate_model_name(name) {
        return Err(CatalogError::InvalidName(error));
    }

    if delete_comfy_weight(state, name) {
        return Ok(());
    }

    let ollama_host = &state.config().ollama_host;
    let url = format!("{}/api/delete", ollama_host);

    #[derive(Serialize)]
    struct DeleteRequest {
        name: String,
    }

    match OLLAMA_HTTP_CLIENT
        .delete(&url)
        .json(&DeleteRequest {
            name: name.to_string(),
        })
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                Ok(())
            } else if response.status() == StatusCode::NOT_FOUND {
                Err(CatalogError::NotFound(name.to_string()))
            } else {
                Err(CatalogError::Unavailable("Ollama service error".into()))
            }
        }
        Err(error) => Err(CatalogError::Unavailable(format!(
            "Failed to connect to Ollama: {error}"
        ))),
    }
}

/// GET /api/models/train/bases
pub async fn train_bases(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    match train_bases_list(&state).await {
        Ok(bases) => Json(bases).into_response(),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(message)),
        )
            .into_response(),
    }
}

pub(crate) async fn train_bases_list(state: &AppState) -> Result<Vec<TrainBase>, &'static str> {
    let mut bases: Vec<TrainBase> = lora::trainable_bases(&state.config().comfyui)
        .map_err(|_| UNREADABLE_CATALOG)?
        .into_iter()
        .map(TrainBase::from)
        .collect();
    bases.extend(chat_train_bases(state).await);
    Ok(bases)
}

async fn chat_train_bases(state: &AppState) -> Vec<TrainBase> {
    match list_ollama_model_rows(state).await {
        Ok(models) => models
            .into_iter()
            .filter(is_chat_train_model)
            .map(chat_train_base)
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn is_chat_train_model(model: &ModelResponse) -> bool {
    let capabilities = model.entry.capabilities.as_deref().unwrap_or(&[]);
    model.completion == Some(true) || capabilities.contains(&ModelCapability::Text)
}

fn chat_train_base(model: ModelResponse) -> TrainBase {
    let parameter_size = model
        .entry
        .details
        .as_ref()
        .and_then(|details| details.parameter_size.as_deref());
    let finetune = chat_finetune(&model.entry.name, parameter_size);
    TrainBase {
        id: model.entry.name.clone(),
        label: model.entry.name,
        edit: false,
        subject: Subject::Language,
        finetune,
    }
}

fn chat_finetune(name: &str, parameter_size: Option<&str>) -> bool {
    llama32_small_chat(name)
        || crate::services::stages::parse_params(parameter_size)
            .is_some_and(|million| million < 8_000)
}

fn llama32_small_chat(name: &str) -> bool {
    let name = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    matches!(name.as_str(), "llama3.2:1b" | "llama3.2:3b")
}

fn language_train_base<'a>(bases: &'a [TrainBase], id: &str) -> Option<&'a TrainBase> {
    bases.iter().find(|base| base.id == id)
}

/// POST /api/models/train/captions
pub async fn captions(
    State(state): State<AppState>,
    _auth: AuthUser,
    request: Request,
) -> impl IntoResponse {
    let request = match upload::caption_request(request).await {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let config = state.config();
    let captioner = Captioner::new(
        &config.comfyui,
        config.litellm_host.clone(),
        SecretValue::new(config.litellm_key.clone()),
    );
    if !captioner.available() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse::new(
                "no vision model is available to auto-caption training images",
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
    let mut drafts: Vec<_> = request
        .images
        .iter()
        .enumerate()
        .map(|(index, image)| image.draft(image.group.unwrap_or(clips + index)))
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
    request: Request,
) -> impl IntoResponse {
    let request = match upload::frame_request(request).await {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let config = state.config();
    let resolution = match train::packaged_config() {
        Ok(settings) => settings.resolution(),
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(error.to_string())),
            )
                .into_response();
        }
    };
    let video = match request.encoded() {
        Ok(video) => video,
        Err(TrainError::Invalid(message)) => {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response();
        }
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(error.to_string())),
            )
                .into_response();
        }
    };
    let options = video::Options::new(request.fps.unwrap_or(config.comfyui.frame_rate), resolution)
        .with_mirror(request.mirror.unwrap_or(true))
        .with_limit(config.comfyui.frame_limit as usize);
    match video::extract(&config.comfyui, &video, &request.filename, options).await {
        Ok(clip) => Json(clip).into_response(),
        Err(TrainError::Invalid(message)) => {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response()
        }
        Err(TrainError::Disabled) => (
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

/// GET /api/models/train/previews/{name}
pub async fn train_preview(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> impl IntoResponse {
    train_preview_response(&state.config().comfyui, &name)
}

fn plain_preview_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

fn train_preview_response(config: &ComfyConfig, name: &str) -> Response {
    if !plain_preview_name(name) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(path) = host::preview(config, name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match std::fs::read(&path) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct TrainQuery {
    pub workspace_id: Option<Uuid>,
}

/// GET /api/models/train
pub async fn train_job(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    match current_train_job(&state) {
        Some(job) => Json(job).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

pub(crate) fn current_train_job(state: &AppState) -> Option<TrainJobView> {
    if let Some(mut job) = state.train_jobs().current() {
        if let Some(host) = host::current_with_progress(&state.config().comfyui) {
            job.provider = Some(host.compute.as_str().to_string());
            if host.gpu.is_some() {
                job.gpu = host.gpu;
            }
            if host.pod_id.is_some() {
                job.pod_id = host.pod_id;
            }
        }
        return Some(job);
    }
    host::current_with_progress(&state.config().comfyui).map(TrainJobView::from_host)
}

fn train_running() -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse::new(TRAIN_RUNNING)),
    )
        .into_response()
}

/// DELETE /api/models/train
pub async fn dismiss_train(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    match dismiss_train_job(&state) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(DismissError::Busy) => train_running(),
        Err(DismissError::Failed(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(message)),
        )
            .into_response(),
    }
}

#[derive(Debug)]
pub(crate) enum DismissError {
    Busy,
    Failed(String),
}

pub(crate) fn dismiss_train_job(state: &AppState) -> Result<(), DismissError> {
    if state
        .train_jobs()
        .current()
        .is_some_and(|job| job.status == TrainJobStatus::Running)
    {
        return Err(DismissError::Busy);
    }
    let config = &state.config().comfyui;
    if host::busy(config) {
        return Err(DismissError::Busy);
    }
    match host::dismiss(config) {
        Ok(()) => {
            state.train_jobs().clear_finished();
            Ok(())
        }
        Err(_) if host::busy(config) => Err(DismissError::Busy),
        Err(error) => Err(DismissError::Failed(error.to_string())),
    }
}

async fn runpod_key(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: Option<Uuid>,
) -> Result<Option<SecretValue>, ServerError> {
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    check_workspace_read_access(state, auth, workspace_id).await?;
    Ok(ai_settings::runpod_api_key(state.db(), workspace_id).await)
}

fn train_user_id(auth: &AuthUser) -> Result<Uuid, ServerError> {
    auth.0
        .user_id()
        .map_err(|_| ServerError::Unauthorized("Invalid token subject".to_string()))
}

/// POST /api/models/train/uploads
pub async fn create_train_upload(
    State(state): State<AppState>,
    auth: AuthUser,
) -> impl IntoResponse {
    let user_id = match train_user_id(&auth) {
        Ok(user_id) => user_id,
        Err(error) => return error.into_response(),
    };
    match upload::create_staging(&state.config().comfyui.models_directory, user_id) {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(error) => error.into_response(),
    }
}

/// POST /api/models/train/uploads/{id}
pub async fn append_train_upload(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    request: Request,
) -> impl IntoResponse {
    let user_id = match train_user_id(&auth) {
        Ok(user_id) => user_id,
        Err(error) => return error.into_response(),
    };
    let images = match upload::append_images(request).await {
        Ok(images) => images,
        Err(error) => return error.into_response(),
    };
    match upload::append_staging(
        &state.config().comfyui.models_directory,
        id,
        user_id,
        images,
    ) {
        Ok(received) => Json(serde_json::json!({ "id": id, "received": received })).into_response(),
        Err(error) => error.into_response(),
    }
}

/// DELETE /api/models/train/uploads/{id}
pub async fn delete_train_upload(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = match train_user_id(&auth) {
        Ok(user_id) => user_id,
        Err(error) => return error.into_response(),
    };
    match upload::drop_staging(&state.config().comfyui.models_directory, id, user_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => error.into_response(),
    }
}

/// POST /api/models/train
pub async fn train(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<TrainQuery>,
    request: Request,
) -> impl IntoResponse {
    let parsed = match upload::train_request(request).await {
        Ok(parsed) => parsed,
        Err(error) => return error.into_response(),
    };
    let mut form = parsed.form;
    if let Some(upload_id) = parsed.upload_id {
        let user_id = match train_user_id(&auth) {
            Ok(user_id) => user_id,
            Err(error) => return error.into_response(),
        };
        match upload::take_staging(&state.config().comfyui.models_directory, upload_id, user_id) {
            Ok(images) => form.images = images,
            Err(error) => return error.into_response(),
        }
    }
    let saved = if form.compute.is_remote() {
        match runpod_key(&state, &auth, query.workspace_id).await {
            Ok(saved) => saved,
            Err(error) => return error.into_response(),
        }
    } else {
        None
    };
    match start_job(&state, form, saved).await {
        Ok(view) => (StatusCode::ACCEPTED, Json(view)).into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) enum StartTrainError {
    Disabled,
    Invalid(&'static str),
    Failed(String),
    Busy,
    UnknownBase,
    FineTuneTooLarge,
}

impl StartTrainError {
    fn from_train(error: TrainError) -> Self {
        match error {
            TrainError::Disabled => Self::Disabled,
            TrainError::Invalid(message) => Self::Invalid(message),
            TrainError::Failed(message) => Self::Failed(message),
            other => Self::Failed(other.to_string()),
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Disabled => train_jobs::DISABLED.to_string(),
            Self::Invalid(message) => (*message).to_string(),
            Self::Failed(message) => message.clone(),
            Self::Busy => TRAIN_RUNNING.to_string(),
            Self::UnknownBase => language::UNKNOWN_BASE.to_string(),
            Self::FineTuneTooLarge => {
                "fine-tune is only available for small chat models".to_string()
            }
        }
    }
}

impl IntoResponse for StartTrainError {
    fn into_response(self) -> Response {
        if matches!(self, Self::Busy) {
            return train_running();
        }
        let status = match self {
            Self::Disabled => StatusCode::SERVICE_UNAVAILABLE,
            Self::Invalid(_) | Self::Failed(_) | Self::UnknownBase | Self::FineTuneTooLarge => {
                StatusCode::BAD_REQUEST
            }
            Self::Busy => StatusCode::CONFLICT,
        };
        (status, Json(ErrorResponse::new(self.message()))).into_response()
    }
}

pub(crate) async fn start_job(
    state: &AppState,
    form: TrainForm,
    saved: Option<SecretValue>,
) -> Result<TrainJobView, StartTrainError> {
    let training = prepare_training(state, form, saved).await?;
    let config = &state.config().comfyui;
    if host::busy(config) {
        return Err(StartTrainError::Busy);
    }
    let Some(job) = state.train_jobs().start(
        training.name().to_string(),
        Some(training.method().as_str().to_string()),
        Some(training.compute().as_str().to_string()),
    ) else {
        return Err(StartTrainError::Busy);
    };
    let view = job.view();
    let comfyui = config.clone();
    let litellm_host = state.config().litellm_host.clone();
    let litellm_key = SecretValue::new(state.config().litellm_key.clone());
    tokio::spawn(async move {
        let (sender, receiver) = watch::channel(None);
        match training {
            Training::Image(request) => {
                let training =
                    lora::train_reporting(&comfyui, litellm_host, litellm_key, request, sender);
                match job.follow(receiver, training).await {
                    Ok(outcome) => job.succeed(outcome),
                    Err(error) => job.fail(error),
                }
            }
            Training::Language {
                request,
                compute,
                key,
            } => {
                let training = language::train(&comfyui, request, compute, key, sender);
                match job.follow(receiver, training).await {
                    Ok(outcome) => job.succeed_language(outcome),
                    Err(error) => job.fail(error),
                }
            }
        }
    });
    Ok(view)
}

async fn prepare_training(
    state: &AppState,
    form: TrainForm,
    saved: Option<SecretValue>,
) -> Result<Training, StartTrainError> {
    let config = &state.config().comfyui;
    let Some(subject) = form.subject.image() else {
        let bases = chat_train_bases(state).await;
        match language_train_base(&bases, form.base.trim()) {
            None => return Err(StartTrainError::UnknownBase),
            Some(base) if form.method == Method::Finetune && !base.finetune => {
                return Err(StartTrainError::FineTuneTooLarge);
            }
            Some(_) => {}
        }
        let compute = form.compute;
        let request = form.language().map_err(StartTrainError::from_train)?;
        language::validate(config, &request).map_err(StartTrainError::from_train)?;
        let key = policy::key(compute, saved).map_err(StartTrainError::from_train)?;
        return Ok(Training::Language {
            request,
            compute,
            key,
        });
    };
    let compute = form.compute;
    let request = form
        .image(subject)
        .and_then(|request| policy::place(request, compute, saved))
        .map_err(StartTrainError::from_train)?;
    lora::validate_request(config, &request).map_err(StartTrainError::from_train)?;
    Ok(Training::Image(request))
}

pub(crate) fn catalog_pull_start(state: &AppState, request: Pull) -> PullStart {
    let manifest = crate::setup::manifest_weight(&request.model).map(|weight| ManifestFile {
        url: weight.url.clone(),
        relative_path: weight.relative_path.clone(),
        size_bytes: weight.size_bytes,
        sha256: weight.sha256.clone(),
    });
    let comfy = (manifest.is_some() || is_comfy_pull(&request)).then(|| {
        let catalog = recipe_catalog(&state.config().comfyui).ok();
        let recipe_id = request.recipe_id.clone().or_else(|| {
            request.hf_base.as_deref().and_then(|base| {
                catalog
                    .as_ref()
                    .and_then(|catalog| catalog.adapter_recipe_for_base(base))
                    .map(|recipe| recipe.id.clone())
            })
        });
        ComfyPull {
            models_directory: state.config().comfyui.models_directory.clone(),
            recipe_id,
            hf_base: request.hf_base.clone(),
            hub_origin: huggingface_hub_origin(&state.config().huggingface_models_url),
            manifest,
        }
    });
    PullStart {
        model: request.model,
        ollama_host: state.config().ollama_host.clone(),
        comfy,
    }
}

fn is_comfy_pull(request: &Pull) -> bool {
    request
        .runtime
        .as_deref()
        .is_some_and(|runtime| runtime.eq_ignore_ascii_case("comfy"))
        || request.model.contains(".safetensors")
        || crate::setup::is_manifest_id(&request.model)
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

pub(crate) fn delete_comfy_weight(state: &AppState, name: &str) -> bool {
    let Ok(filename) = recipe::sanitize_weight_filename(name) else {
        return false;
    };
    let config = &state.config().comfyui;
    let Ok(catalog) = recipe_catalog(config) else {
        return false;
    };
    let items = scan_inventory(config, &catalog);
    let Some(item) = inventory::find(&items, &filename) else {
        return false;
    };
    let Some(path) =
        confined_weight_path(&config.models_directory, &item.directory, &item.filename)
    else {
        return false;
    };
    let Some(sidecar) = confined_sidecar_path(&path, &item.filename) else {
        return false;
    };
    let removed = std::fs::remove_file(&path).is_ok();
    let _ = std::fs::remove_file(sidecar);
    removed
}

fn single_component(value: &str) -> Option<&str> {
    let mut components = std::path::Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Some(value),
        _ => None,
    }
}

/// The models directory is operator configuration and the two names come from a directory
/// scan, so each must resolve to exactly one path component beneath it.
fn confined_weight_path(
    models_directory: &std::path::Path,
    directory: &str,
    filename: &str,
) -> Option<PathBuf> {
    Some(
        models_directory
            .join(single_component(directory)?)
            .join(single_component(filename)?),
    )
}

fn confined_sidecar_path(weight: &std::path::Path, filename: &str) -> Option<PathBuf> {
    Some(weight.with_file_name(format!("{}.zone.json", single_component(filename)?)))
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

#[cfg(test)]
mod weight_path_tests {
    use super::{confined_sidecar_path, confined_weight_path, single_component};
    use std::path::{Path, PathBuf};

    #[test]
    fn a_component_may_not_escape_its_parent() {
        assert_eq!(single_component("loras"), Some("loras"));
        assert!(single_component("..").is_none());
        assert!(single_component(".").is_none());
        assert!(single_component("").is_none());
        assert!(single_component("loras/nested").is_none());
        assert!(single_component("/etc").is_none());
    }

    #[test]
    fn scanned_weights_resolve_beneath_the_models_directory() {
        assert_eq!(
            confined_weight_path(Path::new("/models"), "loras", "style.safetensors"),
            Some(PathBuf::from("/models/loras/style.safetensors"))
        );
    }

    #[test]
    fn traversal_in_a_scanned_name_is_refused() {
        let models_directory = Path::new("/models");
        assert!(confined_weight_path(models_directory, "..", "style.safetensors").is_none());
        assert!(confined_weight_path(models_directory, "loras", "../../etc/passwd").is_none());
        assert!(
            confined_weight_path(models_directory, "loras/nested", "style.safetensors").is_none()
        );
        assert!(confined_weight_path(models_directory, "loras", "/etc/passwd").is_none());
    }

    #[test]
    fn a_sidecar_sits_beside_its_weight() {
        let weight =
            confined_weight_path(Path::new("/models"), "loras", "style.safetensors").unwrap();
        assert_eq!(
            confined_sidecar_path(&weight, "style.safetensors"),
            Some(PathBuf::from("/models/loras/style.safetensors.zone.json"))
        );
        assert!(confined_sidecar_path(&weight, "../style").is_none());
    }
}

#[cfg(test)]
mod train_preview_tests {
    use super::{plain_preview_name, train_preview_response};
    use crate::services::comfy::settings;
    use abnegate_comfy::Config;
    use abnegate_comfy::host::{Job, JobDirectory, PREVIEWS_DIRECTORY};
    use axum::http::{StatusCode, header};
    use http_body_util::BodyExt;
    use std::fs;
    use std::path::Path;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\npreview";

    fn config(models: &Path) -> Config {
        let mut config = settings::development();
        config.models_directory = models.to_path_buf();
        config
    }

    #[tokio::test]
    async fn train_preview_serves_png_rejects_traversal_and_missing_job() {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let job = Job::new("jerry", "lora", "base.safetensors").with_trigger("ohwx");
        let directory = JobDirectory::new(&config, job.id);
        directory.write_job(&job).unwrap();
        let previews = directory.path().join(PREVIEWS_DIRECTORY);
        fs::create_dir_all(&previews).unwrap();
        fs::write(previews.join("step-250-0.png"), PNG).unwrap();

        let served = train_preview_response(&config, "step-250-0.png");
        assert_eq!(served.status(), StatusCode::OK);
        assert_eq!(
            served.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png"
        );
        let body = served.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], PNG);

        let junk = train_preview_response(&config, "../job.json");
        assert_eq!(junk.status(), StatusCode::BAD_REQUEST);

        let empty = tempfile::tempdir().unwrap();
        let missing = train_preview_response(&self::config(empty.path()), "step-250-0.png");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn only_a_plain_file_name_is_a_preview() {
        assert!(plain_preview_name("step-250-0.png"));
        for name in ["", "../job.json", "previews/a.png", "a\\b.png", "a..png"] {
            assert!(!plain_preview_name(name), "{name}");
        }
    }
}

#[cfg(test)]
mod chat_train_base_tests {
    use super::{chat_finetune, is_chat_train_model};
    use crate::routes::models::types::ModelResponse;
    use abnegate_llm::catalog::{ModelCapability, ModelDetails, ModelEntry};

    fn model(
        name: &str,
        completion: Option<bool>,
        capabilities: &[ModelCapability],
        parameter_size: Option<&str>,
    ) -> ModelResponse {
        let mut entry = ModelEntry::default();
        entry.name = name.to_string();
        entry.capabilities = (!capabilities.is_empty()).then(|| capabilities.to_vec());
        entry.details = parameter_size.map(|size| {
            let mut details = ModelDetails::default();
            details.parameter_size = Some(size.to_string());
            details
        });
        ModelResponse {
            entry,
            completion,
            ..Default::default()
        }
    }

    #[test]
    fn chat_filter_keeps_completion_and_completion_plus_vision() {
        assert!(is_chat_train_model(&model(
            "llama3.2:1b",
            Some(true),
            &[ModelCapability::Text],
            Some("1B"),
        )));
        assert!(is_chat_train_model(&model(
            "qwen3.8:27b",
            Some(true),
            &[ModelCapability::Text, ModelCapability::ImageInput],
            Some("27.3B"),
        )));
        assert!(!is_chat_train_model(&model(
            "qwen3-embedding:0.6b",
            Some(false),
            &[ModelCapability::Embeddings],
            Some("0.6B"),
        )));
        assert!(!is_chat_train_model(&model(
            "llava:7b",
            None,
            &[ModelCapability::ImageInput],
            Some("7B"),
        )));
    }

    #[test]
    fn fine_tune_is_for_small_chat_models() {
        assert!(chat_finetune("llama3.2:1b", Some("1B")));
        assert!(chat_finetune("llama3.2:3b", Some("3.2B")));
        assert!(chat_finetune("llama3.2:1b", None));
        assert!(chat_finetune("qwen2.5:7b", Some("7B")));
        assert!(!chat_finetune("llama3.1:8b", Some("8B")));
        assert!(!chat_finetune("qwen3.8:27b", Some("27.3B")));
    }
}
