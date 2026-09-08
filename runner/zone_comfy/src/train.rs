//! HTTP client that runs repository-owned LoRA training graphs on ComfyUI.

use crate::config::Config;
use crate::lora::TrainError;
use crate::recipe::TrainingModel;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const PACKAGED_TRAIN_CONFIG: &str =
    include_str!("../../../comfyui/custom_nodes/zone_lora/train_config.json");
const MIN_WEIGHT_BYTES: usize = 10_000;
const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub struct TrainConfig {
    passes_per_image: u32,
    min_steps: u32,
    max_steps: u32,
    rank: u32,
    learning_rate: f64,
    lora_dtype: String,
    training_dtype: String,
    resolution: u32,
    bypass_mode: bool,
    gradient_checkpointing: bool,
    checkpoint_depth: u32,
    seed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub folder: String,
    pub artifact: String,
}

impl Run {
    pub fn new() -> Self {
        let id = Uuid::new_v4();
        Self {
            folder: format!("zone-train-{id}"),
            artifact: format!("zone-lora-{id}"),
        }
    }

    pub fn validate(&self) -> Result<(), TrainError> {
        validate_run_name(&self.folder, "zone-train-")?;
        validate_run_name(&self.artifact, "zone-lora-")
    }
}

impl Default for Run {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PromptResponse {
    prompt_id: Uuid,
    number: f64,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    node_errors: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadResponse {
    name: String,
    subfolder: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Default, Deserialize)]
struct HistoryStatus {
    #[serde(default)]
    status_str: String,
    #[serde(default)]
    completed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct Manifest {
    schema_version: u32,
    architecture: &'static str,
    pairs: Vec<Pair>,
}

#[derive(Debug, Serialize)]
struct Pair {
    index: usize,
    target: String,
    reference: Option<String>,
    instruction: String,
}

pub fn packaged_config() -> Result<TrainConfig, TrainError> {
    serde_json::from_str(PACKAGED_TRAIN_CONFIG)
        .map_err(|error| TrainError::Failed(format!("train config: {error}")))
}

impl TrainConfig {
    pub fn resolution(&self) -> u32 {
        self.resolution
    }

    pub fn steps(&self, image_count: usize) -> u32 {
        u32::try_from(image_count.max(1))
            .unwrap_or(u32::MAX)
            .saturating_mul(self.passes_per_image)
            .clamp(self.min_steps, self.max_steps)
    }
}

pub async fn run(
    config: &Config,
    model: &TrainingModel,
    work: &Path,
    output: &Path,
    image_count: usize,
) -> Result<Run, TrainError> {
    if !config.enabled {
        return Err(TrainError::Disabled);
    }
    let run = Run::new();
    let result = execute(config, model, work, output, image_count, &run).await;
    if result.is_err() {
        cleanup(config, &run).await;
    }
    result.map(|()| run)
}

async fn execute(
    config: &Config,
    model: &TrainingModel,
    work: &Path,
    output: &Path,
    image_count: usize,
    run: &Run,
) -> Result<(), TrainError> {
    run.validate()?;
    let settings = packaged_config()?;
    let client = client(config)?;
    let manifest = stage_or_upload(&client, config, model, work, run).await?;
    let graph = train_graph(
        model,
        &run.folder,
        &manifest,
        &run.artifact,
        &settings,
        settings.steps(image_count),
    );
    let prompt = queue(&client, config, graph).await?;
    wait_prompt(
        &client,
        config,
        prompt,
        Duration::from_secs(config.train_timeout_secs),
    )
    .await?;
    download(&client, config, run, output).await
}

fn client(config: &Config) -> Result<reqwest::Client, TrainError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(config.train_timeout_secs))
        .build()
        .map_err(|error| TrainError::Failed(error.to_string()))
}

async fn queue(
    client: &reqwest::Client,
    config: &Config,
    graph: Value,
) -> Result<Uuid, TrainError> {
    let response = authorize(
        config,
        client
            .post(format!("{}/prompt", config.base_url))
            .json(&json!({ "prompt": graph })),
    )
    .send()
    .await
    .map_err(|error| TrainError::Failed(error.to_string()))?
    .error_for_status()
    .map_err(|error| TrainError::Failed(error.to_string()))?
    .json::<PromptResponse>()
    .await
    .map_err(|error| TrainError::Failed(format!("invalid ComfyUI prompt response: {error}")))?;
    if response
        .error
        .as_ref()
        .is_some_and(|error| !error.is_null())
        || !response.node_errors.is_empty()
    {
        return Err(TrainError::Failed(format!(
            "ComfyUI rejected train graph: {:?}",
            response
                .error
                .unwrap_or_else(|| json!(response.node_errors))
        )));
    }
    if !response.number.is_finite() || response.number < 0.0 {
        return Err(TrainError::Failed(
            "ComfyUI returned an invalid queue number".into(),
        ));
    }
    Ok(response.prompt_id)
}

async fn download(
    client: &reqwest::Client,
    config: &Config,
    run: &Run,
    output: &Path,
) -> Result<(), TrainError> {
    let filename = format!("{}.safetensors", run.artifact);
    let response = authorize(
        config,
        client.get(format!(
            "{}/view?filename={}&subfolder=loras&type=output",
            config.base_url,
            urlencoding::encode(&filename)
        )),
    )
    .send()
    .await
    .map_err(|error| TrainError::Failed(error.to_string()))?
    .error_for_status()
    .map_err(|error| TrainError::Failed(error.to_string()))?;
    let disposition = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if disposition != format!("filename=\"{filename}\"") {
        return Err(TrainError::Failed(
            "ComfyUI view response named a different artifact".into(),
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type != "application/octet-stream" {
        return Err(TrainError::Failed(
            "ComfyUI view response is not safetensors data".into(),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    if bytes.len() < MIN_WEIGHT_BYTES {
        return Err(TrainError::Failed(
            "ComfyUI returned a LoRA that is too small to be trained weights".into(),
        ));
    }
    atomic_write(output, &bytes)
}

pub(crate) fn atomic_write(output: &Path, bytes: &[u8]) -> Result<(), TrainError> {
    let parent = output
        .parent()
        .ok_or_else(|| TrainError::Failed("training output has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|error| TrainError::Failed(error.to_string()))?;
    if fs::symlink_metadata(output).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(TrainError::Failed(
            "training output cannot be a symlink".into(),
        ));
    }
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| TrainError::Failed("training output filename is invalid".into()))?;
    let temporary = parent.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        fs::rename(&temporary, output).map_err(|error| TrainError::Failed(error.to_string()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn train_graph(
    model: &TrainingModel,
    folder: &str,
    manifest: &str,
    artifact: &str,
    settings: &TrainConfig,
    steps: u32,
) -> Value {
    match model {
        TrainingModel::Flux { checkpoint } => {
            flux_graph(checkpoint, folder, manifest, artifact, settings, steps)
        }
        TrainingModel::QwenEdit { unet, clip, vae } => {
            qwen_graph(unet, clip, vae, folder, manifest, artifact, settings, steps)
        }
    }
}

fn trainer(
    model: Value,
    latents: Value,
    positive: Value,
    artifact: &str,
    settings: &TrainConfig,
    steps: u32,
) -> Value {
    json!({
        "class_type": "ZoneTrainLoRA",
        "inputs": {
            "model": model,
            "latents": latents,
            "positive": positive,
            "steps": steps,
            "learning_rate": settings.learning_rate,
            "rank": settings.rank,
            "seed": settings.seed,
            "training_dtype": settings.training_dtype,
            "lora_dtype": settings.lora_dtype,
            "gradient_checkpointing": settings.gradient_checkpointing,
            "checkpoint_depth": settings.checkpoint_depth,
            "bypass_mode": settings.bypass_mode,
            "save_name": artifact
        }
    })
}

fn flux_graph(
    checkpoint: &str,
    folder: &str,
    manifest: &str,
    artifact: &str,
    settings: &TrainConfig,
    steps: u32,
) -> Value {
    let mut graph = json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": { "ckpt_name": checkpoint }
        },
        "2": {
            "class_type": "ZoneLoadTrainDataset",
            "inputs": {
                "folder": folder,
                "manifest_json": manifest,
                "resolution": settings.resolution
            }
        },
        "3": {
            "class_type": "VAEEncode",
            "inputs": { "pixels": ["2", 0], "vae": ["1", 2] }
        },
        "4": {
            "class_type": "CLIPTextEncode",
            "inputs": { "text": ["2", 2], "clip": ["1", 1] }
        }
    });
    graph["5"] = trainer(
        json!(["1", 0]),
        json!(["3", 0]),
        json!(["4", 0]),
        artifact,
        settings,
        steps,
    );
    graph
}

#[allow(clippy::too_many_arguments)]
fn qwen_graph(
    unet: &str,
    clip: &str,
    vae: &str,
    folder: &str,
    manifest: &str,
    artifact: &str,
    settings: &TrainConfig,
    steps: u32,
) -> Value {
    let mut graph = json!({
        "1": {
            "class_type": "UNETLoader",
            "inputs": { "unet_name": unet, "weight_dtype": "default" }
        },
        "2": {
            "class_type": "CLIPLoader",
            "inputs": { "clip_name": clip, "type": "qwen_image" }
        },
        "3": {
            "class_type": "VAELoader",
            "inputs": { "vae_name": vae }
        },
        "4": {
            "class_type": "ZoneLoadTrainDataset",
            "inputs": {
                "folder": folder,
                "manifest_json": manifest,
                "resolution": settings.resolution
            }
        },
        "5": {
            "class_type": "VAEEncode",
            "inputs": { "pixels": ["4", 0], "vae": ["3", 0] }
        },
        "6": {
            "class_type": "TextEncodeQwenImageEditPlus",
            "inputs": {
                "clip": ["2", 0],
                "prompt": ["4", 2],
                "vae": ["3", 0],
                "image1": ["4", 1]
            }
        }
    });
    graph["7"] = trainer(
        json!(["1", 0]),
        json!(["5", 0]),
        json!(["6", 0]),
        artifact,
        settings,
        steps,
    );
    graph
}

async fn stage_or_upload(
    client: &reqwest::Client,
    config: &Config,
    model: &TrainingModel,
    work: &Path,
    run: &Run,
) -> Result<String, TrainError> {
    let pairs = pairs(model, work)?;
    let manifest = serde_json::to_string(&Manifest {
        schema_version: MANIFEST_VERSION,
        architecture: architecture(model),
        pairs,
    })
    .map_err(|error| TrainError::Failed(error.to_string()))?;
    let input = config
        .models_dir
        .parent()
        .map(|parent| parent.join("input"));
    if let Some(input) = input.filter(|path| path.is_dir()) {
        stage_local(work, &input, run)?;
    } else {
        stage_remote(client, config, model, work, run).await?;
    }
    Ok(manifest)
}

fn pairs(model: &TrainingModel, work: &Path) -> Result<Vec<Pair>, TrainError> {
    let targets = pngs(&work.join("targets"))?;
    let controls = pngs(&work.join("control_1")).unwrap_or_default();
    if matches!(model, TrainingModel::QwenEdit { .. }) && targets.len() != controls.len() {
        return Err(TrainError::Invalid(
            "Qwen edit training needs one reference for every target",
        ));
    }
    let mut pairs = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        let expected = format!("{index:04}.png");
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(TrainError::Invalid("training image name is invalid"))?;
        if name != expected {
            return Err(TrainError::Invalid(
                "training image names must be contiguous indices",
            ));
        }
        let instruction_path = target.with_extension("txt");
        if fs::symlink_metadata(&instruction_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(TrainError::Invalid(
                "training instructions must be regular files",
            ));
        }
        let instruction = fs::read_to_string(instruction_path)
            .map_err(|_| TrainError::Invalid("every training image needs an instruction"))?;
        if instruction.trim().is_empty() {
            return Err(TrainError::Invalid(
                "every training image needs an instruction",
            ));
        }
        let reference = match model {
            TrainingModel::Flux { .. } => None,
            TrainingModel::QwenEdit { .. } => {
                let control = controls.get(index).ok_or(TrainError::Invalid(
                    "Qwen edit training needs one reference for every target",
                ))?;
                if control.file_name() != target.file_name() {
                    return Err(TrainError::Invalid(
                        "Qwen edit references must use the target index",
                    ));
                }
                Some(format!("control_1/{expected}"))
            }
        };
        pairs.push(Pair {
            index,
            target: format!("targets/{expected}"),
            reference,
            instruction: instruction.trim().to_string(),
        });
    }
    if pairs.is_empty() {
        return Err(TrainError::Invalid("training needs images"));
    }
    Ok(pairs)
}

fn stage_local(work: &Path, input: &Path, run: &Run) -> Result<(), TrainError> {
    let destination = input.join(&run.folder);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(TrainError::Failed(
            "training namespace already exists".into(),
        ));
    }
    fs::create_dir(&destination).map_err(|error| TrainError::Failed(error.to_string()))?;
    let result = (|| {
        for directory in ["targets", "control_1"] {
            let source = work.join(directory);
            if !source.is_dir() {
                continue;
            }
            let target = destination.join(directory);
            fs::create_dir(&target).map_err(|error| TrainError::Failed(error.to_string()))?;
            for source in pngs(&source)? {
                let name = source
                    .file_name()
                    .ok_or(TrainError::Invalid("training image name is invalid"))?;
                fs::copy(&source, target.join(name))
                    .map_err(|error| TrainError::Failed(error.to_string()))?;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&destination);
    }
    result
}

async fn stage_remote(
    client: &reqwest::Client,
    config: &Config,
    model: &TrainingModel,
    work: &Path,
    run: &Run,
) -> Result<(), TrainError> {
    for directory in ["targets", "control_1"] {
        if directory == "control_1" && matches!(model, TrainingModel::Flux { .. }) {
            continue;
        }
        for png in pngs(&work.join(directory))? {
            let name = png
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(TrainError::Invalid("training image name is invalid"))?;
            upload_png(
                client,
                config,
                &format!("{}/{directory}", run.folder),
                name,
                &fs::read(&png).map_err(|error| TrainError::Failed(error.to_string()))?,
            )
            .await?;
        }
    }
    Ok(())
}

fn pngs(directory: &Path) -> Result<Vec<PathBuf>, TrainError> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| TrainError::Failed(error.to_string()))? {
        let entry = entry.map_err(|error| TrainError::Failed(error.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("png") {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        if kind.is_symlink() || !kind.is_file() {
            return Err(TrainError::Invalid("training images must be regular files"));
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}

async fn upload_png(
    client: &reqwest::Client,
    config: &Config,
    subfolder: &str,
    filename: &str,
    bytes: &[u8],
) -> Result<(), TrainError> {
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str("image/png")
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .part("image", part)
        .text("overwrite", "false")
        .text("type", "input")
        .text("subfolder", subfolder.to_string());
    let uploaded = authorize(
        config,
        client
            .post(format!("{}/upload/image", config.base_url))
            .multipart(form),
    )
    .send()
    .await
    .map_err(|error| TrainError::Failed(error.to_string()))?
    .error_for_status()
    .map_err(|error| TrainError::Failed(error.to_string()))?
    .json::<UploadResponse>()
    .await
    .map_err(|error| TrainError::Failed(format!("invalid ComfyUI upload response: {error}")))?;
    if uploaded.name != filename || uploaded.subfolder != subfolder || uploaded.kind != "input" {
        return Err(TrainError::Failed(
            "ComfyUI staged a training image under a different name".into(),
        ));
    }
    Ok(())
}

async fn wait_prompt(
    client: &reqwest::Client,
    config: &Config,
    prompt: Uuid,
    timeout: Duration,
) -> Result<(), TrainError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            cancel(client, config, prompt).await;
            return Err(TrainError::Failed("training timed out".into()));
        }
        let history: Value = authorize(
            config,
            client.get(format!("{}/history/{prompt}", config.base_url)),
        )
        .send()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .error_for_status()
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .json()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?;
        if let Some(entry) = exact_history(&history, prompt)?
            && train_prompt_complete(entry)?
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn exact_history(history: &Value, prompt: Uuid) -> Result<Option<&Value>, TrainError> {
    let entries = history
        .as_object()
        .ok_or_else(|| TrainError::Failed("ComfyUI history is not an object".into()))?;
    if entries.is_empty() {
        return Ok(None);
    }
    if entries.len() != 1 {
        return Err(TrainError::Failed(
            "ComfyUI history returned an unexpected prompt".into(),
        ));
    }
    entries
        .get(&prompt.to_string())
        .map(Some)
        .ok_or_else(|| TrainError::Failed("ComfyUI history prompt id changed".into()))
}

fn train_prompt_complete(entry: &Value) -> Result<bool, TrainError> {
    let status: HistoryStatus =
        serde_json::from_value(entry.get("status").cloned().unwrap_or(json!({})))
            .map_err(|_| TrainError::Failed("ComfyUI history status is invalid".into()))?;
    if status.status_str.eq_ignore_ascii_case("error") {
        return Err(TrainError::Failed(format!(
            "ComfyUI train failed: {}",
            entry.get("status").cloned().unwrap_or(json!({}))
        )));
    }
    Ok(status.completed == Some(true) || status.status_str.eq_ignore_ascii_case("success"))
}

async fn cancel(client: &reqwest::Client, config: &Config, prompt: Uuid) {
    let _ = authorize(
        config,
        client.post(format!("{}/api/jobs/{prompt}/cancel", config.base_url)),
    )
    .send()
    .await;
}

pub async fn cleanup(config: &Config, run: &Run) {
    if run.validate().is_err() {
        return;
    }
    cleanup_local(config, run);
    let Ok(client) = client(config) else { return };
    let graph = json!({
        "1": {
            "class_type": "ZoneCleanupTrainingRun",
            "inputs": { "folder": run.folder, "artifact": run.artifact }
        }
    });
    let Ok(prompt) = queue(&client, config, graph).await else {
        return;
    };
    let _ = wait_prompt(&client, config, prompt, Duration::from_secs(30)).await;
}

fn cleanup_local(config: &Config, run: &Run) {
    if let Some(input) = config
        .models_dir
        .parent()
        .map(|parent| parent.join("input"))
    {
        let folder = input.join(&run.folder);
        if !fs::symlink_metadata(&folder).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            let _ = fs::remove_dir_all(folder);
        }
    }
    if let Some(output) = config
        .models_dir
        .parent()
        .map(|parent| parent.join("output/loras"))
    {
        let prefix = format!("{}-step", run.artifact);
        let final_name = format!("{}.safetensors", run.artifact);
        let Ok(entries) = fs::read_dir(output) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if (name == final_name || name.starts_with(&prefix) && name.ends_with(".safetensors"))
                && !entry
                    .file_type()
                    .is_ok_and(|file_type| file_type.is_symlink())
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

pub(crate) fn manifest(
    model: &TrainingModel,
    captions: &HashMap<String, String>,
) -> Option<String> {
    if captions.is_empty() {
        return None;
    }
    let mut entries: Vec<(&String, &String)> = captions.iter().collect();
    entries.sort_by_key(|(name, _)| *name);
    let pairs = entries
        .into_iter()
        .enumerate()
        .map(|(index, (name, instruction))| {
            if name != &format!("{index:04}.png") || instruction.trim().is_empty() {
                return None;
            }
            Some(Pair {
                index,
                target: format!("targets/{name}"),
                reference: matches!(model, TrainingModel::QwenEdit { .. })
                    .then(|| format!("control_1/{name}")),
                instruction: instruction.trim().to_string(),
            })
        })
        .collect::<Option<Vec<Pair>>>()?;
    serde_json::to_string(&Manifest {
        schema_version: MANIFEST_VERSION,
        architecture: architecture(model),
        pairs,
    })
    .ok()
}

pub(crate) fn architecture(model: &TrainingModel) -> &'static str {
    match model {
        TrainingModel::Flux { .. } => "flux",
        TrainingModel::QwenEdit { .. } => "qwen_edit",
    }
}

fn validate_run_name(name: &str, prefix: &str) -> Result<(), TrainError> {
    let Some(id) = name.strip_prefix(prefix) else {
        return Err(TrainError::Invalid("invalid training run namespace"));
    };
    let uuid = Uuid::parse_str(id).map_err(|_| TrainError::Invalid("invalid training run UUID"))?;
    if uuid.get_version_num() != 4 || uuid.to_string() != id {
        return Err(TrainError::Invalid("invalid training run UUID"));
    }
    Ok(())
}

fn authorize(config: &Config, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match &config.api_token {
        Some(token) => request.header("X-Zone-ComfyUI-Token", token),
        None => request,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATASETS: [usize; 4] = [8, 24, 100, 300];
    const HEALTHY_PASSES: std::ops::RangeInclusive<f64> = 17.0..=19.0;

    fn passes(config: &TrainConfig, images: usize) -> f64 {
        f64::from(config.steps(images)) / images as f64
    }

    fn flux() -> TrainingModel {
        TrainingModel::Flux {
            checkpoint: "flux.safetensors".into(),
        }
    }

    fn qwen() -> TrainingModel {
        TrainingModel::QwenEdit {
            unet: "qwen-unet.safetensors".into(),
            clip: "qwen-clip.safetensors".into(),
            vae: "qwen-vae.safetensors".into(),
        }
    }

    #[test]
    fn train_graphs_use_native_encoders_instead_of_an_optional_dataset_node() {
        let settings = packaged_config().unwrap();
        let flux = train_graph(&flux(), "folder", "{}", "artifact", &settings, 12);
        let qwen = train_graph(&qwen(), "folder", "{}", "artifact", &settings, 12);
        for graph in [&flux, &qwen] {
            assert!(graph.to_string().contains("VAEEncode"));
            assert!(graph.to_string().contains("ZoneLoadTrainDataset"));
        }
        assert_eq!(flux["4"]["class_type"], "CLIPTextEncode");
        assert_eq!(qwen["1"]["class_type"], "UNETLoader");
        assert_eq!(qwen["2"]["inputs"]["type"], "qwen_image");
        assert_eq!(qwen["6"]["class_type"], "TextEncodeQwenImageEditPlus");
        assert_eq!(qwen["5"]["inputs"]["pixels"], json!(["4", 0]));
        assert_eq!(qwen["6"]["inputs"]["image1"], json!(["4", 1]));
        assert_eq!(qwen["6"]["inputs"]["prompt"], json!(["4", 2]));
    }

    #[test]
    fn qwen_manifest_keeps_same_index_target_reference_and_instruction() {
        let raw: Value = serde_json::from_str(
            &manifest(
                &qwen(),
                &HashMap::from([("0000.png".into(), "change the colour".into())]),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(raw["architecture"], "qwen_edit");
        assert_eq!(raw["pairs"][0]["target"], "targets/0000.png");
        assert_eq!(raw["pairs"][0]["reference"], "control_1/0000.png");
        assert_eq!(raw["pairs"][0]["instruction"], "change the colour");
    }

    #[test]
    fn run_names_are_server_generated_v4_uuids() {
        let run = Run::new();
        run.validate().unwrap();
        assert_ne!(run, Run::new());
        assert!(
            Run {
                folder: "zone-train-user-name".into(),
                artifact: "zone-lora-user-name".into(),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn an_atomic_download_refuses_to_follow_the_final_symlink() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let root = tempfile::tempdir().unwrap();
            let victim = root.path().join("victim");
            fs::write(&victim, b"safe").unwrap();
            let output = root.path().join("adapter.safetensors");
            symlink(&victim, &output).unwrap();
            assert!(atomic_write(&output, b"replacement").is_err());
            assert_eq!(fs::read(victim).unwrap(), b"safe");
        }
    }

    #[test]
    fn dataset_manifest_refuses_symlinked_images_and_instructions() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let root = tempfile::tempdir().unwrap();
            let targets = root.path().join("targets");
            fs::create_dir(&targets).unwrap();
            let victim = root.path().join("victim.png");
            fs::write(&victim, b"image").unwrap();
            symlink(&victim, targets.join("0000.png")).unwrap();
            fs::write(targets.join("0000.txt"), b"portrait").unwrap();
            assert!(pairs(&flux(), root.path()).is_err());

            fs::remove_file(targets.join("0000.png")).unwrap();
            fs::write(targets.join("0000.png"), b"image").unwrap();
            fs::remove_file(targets.join("0000.txt")).unwrap();
            symlink(&victim, targets.join("0000.txt")).unwrap();
            assert!(pairs(&flux(), root.path()).is_err());
        }
    }

    #[test]
    fn a_larger_dataset_is_never_trained_less_per_image_than_a_smaller_one() {
        let config = packaged_config().unwrap();
        for sizes in DATASETS.windows(2) {
            assert!(passes(&config, sizes[1]) >= passes(&config, sizes[0]));
        }
    }

    #[test]
    fn every_realistic_dataset_trains_inside_the_measured_band() {
        let config = packaged_config().unwrap();
        for images in DATASETS {
            assert!(HEALTHY_PASSES.contains(&passes(&config, images)));
        }
    }

    #[test]
    fn the_floor_and_ceiling_bound_training() {
        let config = packaged_config().unwrap();
        assert!(config.min_steps <= config.max_steps);
        assert_eq!(config.steps(1), config.min_steps);
        assert_eq!(config.steps(usize::MAX), config.max_steps);
    }

    #[test]
    fn packaged_config_tracks_the_shipped_json() {
        let config = packaged_config().unwrap();
        let raw: Value = serde_json::from_str(PACKAGED_TRAIN_CONFIG).unwrap();
        assert_eq!(raw["passes_per_image"], config.passes_per_image);
        assert_eq!(raw["min_steps"], config.min_steps);
        assert_eq!(raw["max_steps"], config.max_steps);
        assert_eq!(raw["rank"], config.rank);
        assert_eq!(raw["learning_rate"], config.learning_rate);
        assert_eq!(raw["lora_dtype"], config.lora_dtype);
        assert_eq!(raw["training_dtype"], config.training_dtype);
        assert_eq!(raw["resolution"], config.resolution);
        assert_eq!(raw["bypass_mode"], config.bypass_mode);
        assert_eq!(raw["gradient_checkpointing"], config.gradient_checkpointing);
        assert_eq!(raw["checkpoint_depth"], config.checkpoint_depth);
        assert_eq!(raw["seed"], config.seed);
    }

    #[test]
    fn prompt_history_must_contain_only_the_requested_uuid() {
        let prompt = Uuid::new_v4();
        assert!(exact_history(&json!({}), prompt).unwrap().is_none());
        assert!(
            exact_history(&json!({ prompt.to_string(): {"status": {}} }), prompt)
                .unwrap()
                .is_some()
        );
        assert!(exact_history(&json!({ Uuid::new_v4().to_string(): {} }), prompt).is_err());
    }

    #[test]
    fn train_prompt_waits_until_status_completes() {
        assert!(!train_prompt_complete(&json!({"status": {"status_str": "running"}})).unwrap());
        assert!(
            train_prompt_complete(&json!({"status": {"completed": true, "status_str": "success"}}))
                .unwrap()
        );
        assert!(train_prompt_complete(&json!({"status": {"status_str": "error"}})).is_err());
    }
}
