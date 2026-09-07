//! HTTP client that runs packaged ZoneTrainLoRA graphs on ComfyUI.

use crate::config::Config;
use crate::lora::TrainError;
use crate::recipe::Recipe;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const PACKAGED_TRAIN_CONFIG: &str =
    include_str!("../../../comfyui/custom_nodes/zone_lora/train_config.json");

#[derive(Debug, Deserialize)]
pub struct TrainConfig {
    steps_per_image: u32,
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

#[derive(Debug, Deserialize)]
struct PromptResponse {
    prompt_id: String,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct HistoryStatus {
    #[serde(default)]
    status_str: String,
    #[serde(default)]
    completed: Option<bool>,
}

/// The packaged identity-training defaults.
pub fn packaged_config() -> Result<TrainConfig, TrainError> {
    serde_json::from_str(PACKAGED_TRAIN_CONFIG)
        .map_err(|error| TrainError::Failed(format!("train config: {error}")))
}

impl TrainConfig {
    /// Steps for a dataset of this size, clamped to the configured bounds.
    pub fn steps(&self, image_count: usize) -> u32 {
        (image_count.max(1) as u32 * self.steps_per_image).clamp(self.min_steps, self.max_steps)
    }
}

pub async fn run(
    config: &Config,
    recipe: &Recipe,
    work: &Path,
    output: &Path,
    save_name: &str,
    image_count: usize,
) -> Result<(), TrainError> {
    if !config.enabled {
        return Err(TrainError::Disabled);
    }
    let settings = packaged_config()?;
    let checkpoint = recipe
        .defaults
        .get("checkpoint")
        .cloned()
        .unwrap_or_else(|| config.checkpoint.clone());
    let folder = format!("zone-train-{}", Uuid::new_v4());
    let captions = stage_or_upload(config, work, &folder).await?;
    let steps = settings.steps(image_count);
    let graph = train_graph(
        &checkpoint,
        &folder,
        &captions,
        save_name.trim_end_matches(".safetensors"),
        &settings,
        steps,
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.train_timeout_secs))
        .build()
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    let mut request = client.post(format!("{}/prompt", config.base_url));
    if let Some(token) = &config.api_token {
        request = request.header("X-Zone-ComfyUI-Token", token);
    }
    let queued = request
        .json(&json!({ "prompt": graph }))
        .send()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .error_for_status()
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .json::<PromptResponse>()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    if let Some(error) = queued.error {
        return Err(TrainError::Failed(format!(
            "ComfyUI rejected train graph: {error}"
        )));
    }
    wait_prompt(
        &client,
        config,
        &queued.prompt_id,
        Duration::from_secs(config.train_timeout_secs),
    )
    .await?;
    let filename = format!("{}.safetensors", save_name.trim_end_matches(".safetensors"));
    let mut view = client.get(format!(
        "{}/view?filename={}&subfolder=loras&type=output",
        config.base_url,
        urlencoding::encode(&filename)
    ));
    if let Some(token) = &config.api_token {
        view = view.header("X-Zone-ComfyUI-Token", token);
    }
    let bytes = view
        .send()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .error_for_status()
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .bytes()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    if bytes.len() < 10_000 {
        return Err(TrainError::Failed(
            "ComfyUI returned a LoRA that is too small to be trained weights".into(),
        ));
    }
    fs::write(output, bytes).map_err(|error| TrainError::Failed(error.to_string()))?;
    Ok(())
}

fn train_graph(
    checkpoint: &str,
    folder: &str,
    captions: &HashMap<String, String>,
    save_name: &str,
    settings: &TrainConfig,
    steps: u32,
) -> Value {
    json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": { "ckpt_name": checkpoint }
        },
        "2": {
            "class_type": "ZoneLoadTrainFolder",
            "inputs": {
                "folder": folder,
                "captions_json": serde_json::to_string(captions).unwrap_or_else(|_| "{}".into()),
                "resolution": settings.resolution
            }
        },
        "3": {
            "class_type": "MakeTrainingDataset",
            "inputs": {
                "images": ["2", 0],
                "texts": ["2", 1],
                "vae": ["1", 2],
                "clip": ["1", 1]
            }
        },
        "4": {
            "class_type": "ZoneTrainLoRA",
            "inputs": {
                "model": ["1", 0],
                "latents": ["3", 0],
                "positive": ["3", 1],
                "steps": steps,
                "learning_rate": settings.learning_rate,
                "rank": settings.rank,
                "seed": settings.seed,
                "training_dtype": settings.training_dtype,
                "lora_dtype": settings.lora_dtype,
                "gradient_checkpointing": settings.gradient_checkpointing,
                "checkpoint_depth": settings.checkpoint_depth,
                "bypass_mode": settings.bypass_mode,
                "save_name": save_name
            }
        }
    })
}

async fn stage_or_upload(
    config: &Config,
    work: &Path,
    folder: &str,
) -> Result<HashMap<String, String>, TrainError> {
    let targets = work.join("targets");
    let mut captions = HashMap::new();
    let input_root = config
        .models_dir
        .parent()
        .map(|parent| parent.join("input"));
    if let Some(input_root) = input_root.filter(|path| path.is_dir()) {
        let destination = input_root.join(folder);
        fs::create_dir_all(&destination).map_err(|error| TrainError::Failed(error.to_string()))?;
        let mut copied = 0;
        for png in pngs(&targets)? {
            let name = png
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or(TrainError::Invalid("training image name is invalid"))?;
            fs::copy(&png, destination.join(name))
                .map_err(|error| TrainError::Failed(error.to_string()))?;
            copied += 1;
            let txt = png.with_extension("txt");
            if txt.is_file() {
                fs::copy(&txt, destination.join(txt.file_name().unwrap()))
                    .map_err(|error| TrainError::Failed(error.to_string()))?;
                captions.insert(
                    name.to_string(),
                    fs::read_to_string(&txt)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                );
            }
        }
        if copied > 0 {
            return Ok(captions);
        }
    }
    for png in pngs(&targets)? {
        let name = png
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(TrainError::Invalid("training image name is invalid"))?
            .to_string();
        upload_png(
            config,
            folder,
            &name,
            &fs::read(&png).map_err(|error| TrainError::Failed(error.to_string()))?,
        )
        .await?;
        let txt = png.with_extension("txt");
        if txt.is_file() {
            captions.insert(
                name,
                fs::read_to_string(&txt)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            );
        }
    }
    Ok(captions)
}

fn pngs(targets: &Path) -> Result<Vec<PathBuf>, TrainError> {
    let mut files: Vec<PathBuf> = fs::read_dir(targets)
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("png"))
        .collect();
    files.sort();
    Ok(files)
}

async fn upload_png(
    config: &Config,
    folder: &str,
    filename: &str,
    bytes: &[u8],
) -> Result<(), TrainError> {
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str("image/png")
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .part("image", part)
        .text("overwrite", "true")
        .text("type", "input")
        .text("subfolder", folder.to_string());
    let mut request = reqwest::Client::new()
        .post(format!("{}/upload/image", config.base_url))
        .multipart(form);
    if let Some(token) = &config.api_token {
        request = request.header("X-Zone-ComfyUI-Token", token);
    }
    request
        .send()
        .await
        .map_err(|error| TrainError::Failed(error.to_string()))?
        .error_for_status()
        .map_err(|error| TrainError::Failed(error.to_string()))?;
    Ok(())
}

async fn wait_prompt(
    client: &reqwest::Client,
    config: &Config,
    prompt_id: &str,
    timeout: Duration,
) -> Result<(), TrainError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(TrainError::Failed("training timed out".into()));
        }
        let mut request = client.get(format!("{}/history/{prompt_id}", config.base_url));
        if let Some(token) = &config.api_token {
            request = request.header("X-Zone-ComfyUI-Token", token);
        }
        let history: Value = request
            .send()
            .await
            .map_err(|error| TrainError::Failed(error.to_string()))?
            .error_for_status()
            .map_err(|error| TrainError::Failed(error.to_string()))?
            .json()
            .await
            .map_err(|error| TrainError::Failed(error.to_string()))?;
        if let Some(entry) = history.get(prompt_id)
            && train_prompt_complete(entry)?
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn train_prompt_complete(entry: &Value) -> Result<bool, TrainError> {
    let status: HistoryStatus = serde_json::from_value(
        entry.get("status").cloned().unwrap_or(json!({})),
    )
    .unwrap_or(HistoryStatus {
        status_str: String::new(),
        completed: None,
    });
    if status.status_str.eq_ignore_ascii_case("error") {
        return Err(TrainError::Failed(format!(
            "ComfyUI train failed: {}",
            entry.get("status").cloned().unwrap_or(json!({}))
        )));
    }
    Ok(status.completed == Some(true) || status.status_str.eq_ignore_ascii_case("success"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_config_trains_long_enough_for_eight_images() {
        let config = packaged_config().unwrap();
        assert_eq!(config.steps(8), 400);
        assert_eq!(config.steps(1), config.min_steps, "a tiny set still trains");
        assert_eq!(
            config.steps(10_000),
            config.max_steps,
            "a huge set is capped"
        );
        assert_eq!(config.rank, 8);
        assert_eq!(config.resolution, 512);
        assert_eq!(config.min_steps, 400);
        assert_eq!(config.steps_per_image, 50);
    }

    /// Keys the packaged Python node reads. Rust never touches them, so only a
    /// test keeps the two sides of the file in step.
    #[test]
    fn packaged_config_keeps_the_keys_the_train_node_reads() {
        let raw: Value = serde_json::from_str(PACKAGED_TRAIN_CONFIG).unwrap();
        assert_eq!(raw["alpha_equals_rank"], true);
        assert_eq!(raw["train_modulation"], false);
        assert!(raw["min_adapters"].as_u64().is_some());
        assert!(raw["batch_size"].as_u64().is_some());
    }

    #[test]
    fn train_prompt_waits_until_status_completes() {
        let running = json!({"status": {"status_str": "running"}, "outputs": {"3": {}}});
        assert!(!train_prompt_complete(&running).unwrap());
        let done = json!({"status": {"completed": true, "status_str": "success"}});
        assert!(train_prompt_complete(&done).unwrap());
        let failed = json!({"status": {"status_str": "error"}});
        assert!(train_prompt_complete(&failed).is_err());
    }
}
