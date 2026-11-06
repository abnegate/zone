//! Catalog, install, and train as ordinary chat tools.

use super::PREVIEW_TITLE_CHARS;
use super::tools::{WorkspaceScope, optional_string_arg, string_arg};
use crate::auth::permission::{self, Permission};
use crate::db::workspace_members::{self, WorkspaceRole};
use crate::pull::Pull;
use crate::routes::models::{
    self, CatalogError, DismissError, ListModelsQuery, Method, Subject, TrainForm, browse_models,
    catalog_pull_start, current_train_job, delete_named, dismiss_train_job, filesystem_usage,
    list_comfy_models, list_ollama_model_rows, show_model, start_job, train_bases_list,
    validate_model_name,
};
use crate::state::AppState;
use abnegate_comfy::host::Compute;
use abnegate_comfy::lora::{TrainError, TrainImage};
use abnegate_comfy::{train, video};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zone_core::tools::{Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult, excerpt};

const LIST_CAP: usize = 25;
const START_TIMEOUT_SECS: u64 = 600;
const NO_DOCUMENTS: &str = "training needs documents";
const NO_IMAGES: &str = "training needs images";
const CLIP_EXTENSIONS: [&str; 6] = ["mp4", "mov", "m4v", "webm", "mkv", "avi"];

pub(crate) const NAMES: [&str; 10] = [
    "cancel_model_install",
    "delete_model",
    "dismiss_train",
    "get_model",
    "get_model_install",
    "get_train_job",
    "install_model",
    "list_models",
    "list_train_bases",
    "start_train",
];

#[derive(Clone, Copy)]
enum Operation {
    List,
    Get,
    Install,
    InstallStatus,
    CancelInstall,
    Delete,
    ListBases,
    GetTrain,
    StartTrain,
    DismissTrain,
}

impl Operation {
    fn permission(self) -> Option<Permission> {
        match self {
            Self::Install | Self::CancelInstall | Self::StartTrain | Self::DismissTrain => {
                Some(Permission::ModelsCreate)
            }
            Self::Delete => Some(Permission::ModelsDelete),
            Self::List | Self::Get | Self::InstallStatus | Self::ListBases | Self::GetTrain => None,
        }
    }
}

struct ModelTool {
    scope: WorkspaceScope,
    operation: Operation,
}

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    for operation in [
        Operation::List,
        Operation::Get,
        Operation::Install,
        Operation::InstallStatus,
        Operation::CancelInstall,
        Operation::Delete,
        Operation::ListBases,
        Operation::GetTrain,
        Operation::StartTrain,
        Operation::DismissTrain,
    ] {
        registry.register(Arc::new(ModelTool {
            scope: scope.clone(),
            operation,
        }));
    }
}

#[async_trait]
impl Tool for ModelTool {
    fn name(&self) -> &str {
        match self.operation {
            Operation::List => "list_models",
            Operation::Get => "get_model",
            Operation::Install => "install_model",
            Operation::InstallStatus => "get_model_install",
            Operation::CancelInstall => "cancel_model_install",
            Operation::Delete => "delete_model",
            Operation::ListBases => "list_train_bases",
            Operation::GetTrain => "get_train_job",
            Operation::StartTrain => "start_train",
            Operation::DismissTrain => "dismiss_train",
        }
    }

    fn description(&self) -> &str {
        match self.operation {
            Operation::List => {
                "List installed Ollama and Comfy models, with disk used/total. Pass source (ollama, huggingface, comfy, gpt4all) plus optional query and cursor to browse a catalog. Returns at most 25 rows."
            }
            Operation::Get => {
                "Show one installed Ollama model, or Hugging Face GGUF/size options when it is not installed. Call this before install_model when the user has not named a quant."
            }
            Operation::Install => {
                "Start installing a model and return immediately with current progress. runtime is ollama (default) or comfy for a .safetensors weight. Poll get_model_install; do not claim the install finished until that tool reports success."
            }
            Operation::InstallStatus => {
                "Current install progress for a model name: percent, steps, and a terminal success or error. Empty when nothing is installing that name."
            }
            Operation::CancelInstall => "Cancel an in-progress model install by name.",
            Operation::Delete => "Delete an installed Comfy weight or Ollama model by name.",
            Operation::ListBases => {
                "List training bases: image recipes that are ready, plus installed Ollama chat models (finetune is true only for small chat models)."
            }
            Operation::GetTrain => {
                "Current training job with progress, or a short note when none is running. Poll this after start_train; do not claim the job finished until status is succeeded or failed."
            }
            Operation::StartTrain => {
                "Start a training job from host file paths and return immediately. One job at a time. Poll get_train_job; do not claim it finished from this call. Person uses the SDXL people base and a trigger. Language dumps documents."
            }
            Operation::DismissTrain => {
                "Dismiss a finished training job so it no longer appears. Refuses a job that is still running."
            }
        }
    }

    fn parameters_schema(&self) -> Value {
        match self.operation {
            Operation::List => json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "enum": ["ollama", "huggingface", "comfy", "gpt4all"],
                        "description": "Catalog to browse. Omit to list installed models."
                    },
                    "query": {
                        "type": "string",
                        "description": "Search text for a browsed catalog, or a name filter on the installed list."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Pagination cursor from a previous browse."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 25,
                        "default": 25
                    }
                },
                "additionalProperties": false
            }),
            Operation::Get
            | Operation::InstallStatus
            | Operation::CancelInstall
            | Operation::Delete => {
                json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Model name, tag, Hugging Face repo, or Comfy filename."
                        }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                })
            }
            Operation::Install => json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Ollama model:tag, hf.co reference, or owner/repo:filename.safetensors."
                    },
                    "runtime": {
                        "type": "string",
                        "enum": ["ollama", "comfy"],
                        "description": "ollama (default) or comfy for a .safetensors weight."
                    },
                    "recipe_id": {
                        "type": "string",
                        "description": "Comfy recipe id to stamp on a downloaded adapter sidecar."
                    },
                    "hf_base": {
                        "type": "string",
                        "description": "Hugging Face base id used to pick the adapter recipe when recipe_id is omitted."
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            Operation::ListBases | Operation::GetTrain | Operation::DismissTrain => {
                json!({"type": "object", "properties": {}, "additionalProperties": false})
            }
            Operation::StartTrain => json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Adapter or chat-model name to publish as. A pivotal adapter's name is only letters, digits, dashes or underscores."
                    },
                    "base": {
                        "type": "string",
                        "description": "Training base id from list_train_bases."
                    },
                    "subject": {
                        "type": "string",
                        "enum": ["person", "other", "language"],
                        "description": "Defaults from the chosen base."
                    },
                    "method": {
                        "type": "string",
                        "enum": ["lora", "finetune", "pivotal"],
                        "description": "Defaults to lora."
                    },
                    "trigger": {
                        "type": "string",
                        "description": "Identity token the adapter learns. Required for a person."
                    },
                    "provider": {
                        "type": "string",
                        "enum": ["local", "runpod"],
                        "description": "Defaults to local. runpod needs a workspace Runpod API key."
                    },
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Host file paths: images, clips, or language .jsonl/.json/.txt/.md. Relative paths resolve against the tool working directory."
                    }
                },
                "required": ["name", "base", "paths"],
                "additionalProperties": false
            }),
        }
    }

    fn tier(&self) -> Tier {
        match self.operation {
            Operation::Install
            | Operation::CancelInstall
            | Operation::Delete
            | Operation::StartTrain
            | Operation::DismissTrain => Tier::Write,
            _ => Tier::Read,
        }
    }

    fn preview(&self, params: &Value) -> Option<String> {
        match self.operation {
            Operation::Install => Some(format!(
                "Install model {}.",
                excerpt(params["name"].as_str()?, PREVIEW_TITLE_CHARS)
            )),
            Operation::CancelInstall => Some(format!(
                "Cancel install of {}.",
                excerpt(params["name"].as_str()?, PREVIEW_TITLE_CHARS)
            )),
            Operation::Delete => Some(format!(
                "Delete model {}.",
                excerpt(params["name"].as_str()?, PREVIEW_TITLE_CHARS)
            )),
            Operation::StartTrain => {
                let name = excerpt(params["name"].as_str()?, PREVIEW_TITLE_CHARS);
                let base = excerpt(params["base"].as_str()?, PREVIEW_TITLE_CHARS);
                let count = params["paths"].as_array().map(Vec::len).unwrap_or(0);
                Some(format!(
                    "Start training {name} on {base} from {count} path(s)."
                ))
            }
            Operation::DismissTrain => Some("Dismiss the finished training job.".to_string()),
            _ => None,
        }
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        match self.operation {
            Operation::StartTrain => Duration::from_secs(START_TIMEOUT_SECS),
            _ => Duration::from_secs(30),
        }
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(self.run(&params, context).await)
    }
}

impl ModelTool {
    async fn run(&self, params: &Value, context: &ToolContext) -> ToolResult {
        if self.tier().mutating() {
            match workspace_members::has_role_or_higher(
                self.scope.state.db(),
                self.scope.user_id,
                self.scope.workspace_id,
                WorkspaceRole::Member,
            )
            .await
            {
                Ok(true) => {}
                Ok(false) => {
                    return ToolResult::error(
                        "You do not have permission to change models in this workspace.",
                    );
                }
                Err(error) => {
                    tracing::warn!(tool = self.name(), %error, "Model tool authorization failed");
                    return ToolResult::error(
                        "The model operation failed. Check current state before retrying a write.",
                    );
                }
            }
        }
        if let Some(permission) = self.operation.permission() {
            match permission::holds(self.scope.state.db(), self.scope.user_id, permission).await {
                Ok(true) => {}
                Ok(false) => return ToolResult::error(permission.refusal()),
                Err(error) => {
                    tracing::warn!(tool = self.name(), %error, "Model permission check failed");
                    return ToolResult::error(
                        "The model operation failed. Check current state before retrying a write.",
                    );
                }
            }
        }
        match self.operation {
            Operation::List => list(&self.scope, params).await,
            Operation::Get => get(&self.scope.state, params).await,
            Operation::Install => install(&self.scope.state, params),
            Operation::InstallStatus => install_status(&self.scope.state, params),
            Operation::CancelInstall => cancel_install(&self.scope.state, params),
            Operation::Delete => delete(&self.scope.state, params).await,
            Operation::ListBases => list_bases(&self.scope.state).await,
            Operation::GetTrain => get_train(&self.scope.state),
            Operation::StartTrain => start_train(&self.scope, params, &context.cwd).await,
            Operation::DismissTrain => dismiss_train(&self.scope.state),
        }
    }
}

async fn list(scope: &WorkspaceScope, params: &Value) -> ToolResult {
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| (n as usize).clamp(1, LIST_CAP))
        .unwrap_or(LIST_CAP);
    let query = optional_string_arg(params, "query");
    let cursor = optional_string_arg(params, "cursor").map(str::to_string);
    match optional_string_arg(params, "source") {
        Some(source) => {
            let listing = ListModelsQuery {
                source: Some(source.to_string()),
                search: query.map(str::to_string),
                cursor,
                limit: Some(limit),
                sort: None,
                family: None,
                size: None,
                medium: None,
                workspace_id: Some(scope.workspace_id),
            };
            match browse_models(&scope.state, source, listing).await {
                Ok(mut response) => {
                    let omitted = trim_models(&mut response.models, limit);
                    json_ok(json!({
                        "models": response.models,
                        "next_cursor": response.next_cursor,
                        "models_omitted": omitted,
                    }))
                }
                Err(error) => ToolResult::error(error),
            }
        }
        None => {
            let mut models = installed_models(&scope.state).await;
            if let Some(query) = query {
                let needle = query.to_ascii_lowercase();
                models.retain(|model| {
                    model.name.to_ascii_lowercase().contains(&needle)
                        || model.description.as_deref().is_some_and(|description| {
                            description.to_ascii_lowercase().contains(&needle)
                        })
                });
            }
            let omitted = trim_models(&mut models, limit);
            let path = std::env::var("ZONE_DISK_PATH").unwrap_or_else(|_| "/".to_string());
            json_ok(json!({
                "models": models,
                "disk": filesystem_usage(&path),
                "models_omitted": omitted,
            }))
        }
    }
}

async fn installed_models(state: &AppState) -> Vec<models::ModelResponse> {
    let comfy = list_comfy_models(state);
    match list_ollama_model_rows(state).await {
        Ok(mut models) => {
            models.extend(comfy);
            models
        }
        Err(failure) => {
            tracing::warn!(error = %failure.message, "Could not list installed Ollama models");
            comfy
        }
    }
}

async fn get(state: &AppState, params: &Value) -> ToolResult {
    let name = match string_arg(params, "name") {
        Ok(name) => name,
        Err(error) => return error,
    };
    match show_model(state, name).await {
        Ok(info) => json_ok(info),
        Err(error) => ToolResult::error(error.message()),
    }
}

fn install(state: &AppState, params: &Value) -> ToolResult {
    let name = match string_arg(params, "name") {
        Ok(name) => name.to_string(),
        Err(error) => return error,
    };
    if let Err(error) = validate_model_name(&name) {
        return ToolResult::error(error.error);
    }
    let runtime = optional_string_arg(params, "runtime").map(str::to_string);
    let recipe_id = optional_string_arg(params, "recipe_id").map(str::to_string);
    let hf_base = optional_string_arg(params, "hf_base").map(str::to_string);
    let _subscription = state.pull_registry().start(catalog_pull_start(
        state,
        Pull {
            model: name.clone(),
            cancel: false,
            runtime,
            recipe_id,
            hf_base,
        },
    ));
    match state.pull_registry().status(&name) {
        Some(view) => json_ok(view),
        None => ToolResult::success(format!(
            "Started install of {name}. Poll get_model_install."
        )),
    }
}

fn install_status(state: &AppState, params: &Value) -> ToolResult {
    let name = match string_arg(params, "name") {
        Ok(name) => name,
        Err(error) => return error,
    };
    match state.pull_registry().status(name) {
        Some(view) => json_ok(view),
        None => json_ok(json!({"name": name, "active": false})),
    }
}

fn cancel_install(state: &AppState, params: &Value) -> ToolResult {
    let name = match string_arg(params, "name") {
        Ok(name) => name,
        Err(error) => return error,
    };
    if let Err(error) = validate_model_name(name) {
        return ToolResult::error(error.error);
    }
    if state.pull_registry().cancel(name) {
        ToolResult::success(format!("Cancelled install of {name}."))
    } else {
        ToolResult::success(format!("No install in progress for {name}."))
    }
}

async fn delete(state: &AppState, params: &Value) -> ToolResult {
    let name = match string_arg(params, "name") {
        Ok(name) => name,
        Err(error) => return error,
    };
    match delete_named(state, name).await {
        Ok(()) => ToolResult::success(format!("Deleted {name}.")),
        Err(CatalogError::InvalidName(error)) => ToolResult::error(error.error),
        Err(error) => ToolResult::error(error.message()),
    }
}

async fn list_bases(state: &AppState) -> ToolResult {
    match train_bases_list(state).await {
        Ok(bases) => json_ok(bases),
        Err(message) => ToolResult::error(message),
    }
}

fn get_train(state: &AppState) -> ToolResult {
    match current_train_job(state) {
        Some(job) => json_ok(job),
        None => ToolResult::success("No training job."),
    }
}

fn dismiss_train(state: &AppState) -> ToolResult {
    if current_train_job(state).is_none() {
        return ToolResult::success("No training job to dismiss.");
    }
    match dismiss_train_job(state) {
        Ok(()) => ToolResult::success("Dismissed the finished training job."),
        Err(DismissError::Busy) => ToolResult::error(models::TRAIN_RUNNING),
        Err(DismissError::Failed(message)) => ToolResult::error(message),
    }
}

async fn start_train(scope: &WorkspaceScope, params: &Value, cwd: &Path) -> ToolResult {
    let name = match string_arg(params, "name") {
        Ok(name) => name.to_string(),
        Err(error) => return error,
    };
    let base = match string_arg(params, "base") {
        Ok(base) => base.to_string(),
        Err(error) => return error,
    };
    let method = match optional_string_arg(params, "method") {
        Some(value) => match parse_method(value) {
            Ok(method) => method,
            Err(error) => return error,
        },
        None => Method::Lora,
    };
    let subject = match optional_string_arg(params, "subject") {
        Some(value) => match parse_subject(value) {
            Ok(subject) => subject,
            Err(error) => return error,
        },
        None => infer_subject(&scope.state, &base, method).await,
    };
    let compute = match optional_string_arg(params, "provider") {
        Some(value) => match parse_compute(value) {
            Ok(compute) => compute,
            Err(error) => return error,
        },
        None => Compute::Local,
    };
    let paths = match string_list(params, "paths") {
        Ok(paths) => paths,
        Err(error) => return error,
    };
    if paths.is_empty() {
        return ToolResult::error(empty_paths_message(subject));
    }
    let images = match load_training_paths(&scope.state, cwd, &paths, subject).await {
        Ok(images) => images,
        Err(error) => return ToolResult::error(error),
    };
    let trigger = optional_string_arg(params, "trigger").map(str::to_string);
    let form = TrainForm {
        name,
        base,
        trigger,
        subject,
        method,
        images,
        compute,
    };
    let saved = if compute.is_remote() {
        crate::db::ai_settings::runpod_api_key(scope.state.db(), scope.workspace_id).await
    } else {
        None
    };
    match start_job(&scope.state, form, saved).await {
        Ok(view) => json_ok(json!({
            "job": view,
            "note": "Training continues after this call. Poll get_train_job; do not claim it finished.",
        })),
        Err(error) => ToolResult::error(error.message()),
    }
}

fn empty_paths_message(subject: Subject) -> &'static str {
    if subject == Subject::Language {
        NO_DOCUMENTS
    } else {
        NO_IMAGES
    }
}

async fn infer_subject(state: &AppState, base: &str, method: Method) -> Subject {
    if let Ok(bases) = train_bases_list(state).await
        && let Some(found) = bases.into_iter().find(|item| item.id == base)
    {
        return found.subject;
    }
    if method == Method::Lora {
        Subject::Other
    } else {
        Subject::Person
    }
}

fn parse_subject(value: &str) -> Result<Subject, ToolResult> {
    Subject::parse(value)
        .ok_or_else(|| ToolResult::error("subject must be person, other, or language"))
}

fn parse_method(value: &str) -> Result<Method, ToolResult> {
    match value {
        "lora" => Ok(Method::Lora),
        "finetune" => Ok(Method::Finetune),
        "pivotal" => Ok(Method::Pivotal),
        _ => Err(ToolResult::error(
            "method must be lora, finetune, or pivotal",
        )),
    }
}

fn parse_compute(value: &str) -> Result<Compute, ToolResult> {
    [Compute::Local, Compute::Runpod]
        .into_iter()
        .find(|compute| compute.as_str() == value)
        .ok_or_else(|| ToolResult::error("provider must be local or runpod"))
}

fn string_list(params: &Value, key: &str) -> Result<Vec<String>, ToolResult> {
    match params.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut paths = Vec::new();
            for item in items {
                let Some(path) = item
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Err(ToolResult::error("paths must be an array of file paths"));
                };
                paths.push(path.to_string());
            }
            Ok(paths)
        }
        _ => Err(ToolResult::error("paths must be an array of file paths")),
    }
}

async fn load_training_paths(
    state: &AppState,
    cwd: &Path,
    paths: &[String],
    subject: Subject,
) -> Result<Vec<TrainImage>, String> {
    let limit = state
        .config()
        .train_upload_limit_mb
        .saturating_mul(1024 * 1024);
    let resolved: Vec<PathBuf> = paths.iter().map(|path| resolve_path(cwd, path)).collect();
    let mut used = 0u64;
    for path in &resolved {
        let meta =
            std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if meta.is_dir() {
            return Err(format!("{} is a directory; pass files", path.display()));
        }
        used = used.saturating_add(meta.len());
        if used > limit {
            return Err(format!(
                "training files exceed the {} MB upload budget",
                state.config().train_upload_limit_mb
            ));
        }
    }
    let mut images = Vec::new();
    let mut group_offset = 0usize;
    for path in resolved {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} is not a usable file name", path.display()))?
            .to_string();
        let bytes = std::fs::read(&path).map_err(|error| format!("{filename}: {error}"))?;
        if subject == Subject::Language {
            images.push(image_from_bytes(filename, bytes, None));
            continue;
        }
        if is_clip(&filename) {
            let clip = extract_clip(state, &bytes, &filename).await?;
            let max_group = clip
                .frames
                .iter()
                .map(|frame| frame.group)
                .max()
                .unwrap_or(0);
            for frame in clip.frames {
                let pixels = decode_frame(&frame.bytes_base64)?;
                images.push(image_from_bytes(
                    frame.filename,
                    pixels,
                    Some(frame.group + group_offset),
                ));
            }
            group_offset += max_group + 1;
        } else {
            images.push(image_from_bytes(filename, bytes, None));
        }
    }
    Ok(images)
}

fn is_clip(filename: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            CLIP_EXTENSIONS
                .iter()
                .any(|clip| extension.eq_ignore_ascii_case(clip))
        })
}

fn resolve_path(cwd: &Path, given: &str) -> PathBuf {
    let path = Path::new(given);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn image_from_bytes(filename: String, bytes: Vec<u8>, group: Option<usize>) -> TrainImage {
    let image = TrainImage::from_bytes(filename, String::new(), bytes);
    match group {
        Some(group) => image.with_group(group),
        None => image,
    }
}

fn decode_frame(encoded: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| "extracted frame is not valid base64".to_string())
}

async fn extract_clip(
    state: &AppState,
    video: &[u8],
    filename: &str,
) -> Result<video::Clip, String> {
    let config = &state.config().comfyui;
    let resolution = train::packaged_config()
        .map(|settings| settings.resolution())
        .map_err(|error| error.to_string())?;
    let options =
        video::Options::new(config.frame_rate, resolution).with_limit(config.frame_limit as usize);
    match video::extract(config, video, filename, options).await {
        Ok(clip) => Ok(clip),
        Err(TrainError::Invalid(message)) => Err(message.to_string()),
        Err(TrainError::Disabled) => Err(format!(
            "{} is not installed on this server, so a video cannot be turned into training frames",
            config.ffmpeg
        )),
        Err(error) => Err(error.to_string()),
    }
}

fn trim_models<T>(models: &mut Vec<T>, limit: usize) -> usize {
    let omitted = models.len().saturating_sub(limit);
    models.truncate(limit);
    omitted
}

fn json_ok(value: impl Serialize) -> ToolResult {
    match serde_json::to_string(&value) {
        Ok(output) => ToolResult::success(output),
        Err(error) => ToolResult::error(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfy::policy;
    use crate::state::test_config;
    use crate::train_jobs::TrainJobStatus;
    use uuid::Uuid;
    use zone_core::tools::ToolRegistry;

    const PIXEL: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE, 0xD4, 0xEF, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn scope_with(state: AppState) -> WorkspaceScope {
        WorkspaceScope {
            state,
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
            offline: false,
        }
    }

    fn scope() -> WorkspaceScope {
        scope_with(AppState::for_tests())
    }

    fn train_state() -> (tempfile::TempDir, AppState) {
        let directory = tempfile::tempdir().expect("scratch models dir");
        let mut config = test_config();
        config.comfyui.enabled = true;
        config.comfyui.train_command = Some("true".into());
        config.comfyui.models_directory = directory.path().to_path_buf();
        let database =
            sqlx::PgPool::connect_lazy("postgres://localhost/test").expect("a lazy pool");
        (directory, AppState::new(config, database, None))
    }

    fn still(filename: &str) -> TrainImage {
        image_from_bytes(filename.to_string(), PIXEL.to_vec(), None)
    }

    fn form(base: &str, trigger: Option<&str>, subject: Subject) -> TrainForm {
        TrainForm {
            name: "Ada".into(),
            base: base.into(),
            trigger: trigger.map(str::to_string),
            subject,
            method: Method::Lora,
            images: vec![still("shot.png")],
            compute: Compute::Local,
        }
    }

    #[tokio::test]
    async fn catalog_tools_register_on_a_chat_and_publish_stable_contracts() {
        let scope = scope();
        let mut registry = ToolRegistry::new();
        register(&mut registry, &scope);
        let mut names = registry.names();
        names.sort_unstable();
        assert_eq!(names, NAMES);

        let list = ModelTool {
            scope: scope.clone(),
            operation: Operation::List,
        };
        assert_eq!(list.tier(), Tier::Read);
        assert!(list.preview(&json!({})).is_none());

        let start = ModelTool {
            scope: scope.clone(),
            operation: Operation::StartTrain,
        };
        assert_eq!(start.tier(), Tier::Write);
        assert_eq!(
            start.preview(&json!({"name": "Ada", "base": "flux-schnell", "paths": ["a.png"]})),
            Some("Start training Ada on flux-schnell from 1 path(s).".into())
        );
        let schema = start.parameters_schema();
        assert_eq!(schema["required"], json!(["name", "base", "paths"]));
        assert_eq!(
            schema["properties"]["method"]["enum"],
            json!(["lora", "finetune", "pivotal"])
        );
        let written = format!("{}{}", start.description(), schema);
        assert!(!written.contains("wait_for"), "{written}");

        let install = ModelTool {
            scope,
            operation: Operation::Install,
        };
        assert_eq!(
            install.preview(&json!({"name": "llama3.2:3b"})),
            Some("Install model llama3.2:3b.".into())
        );
        assert!(!install.description().contains("wait_for"));
    }

    #[tokio::test]
    async fn every_write_needs_a_model_permission_and_no_read_does() {
        let operations = [
            Operation::List,
            Operation::Get,
            Operation::Install,
            Operation::InstallStatus,
            Operation::CancelInstall,
            Operation::Delete,
            Operation::ListBases,
            Operation::GetTrain,
            Operation::StartTrain,
            Operation::DismissTrain,
        ];
        for operation in operations {
            let tool = ModelTool {
                scope: scope(),
                operation,
            };
            assert_eq!(
                operation.permission().is_some(),
                tool.tier() == Tier::Write,
                "{} must need a permission exactly when it writes",
                tool.name()
            );
        }
        assert_eq!(
            Operation::Delete.permission(),
            Some(Permission::ModelsDelete)
        );
        assert_eq!(
            Operation::Install.permission(),
            Some(Permission::ModelsCreate)
        );
    }

    async fn member_with_role(role: &str) -> WorkspaceScope {
        use crate::db::{organizations, users, workspaces};
        let pool =
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").expect("a migrated DATABASE_URL"))
                .await
                .unwrap();
        let user = users::create_user(
            &pool,
            &format!("{}@example.com", Uuid::new_v4()),
            "hash",
            Some("Model tool member"),
            false,
        )
        .await
        .unwrap();
        users::assign_user_role(&pool, user.id, role).await.unwrap();
        let organization = organizations::create_organization(
            &pool,
            "Model tool tests",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Models",
            &Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap();
        workspace_members::add_member(&pool, workspace.id, user.id, WorkspaceRole::Member, None)
            .await
            .unwrap();
        let mut config = test_config();
        config.ollama_host = "http://127.0.0.1:1".into();
        WorkspaceScope {
            state: AppState::new(config, pool, None),
            workspace_id: workspace.id,
            chat_id: Some(Uuid::new_v4()),
            user_id: user.id,
            offline: false,
        }
    }

    async fn run(scope: &WorkspaceScope, name: &str, params: Value) -> ToolResult {
        let mut registry = ToolRegistry::new();
        register(&mut registry, scope);
        registry
            .get(name)
            .unwrap()
            .execute(params, &ToolContext::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_workspace_member_without_model_permissions_cannot_change_server_models() {
        let scope = member_with_role("user").await;
        for (name, params, permission) in [
            (
                "install_model",
                json!({"name": "llama3.2:3b"}),
                Permission::ModelsCreate,
            ),
            (
                "cancel_model_install",
                json!({"name": "llama3.2:3b"}),
                Permission::ModelsCreate,
            ),
            ("dismiss_train", json!({}), Permission::ModelsCreate),
            (
                "delete_model",
                json!({"name": "llama3.2:3b"}),
                Permission::ModelsDelete,
            ),
        ] {
            let result = run(&scope, name, params).await;
            assert!(!result.success, "{name} must be refused");
            assert_eq!(
                result.error.as_deref(),
                Some(permission.refusal().as_str()),
                "{name}"
            );
        }
        assert!(scope.state.pull_registry().status("llama3.2:3b").is_none());
    }

    #[tokio::test]
    async fn a_workspace_member_with_the_admin_role_reaches_the_model_backend() {
        let scope = member_with_role("admin").await;
        let result = run(&scope, "delete_model", json!({"name": "llama3.2:3b"})).await;
        assert_ne!(
            result.error.as_deref(),
            Some(Permission::ModelsDelete.refusal().as_str())
        );
    }

    #[test]
    fn empty_paths_use_the_same_messages_as_validate_request() {
        assert_eq!(empty_paths_message(Subject::Other), "training needs images");
        assert_eq!(
            empty_paths_message(Subject::Person),
            "training needs images"
        );
        assert_eq!(
            empty_paths_message(Subject::Language),
            "training needs documents"
        );
    }

    #[test]
    fn the_train_tool_does_not_take_the_video_method() {
        let refused = parse_method("video").expect_err("video identity training is off");
        assert_eq!(
            refused.error.as_deref(),
            Some("method must be lora, finetune, or pivotal")
        );
    }

    #[tokio::test]
    async fn video_identity_training_is_refused_before_a_job_starts() {
        let (_dir, state) = train_state();
        let request = TrainForm {
            name: "Ada".into(),
            base: policy::PEOPLE_BASE.into(),
            trigger: Some("ada".into()),
            subject: Subject::Person,
            method: Method::Video,
            images: vec![image_from_bytes("walk.mp4".into(), b"clip".to_vec(), None)],
            compute: Compute::Local,
        };
        let error = start_job(&state, request, None)
            .await
            .expect_err("video identity training is off");
        assert_eq!(error.message(), policy::VIDEO_REFUSAL);
        assert!(state.train_jobs().current().is_none());
    }

    #[tokio::test]
    async fn a_pivotal_name_with_a_dot_is_refused_before_a_job_starts() {
        let (directory, state) = train_state();
        let checkpoint = directory.path().join(policy::PEOPLE_CHECKPOINT);
        std::fs::create_dir_all(checkpoint.parent().expect("checkpoints")).expect("checkpoints");
        std::fs::write(&checkpoint, b"weights").expect("checkpoint");
        let request = TrainForm {
            name: "ada.v2".into(),
            method: Method::Pivotal,
            ..form(policy::PEOPLE_BASE, Some("ada"), Subject::Person)
        };
        let error = start_job(&state, request, None)
            .await
            .expect_err("an embedding named ada.v2 cannot be prompted");
        assert_eq!(error.message(), policy::PIVOTAL_NAME);
        assert!(state.train_jobs().current().is_none());
    }

    #[test]
    fn a_clip_is_a_file_in_a_container_ffmpeg_reads_frames_from() {
        for clip in [
            "walk.mp4",
            "walk.MOV",
            "walk.webm",
            "walk.m4v",
            "walk.mkv",
            "walk.avi",
        ] {
            assert!(is_clip(clip), "{clip}");
        }
        for still in ["walk.png", "walk.mp4.png", "walk", "mp4"] {
            assert!(!is_clip(still), "{still}");
        }
    }

    #[tokio::test]
    async fn start_train_with_a_png_reaches_the_registry() {
        let (_dir, state) = train_state();
        let request = form("flux-schnell", Some("ada"), Subject::Other);
        match start_job(&state, request, None).await {
            Ok(view) => {
                assert_eq!(view.name, "Ada");
                assert_eq!(view.status, TrainJobStatus::Running);
                assert!(state.train_jobs().current().is_some());
            }
            Err(error) => {
                let message = error.message();
                assert!(
                    message.contains("not configured")
                        || message.contains("invalid")
                        || message.contains("unknown training base")
                        || message.contains("trigger word is required"),
                    "{message}"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_busy_registry_refuses_a_second_job() {
        let (_dir, state) = train_state();
        assert!(
            state
                .train_jobs()
                .start("held".into(), Some("lora".into()), Some("local".into()))
                .is_some()
        );
        let request = form("flux-schnell", Some("ada"), Subject::Other);
        let error = start_job(&state, request, None)
            .await
            .expect_err("the slot is held");
        assert!(
            error
                .message()
                .contains("a training job is already running"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn a_person_run_without_a_trigger_is_refused() {
        let (directory, state) = train_state();
        let checkpoint = directory.path().join(policy::PEOPLE_CHECKPOINT);
        std::fs::create_dir_all(checkpoint.parent().expect("a checkpoints directory"))
            .expect("the checkpoints directory");
        std::fs::write(&checkpoint, b"weights").expect("the people checkpoint");
        let request = form("sdxl-people", None, Subject::Person);
        let error = start_job(&state, request, None)
            .await
            .expect_err("a person needs a trigger");
        assert!(
            error.message().contains("trigger word is required"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn runpod_without_a_saved_key_is_refused_before_training() {
        let (_dir, state) = train_state();
        let mut request = form("sdxl-people", Some("ada"), Subject::Person);
        request.compute = Compute::Runpod;
        let error = start_job(&state, request, None)
            .await
            .expect_err("Runpod needs a saved key");
        assert_eq!(
            error.message(),
            "Save a Runpod API key in Workspace Settings."
        );
        assert!(state.train_jobs().current().is_none());
    }

    #[test]
    fn the_tool_reads_the_wire_names_of_subjects_and_computes() {
        assert_eq!(parse_subject("language").ok(), Some(Subject::Language));
        assert_eq!(parse_subject("person").ok(), Some(Subject::Person));
        assert_eq!(
            parse_subject("robot").err().and_then(|result| result.error),
            Some("subject must be person, other, or language".into())
        );
        assert_eq!(parse_compute("runpod").ok(), Some(Compute::Runpod));
        assert_eq!(parse_compute("local").ok(), Some(Compute::Local));
        assert_eq!(
            parse_compute("lambda")
                .err()
                .and_then(|result| result.error),
            Some("provider must be local or runpod".into())
        );
    }

    #[tokio::test]
    async fn dismiss_refuses_a_running_job() {
        let (_dir, state) = train_state();
        assert!(
            state
                .train_jobs()
                .start("held".into(), Some("lora".into()), Some("local".into()))
                .is_some()
        );
        match dismiss_train_job(&state) {
            Err(DismissError::Busy) => {}
            other => panic!("expected busy, got {other:?}"),
        }
        assert!(
            dismiss_train(&state)
                .error
                .as_deref()
                .is_some_and(|message| message.contains("a training job is already running"))
        );
    }

    #[tokio::test]
    async fn install_starts_a_pull_and_status_reads_it() {
        let state = AppState::for_tests();
        let result = install(&state, &json!({"name": "llama3.2:3b", "runtime": "ollama"}));
        assert!(result.success, "{result:?}");
        let status = install_status(&state, &json!({"name": "llama3.2:3b"}));
        assert!(status.success, "{status:?}");
        let body = status.output.expect("status body");
        assert!(body.contains("llama3.2:3b"), "{body}");
        let empty = install_status(&state, &json!({"name": "missing"}));
        assert!(
            empty
                .output
                .as_deref()
                .is_some_and(|body| body.contains("\"active\":false")),
            "{empty:?}"
        );
    }

    #[tokio::test]
    async fn delete_rejects_invalid_names_the_same_way_as_http() {
        let state = AppState::for_tests();
        let error = delete_named(&state, "hello world")
            .await
            .expect_err("spaces are invalid");
        assert_eq!(error.message(), "Invalid characters in model name");
        let empty = delete_named(&state, "")
            .await
            .expect_err("an empty name is invalid");
        assert_eq!(empty.message(), "Invalid model name length");
        let tool = delete(&state, &json!({"name": "hello world"})).await;
        assert_eq!(
            tool.error.as_deref(),
            Some("Invalid characters in model name")
        );
    }
}
