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
    /// Side of the square every training image is read back at.
    pub fn resolution(&self) -> u32 {
        self.resolution
    }

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
    use crate::recipe::RecipeCatalog;
    use wiremock::matchers::{method, path as path_matcher, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A dataset on disk, as `lora::train` leaves it for the graph runner.
    fn dataset() -> tempfile::TempDir {
        let work = tempfile::tempdir().unwrap();
        let targets = work.path().join("targets");
        fs::create_dir_all(&targets).unwrap();
        let mut png = Vec::new();
        {
            use image::ImageEncoder;
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&[10, 20, 30], 1, 1, image::ExtendedColorType::Rgb8)
                .unwrap();
        }
        fs::write(targets.join("0000.png"), &png).unwrap();
        fs::write(targets.join("0000.txt"), "ohwx, a portrait").unwrap();
        fs::write(targets.join("0001.png"), &png).unwrap();
        work
    }

    fn config(server: &MockServer) -> Config {
        Config {
            enabled: true,
            base_url: server.uri(),
            api_token: Some("secret".into()),
            train_timeout_secs: 60,
            poll_interval_ms: 50,
            // No sibling input/ directory, so staging falls through to upload.
            models_dir: std::env::temp_dir().join(format!("zone-models-{}", Uuid::new_v4())),
            ..Default::default()
        }
    }

    fn recipe() -> Recipe {
        RecipeCatalog::packaged()
            .unwrap()
            .get("flux-schnell")
            .expect("the packaged catalog ships flux-schnell")
            .clone()
    }

    async fn queues(server: &MockServer, body: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path_matcher("/prompt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn uploads(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path_matcher("/upload/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "0000.png"})))
            .mount(server)
            .await;
    }

    async fn finishes(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path_matcher("/history/p-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"p-1": {"status": {"completed": true, "status_str": "success"}}}),
            ))
            .mount(server)
            .await;
    }

    async fn serves(server: &MockServer, weights: Vec<u8>) {
        Mock::given(method("GET"))
            .and(path_matcher("/view"))
            .and(query_param("subfolder", "loras"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(weights))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_finished_graph_writes_the_weights_comfyui_produced() {
        let server = MockServer::start().await;
        uploads(&server).await;
        queues(&server, json!({"prompt_id": "p-1"})).await;
        finishes(&server).await;
        serves(&server, vec![7u8; 20_000]).await;

        let work = dataset();
        let output = work.path().join("my-style.safetensors");
        run(
            &config(&server),
            &recipe(),
            work.path(),
            &output,
            "my-style.safetensors",
            2,
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&output).unwrap(), vec![7u8; 20_000]);

        let posted = server.received_requests().await.unwrap();
        let graph = posted
            .iter()
            .find(|request| request.url.path() == "/prompt")
            .expect("the graph was never submitted");
        assert_eq!(
            graph.headers.get("X-Zone-ComfyUI-Token").unwrap(),
            "secret",
            "a token-protected ComfyUI has to be told who is asking"
        );
        let body: Value = serde_json::from_slice(&graph.body).unwrap();
        let inputs = &body["prompt"]["4"]["inputs"];
        assert_eq!(inputs["steps"], 400, "two images clamp up to min_steps");
        assert_eq!(inputs["save_name"], "my-style");
        assert_eq!(
            body["prompt"]["1"]["inputs"]["ckpt_name"],
            recipe().defaults["checkpoint"]
        );
        assert_eq!(
            body["prompt"]["2"]["inputs"]["resolution"],
            packaged_config().unwrap().resolution()
        );
        let captions: HashMap<String, String> = serde_json::from_str(
            body["prompt"]["2"]["inputs"]["captions_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(captions["0000.png"], "ohwx, a portrait");
        assert!(
            !captions.contains_key("0001.png"),
            "an image with no .txt beside it carries no caption"
        );
    }

    #[tokio::test]
    async fn a_graph_comfyui_will_not_accept_fails_the_job() {
        let server = MockServer::start().await;
        uploads(&server).await;
        queues(
            &server,
            json!({"prompt_id": "", "error": {"type": "prompt_outputs_failed_validation"}}),
        )
        .await;

        let work = dataset();
        let error = run(
            &config(&server),
            &recipe(),
            work.path(),
            &work.path().join("out.safetensors"),
            "out",
            2,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, TrainError::Failed(message) if message.contains("rejected")),
            "{error}"
        );
    }

    #[tokio::test]
    async fn weights_too_small_to_be_trained_are_not_accepted() {
        let server = MockServer::start().await;
        uploads(&server).await;
        queues(&server, json!({"prompt_id": "p-1"})).await;
        finishes(&server).await;
        // ComfyUI serves its error pages with a 200, so size is the only tell.
        serves(&server, b"<html>not found</html>".to_vec()).await;

        let work = dataset();
        let output = work.path().join("out.safetensors");
        let error = run(&config(&server), &recipe(), work.path(), &output, "out", 2)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, TrainError::Failed(message) if message.contains("too small")),
            "{error}"
        );
        assert!(
            !output.exists(),
            "a rejected download leaves nothing behind"
        );
    }

    #[tokio::test]
    async fn a_graph_that_never_finishes_times_out() {
        let server = MockServer::start().await;
        uploads(&server).await;
        queues(&server, json!({"prompt_id": "p-1"})).await;
        Mock::given(method("GET"))
            .and(path_matcher("/history/p-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"p-1": {"status": {"status_str": "running"}}})),
            )
            .mount(&server)
            .await;

        let work = dataset();
        let client = reqwest::Client::new();
        let error = wait_prompt(&client, &config(&server), "p-1", Duration::from_millis(120))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, TrainError::Failed(message) if message.contains("timed out")),
            "{error}"
        );
        drop(work);
    }

    #[tokio::test]
    async fn a_graph_that_errors_is_reported_rather_than_polled_forever() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_matcher("/history/p-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"p-1": {"status": {"status_str": "error"}}})),
            )
            .mount(&server)
            .await;

        let error = wait_prompt(
            &reqwest::Client::new(),
            &config(&server),
            "p-1",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, TrainError::Failed(message) if message.contains("train failed")),
            "{error}"
        );
    }

    #[tokio::test]
    async fn training_against_a_disabled_comfyui_does_not_reach_the_network() {
        let work = dataset();
        let error = run(
            &Config::default(),
            &recipe(),
            work.path(),
            &work.path().join("out.safetensors"),
            "out",
            2,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, TrainError::Disabled), "{error}");
    }

    #[tokio::test]
    async fn a_dataset_beside_comfyui_is_staged_rather_than_uploaded() {
        let server = MockServer::start().await;
        queues(&server, json!({"prompt_id": "p-1"})).await;
        finishes(&server).await;
        serves(&server, vec![7u8; 20_000]).await;

        // models_dir with a sibling input/ is the shared-volume deployment,
        // where the dataset can simply be copied into place.
        let comfy = tempfile::tempdir().unwrap();
        let input = comfy.path().join("input");
        fs::create_dir_all(&input).unwrap();
        let mut settings = config(&server);
        settings.models_dir = comfy.path().join("models");
        fs::create_dir_all(&settings.models_dir).unwrap();

        let work = dataset();
        run(
            &settings,
            &recipe(),
            work.path(),
            &work.path().join("out.safetensors"),
            "out",
            2,
        )
        .await
        .unwrap();

        let staged: Vec<PathBuf> = fs::read_dir(&input)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .collect();
        assert_eq!(staged.len(), 1, "one folder per training run");
        assert!(staged[0].join("0000.png").is_file());
        assert!(staged[0].join("0000.txt").is_file());
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path() == "/upload/image"),
            "a staged dataset must not also be uploaded"
        );
    }

    #[test]
    fn identity_config_trains_long_enough_for_eight_images() {
        let config: TrainConfig = packaged_config().unwrap();
        assert!(
            config.min_steps >= 400,
            "identity training needs at least 400 steps, and clamping to a lower floor would pass every other assertion here"
        );
        assert_eq!(config.steps(1), config.min_steps, "a tiny set still trains");
        assert_eq!(
            config.steps(10_000),
            config.max_steps,
            "a huge set is capped"
        );
    }

    #[test]
    fn packaged_config_tracks_the_shipped_json() {
        let config: TrainConfig = packaged_config().unwrap();
        let raw: Value = serde_json::from_str(PACKAGED_TRAIN_CONFIG).unwrap();
        assert_eq!(raw["steps_per_image"], config.steps_per_image);
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
    fn train_graph_sends_every_value_from_the_packaged_config() {
        let settings: TrainConfig = packaged_config().unwrap();
        let images: usize = 12;
        let steps: u32 = settings.steps(images);
        assert!(
            steps > settings.min_steps && steps < settings.max_steps,
            "pick a dataset size between the step bounds, or a hardcoded step count passes unnoticed"
        );
        let graph: Value = train_graph(
            "base.safetensors",
            "zone-train-inputs",
            &HashMap::new(),
            "identity",
            &settings,
            steps,
        );

        let loader: &Value = &graph["2"]["inputs"];
        assert_eq!(loader["resolution"], settings.resolution);

        let trainer: &Value = &graph["4"]["inputs"];
        assert_eq!(trainer["steps"], steps);
        assert_eq!(trainer["rank"], settings.rank);
        assert_eq!(trainer["learning_rate"], settings.learning_rate);
        assert_eq!(trainer["seed"], settings.seed);
        assert_eq!(trainer["training_dtype"], settings.training_dtype);
        assert_eq!(trainer["lora_dtype"], settings.lora_dtype);
        assert_eq!(
            trainer["gradient_checkpointing"],
            settings.gradient_checkpointing
        );
        assert_eq!(trainer["checkpoint_depth"], settings.checkpoint_depth);
        assert_eq!(trainer["bypass_mode"], settings.bypass_mode);
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
