//! Direct ComfyUI API client. Graphs come from packaged recipes; chat only
//! supplies prompt, seed, checkpoint filename, and an optional source image.

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use super::comfy_recipe::{Fill, RecipeCatalog, sanitize_upload_name, sanitize_weight_filename};
use crate::config::ComfyUiConfig;

pub const MAX_SOURCE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const PACKAGED_VIDEO_WORKFLOW: &str =
    include_str!("../../../../comfyui/workflows/wan2.2-ti2v-5b-api.json");
const PACKAGED_I2V_WORKFLOW: &str =
    include_str!("../../../../comfyui/workflows/wan2.2-ti2v-5b-i2v-api.json");

#[derive(Debug, thiserror::Error)]
pub enum ComfyUiError {
    #[error("ComfyUI is disabled")]
    Disabled,
    #[error("invalid ComfyUI configuration: {0}")]
    Configuration(&'static str),
    #[error("ComfyUI request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("ComfyUI returned an invalid response: {0}")]
    InvalidResponse(&'static str),
    #[error("generation timed out")]
    Timeout,
    #[error("image generation cancelled")]
    Cancelled,
}

#[derive(Debug)]
pub struct GeneratedImage {
    pub bytes: bytes::Bytes,
    pub mime: String,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct SourceImage {
    pub bytes: bytes::Bytes,
    pub mime: String,
    pub filename: String,
}

impl SourceImage {
    pub fn new(bytes: impl Into<bytes::Bytes>, mime: &str) -> Result<Self, ComfyUiError> {
        let mime = normalize_source_mime(mime)?;
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_SOURCE_IMAGE_BYTES {
            return Err(ComfyUiError::Configuration(
                "source image is empty or too large",
            ));
        }
        Ok(Self {
            filename: format!(
                "zone-img2img-{}.{}",
                Uuid::new_v4(),
                extension_for_mime(&mime)
            ),
            bytes,
            mime,
        })
    }
}

#[derive(Clone)]
pub struct ComfyUiClient {
    config: ComfyUiConfig,
    client: Client,
    catalog: RecipeCatalog,
}

#[derive(Deserialize)]
struct PromptResponse {
    prompt_id: String,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    name: String,
    #[serde(default)]
    subfolder: String,
}

#[derive(Debug, Deserialize)]
struct OutputImage {
    filename: String,
    #[serde(default)]
    subfolder: String,
    #[serde(default = "default_output_type")]
    r#type: String,
}

fn default_output_type() -> String {
    "output".to_string()
}

#[derive(Clone, Copy)]
enum OutputMode {
    Image,
    Video,
}

fn collect_output_files(node: &Value, mode: OutputMode) -> Result<Vec<OutputImage>, ComfyUiError> {
    let keys = match mode {
        OutputMode::Image => &["images"][..],
        OutputMode::Video => &["videos", "gifs", "images"][..],
    };
    let mut files = Vec::new();
    for key in keys {
        let Some(items) = node.get(*key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let file: OutputImage = serde_json::from_value(item.clone())
                .map_err(|_| ComfyUiError::InvalidResponse("invalid media output"))?;
            match mode {
                OutputMode::Image => {
                    if file.r#type != "temp" {
                        return Err(ComfyUiError::InvalidResponse(
                            "workflow returned a non-temporary image",
                        ));
                    }
                    files.push(file);
                }
                OutputMode::Video => {
                    if !is_video_filename(&file.filename) {
                        continue;
                    }
                    if file.r#type != "temp" && file.r#type != "output" {
                        return Err(ComfyUiError::InvalidResponse(
                            "workflow returned an unsupported video location",
                        ));
                    }
                    files.push(file);
                }
            }
        }
    }
    Ok(files)
}

fn outputs_from_history_entry(
    status: &str,
    nodes: Option<&serde_json::Map<String, Value>>,
    mode: OutputMode,
) -> Result<Option<Vec<OutputImage>>, ComfyUiError> {
    if status == "error" {
        return Err(ComfyUiError::InvalidResponse("workflow execution failed"));
    }
    let mut files = Vec::new();
    if let Some(nodes) = nodes {
        for node in nodes.values() {
            files.extend(collect_output_files(node, mode)?);
        }
    }
    if files.is_empty() {
        if status == "success" {
            return Err(ComfyUiError::InvalidResponse(
                "workflow completed without a usable output",
            ));
        }
        return Ok(None);
    }
    Ok(Some(files))
}

impl ComfyUiClient {
    pub fn new(config: ComfyUiConfig) -> Result<Self, ComfyUiError> {
        if config.base_url.trim().is_empty() {
            return Err(ComfyUiError::Configuration("COMFYUI_BASE_URL is empty"));
        }
        sanitize_weight_filename(&config.checkpoint).map_err(|_| {
            ComfyUiError::Configuration("COMFYUI_CHECKPOINT must be a checkpoint filename")
        })?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let catalog = RecipeCatalog::load(Some(config.workflow_path.as_path()))?;
        let _ = catalog.image_recipe_for(&config.checkpoint)?;
        Ok(Self {
            config,
            client,
            catalog,
        })
    }

    fn video_workflows(&self) -> Result<(Value, Value), ComfyUiError> {
        for (value, message) in [
            (
                self.config.video_unet.as_str(),
                "COMFYUI_VIDEO_UNET must be a diffusion model filename",
            ),
            (
                self.config.video_clip.as_str(),
                "COMFYUI_VIDEO_CLIP must be a text encoder filename",
            ),
            (
                self.config.video_vae.as_str(),
                "COMFYUI_VIDEO_VAE must be a VAE filename",
            ),
        ] {
            if !is_model_filename(value) {
                return Err(ComfyUiError::Configuration(message));
            }
        }
        let video_workflow = load_video_workflow(&self.config.video_workflow_path)?;
        validate_video_workflow(&video_workflow)?;
        let i2v_workflow = load_i2v_workflow(&self.config.video_workflow_path)?;
        validate_i2v_workflow(&i2v_workflow)?;
        Ok((video_workflow, i2v_workflow))
    }

    pub async fn generate(
        &self,
        prompt: &str,
        source: Option<&SourceImage>,
        cancel: &mut broadcast::Receiver<()>,
        progress: mpsc::UnboundedSender<String>,
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        if !self.config.enabled {
            return Err(ComfyUiError::Disabled);
        }

        if cancel.try_recv().is_ok() {
            return Err(ComfyUiError::Cancelled);
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.config.generation_timeout_secs);
        let prompt = if prompt.trim().is_empty() {
            if source.is_some() {
                "edit this image"
            } else {
                return Err(ComfyUiError::Configuration("prompt is empty or too long"));
            }
        } else {
            prompt
        };
        let recipe = self.catalog.image_recipe_for(&self.config.checkpoint)?;
        let workflow = if let Some(source) = source {
            let _ = progress.send("Uploading source image...".to_string());
            let uploaded = self.upload_source(source, cancel, deadline).await?;
            recipe.apply(Fill {
                prompt,
                seed: rand::random::<u64>() & i64::MAX as u64,
                weights: HashMap::from([("checkpoint", self.config.checkpoint.as_str())]),
                source: Some(uploaded.as_str()),
            })?
        } else {
            recipe.apply(Fill {
                prompt,
                seed: rand::random::<u64>() & i64::MAX as u64,
                weights: HashMap::from([("checkpoint", self.config.checkpoint.as_str())]),
                source: None,
            })?
        };
        self.submit_and_collect(
            workflow,
            cancel,
            deadline,
            progress,
            "Image queued...",
            "Generating image...",
            "Saving generated image...",
            OutputMode::Image,
        )
        .await
    }

    pub async fn generate_video(
        &self,
        prompt: &str,
        source: Option<&SourceImage>,
        cancel: &mut broadcast::Receiver<()>,
        progress: mpsc::UnboundedSender<String>,
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        if !self.config.enabled {
            return Err(ComfyUiError::Disabled);
        }

        if cancel.try_recv().is_ok() {
            return Err(ComfyUiError::Cancelled);
        }
        let (video_workflow, i2v_workflow) = self.video_workflows()?;
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(self.config.video_generation_timeout_secs);
        let prompt = if prompt.trim().is_empty() {
            if source.is_some() {
                "animate this image"
            } else {
                return Err(ComfyUiError::Configuration("prompt is empty or too long"));
            }
        } else {
            prompt
        };
        let workflow = if let Some(source) = source {
            let _ = progress.send("Uploading source image...".to_string());
            let uploaded = self.upload_source(source, cancel, deadline).await?;
            configure_wan_i2v_workflow(
                i2v_workflow,
                prompt,
                &self.config.video_unet,
                &self.config.video_clip,
                &self.config.video_vae,
                rand::random::<u64>() & i64::MAX as u64,
                &uploaded,
            )?
        } else {
            configure_wan_t2v_workflow(
                video_workflow,
                prompt,
                &self.config.video_unet,
                &self.config.video_clip,
                &self.config.video_vae,
                rand::random::<u64>() & i64::MAX as u64,
            )?
        };
        self.submit_and_collect(
            workflow,
            cancel,
            deadline,
            progress,
            "Video queued...",
            "Generating video...",
            "Saving generated video...",
            OutputMode::Video,
        )
        .await
    }

    async fn submit_and_collect(
        &self,
        workflow: Value,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
        progress: mpsc::UnboundedSender<String>,
        queued: &str,
        generating: &str,
        saving: &str,
        mode: OutputMode,
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        let request = self
            .authorize(self.client.post(format!("{}/prompt", self.config.base_url)))
            .json(&json!({
                "prompt": workflow,
                "client_id": Uuid::new_v4().to_string()
            }));
        let response = self
            .bounded(cancel, deadline, async move {
                request
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<PromptResponse>()
                    .await
            })
            .await?;
        let prompt_id = response.prompt_id;
        let _ = progress.send(queued.to_string());

        let mut announced_generation = false;
        loop {
            tokio::select! {
                biased;
                _ = cancel.recv() => {
                    self.cancel(&prompt_id).await;
                    return Err(ComfyUiError::Cancelled);
                }
                _ = tokio::time::sleep_until(deadline) => {
                    self.cancel(&prompt_id).await;
                    return Err(ComfyUiError::Timeout);
                }
                _ = tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms)) => {
                    if !announced_generation {
                        let _ = progress.send(generating.to_string());
                        announced_generation = true;
                    }
                    match self.history_outputs(&prompt_id, cancel, deadline, mode).await {
                        Ok(Some(outputs)) => {
                            let _ = progress.send(saving.to_string());
                            let result = self.fetch_outputs(outputs, cancel, deadline).await;
                            self.clear_history(&prompt_id).await;
                            if matches!(result, Err(ComfyUiError::Cancelled | ComfyUiError::Timeout)) {
                                self.cancel(&prompt_id).await;
                            }
                            return result;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if matches!(error, ComfyUiError::Cancelled | ComfyUiError::Timeout) {
                                self.cancel(&prompt_id).await;
                            }
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    async fn upload_source(
        &self,
        source: &SourceImage,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
    ) -> Result<String, ComfyUiError> {
        let filename = sanitize_upload_name(&source.filename)?;
        let part = reqwest::multipart::Part::bytes(source.bytes.to_vec())
            .file_name(filename.clone())
            .mime_str(&source.mime)
            .map_err(|_| ComfyUiError::Configuration("source image type is not supported"))?;
        let form = reqwest::multipart::Form::new()
            .part("image", part)
            .text("overwrite", "true")
            .text("type", "input");
        let request = self.authorize(
            self.client
                .post(format!("{}/upload/image", self.config.base_url))
                .multipart(form),
        );
        let uploaded = self
            .bounded(cancel, deadline, async move {
                request
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<UploadResponse>()
                    .await
            })
            .await?;
        uploaded_image_name(&uploaded, &filename)
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.config.api_token {
            Some(token) => request.header("X-Zone-ComfyUI-Token", token),
            None => request,
        }
    }

    async fn bounded<T, F>(
        &self,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
        request: F,
    ) -> Result<T, ComfyUiError>
    where
        F: Future<Output = Result<T, reqwest::Error>>,
    {
        tokio::select! {
            biased;
            _ = cancel.recv() => Err(ComfyUiError::Cancelled),
            _ = tokio::time::sleep_until(deadline) => Err(ComfyUiError::Timeout),
            result = request => result.map_err(ComfyUiError::Http),
        }
    }

    async fn history_outputs(
        &self,
        prompt_id: &str,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
        mode: OutputMode,
    ) -> Result<Option<Vec<OutputImage>>, ComfyUiError> {
        let request = self.authorize(
            self.client
                .get(format!("{}/history/{}", self.config.base_url, prompt_id)),
        );
        let history = self
            .bounded(cancel, deadline, async move {
                request
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<Value>()
                    .await
            })
            .await?;
        let Some(entry) = history.get(prompt_id) else {
            return Ok(None);
        };
        let status = entry
            .pointer("/status/status_str")
            .and_then(Value::as_str)
            .unwrap_or_default();
        outputs_from_history_entry(
            status,
            entry.get("outputs").and_then(Value::as_object),
            mode,
        )
    }

    async fn fetch_outputs(
        &self,
        outputs: Vec<OutputImage>,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        let mut generated = Vec::with_capacity(outputs.len());
        for output in outputs {
            let filename = output.filename.clone();
            // reqwest 0.13 dropped RequestBuilder::query; encode onto the URL.
            let url = format!(
                "{}/view?filename={}&subfolder={}&type={}",
                self.config.base_url,
                urlencoding::encode(output.filename.as_str()),
                urlencoding::encode(output.subfolder.as_str()),
                urlencoding::encode(output.r#type.as_str()),
            );
            let request = self.authorize(self.client.get(url));
            let (bytes, mime) = self
                .bounded(cancel, deadline, async move {
                    let response = request.send().await?.error_for_status()?;
                    let mime = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.split(';').next())
                        .map(str::trim)
                        .filter(|value| value.starts_with("image/") || value.starts_with("video/"))
                        .map(str::to_string)
                        .unwrap_or_else(|| mime_for_filename(&filename));
                    Ok((response.bytes().await?, mime))
                })
                .await?;
            generated.push(GeneratedImage {
                bytes,
                mime,
                filename: output.filename,
            });
        }
        Ok(generated)
    }

    async fn cancel(&self, prompt_id: &str) {
        if let Err(error) = self
            .authorize(self.client.post(format!("{}/queue", self.config.base_url)))
            .json(&json!({ "delete": [prompt_id] }))
            .send()
            .await
        {
            tracing::warn!("Failed to cancel ComfyUI prompt {}: {}", prompt_id, error);
        }
        // `/interrupt` is process-wide in ComfyUI and cannot safely identify a
        // prompt. Never call it: removing queued work is safe, while an already
        // running cancelled job finishes into ComfyUI's temporary directory.
    }

    async fn clear_history(&self, prompt_id: &str) {
        if let Err(error) = self
            .authorize(
                self.client
                    .post(format!("{}/history", self.config.base_url)),
            )
            .json(&json!({ "delete": [prompt_id] }))
            .send()
            .await
        {
            tracing::warn!("Failed to clear ComfyUI history {}: {}", prompt_id, error);
        }
    }
}

/// Build the default image recipe and mutate only prompt, checkpoint, and seed.
pub fn build_flux_schnell_workflow(
    prompt: &str,
    checkpoint: &str,
    seed: u64,
) -> Result<Value, ComfyUiError> {
    RecipeCatalog::packaged()?
        .image_recipe_for("flux1-schnell-fp8.safetensors")?
        .apply(Fill {
            prompt,
            seed,
            weights: HashMap::from([("checkpoint", checkpoint)]),
            source: None,
        })
}

/// Build the default image-to-image recipe and mutate only approved inputs.
pub fn build_flux_schnell_img2img_workflow(
    prompt: &str,
    checkpoint: &str,
    seed: u64,
    image_name: &str,
) -> Result<Value, ComfyUiError> {
    RecipeCatalog::packaged()?
        .image_recipe_for("flux1-schnell-fp8.safetensors")?
        .apply(Fill {
            prompt,
            seed,
            weights: HashMap::from([("checkpoint", checkpoint)]),
            source: Some(image_name),
        })
}

fn load_workflow_file(path: &std::path::Path) -> Result<Value, ComfyUiError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|_| ComfyUiError::Configuration("COMFYUI_WORKFLOW_PATH is not readable"))?;
    serde_json::from_str(&contents)
        .map_err(|_| ComfyUiError::Configuration("COMFYUI_WORKFLOW_PATH is not valid JSON"))
}

fn load_video_workflow(path: &std::path::Path) -> Result<Value, ComfyUiError> {
    if path.is_file() {
        return load_workflow_file(path)
            .map_err(|_| ComfyUiError::Configuration("video workflow path is not readable"));
    }
    serde_json::from_str(PACKAGED_VIDEO_WORKFLOW)
        .map_err(|_| ComfyUiError::Configuration("packaged video workflow is not valid JSON"))
}

fn load_i2v_workflow(text_to_video_path: &std::path::Path) -> Result<Value, ComfyUiError> {
    let sibling = text_to_video_path
        .parent()
        .map(|directory| directory.join("wan2.2-ti2v-5b-i2v-api.json"));
    if let Some(path) = sibling.filter(|path| path.is_file()) {
        return load_workflow_file(&path).map_err(|_| {
            ComfyUiError::Configuration("image-to-video workflow path is not readable")
        });
    }
    serde_json::from_str(PACKAGED_I2V_WORKFLOW).map_err(|_| {
        ComfyUiError::Configuration("packaged image-to-video workflow is not valid JSON")
    })
}

/// Build the text-to-video workflow and mutate only approved inputs.
pub fn build_wan_t2v_workflow(
    prompt: &str,
    unet: &str,
    clip: &str,
    vae: &str,
    seed: u64,
) -> Result<Value, ComfyUiError> {
    let workflow = serde_json::from_str(PACKAGED_VIDEO_WORKFLOW)
        .map_err(|_| ComfyUiError::Configuration("packaged video workflow is not valid JSON"))?;
    configure_wan_t2v_workflow(workflow, prompt, unet, clip, vae, seed)
}

/// Build the image-to-video workflow and mutate only approved inputs.
pub fn build_wan_i2v_workflow(
    prompt: &str,
    unet: &str,
    clip: &str,
    vae: &str,
    seed: u64,
    image_name: &str,
) -> Result<Value, ComfyUiError> {
    let workflow = serde_json::from_str(PACKAGED_I2V_WORKFLOW).map_err(|_| {
        ComfyUiError::Configuration("packaged image-to-video workflow is not valid JSON")
    })?;
    configure_wan_i2v_workflow(workflow, prompt, unet, clip, vae, seed, image_name)
}
fn validate_video_workflow(workflow: &Value) -> Result<(), ComfyUiError> {
    for pointer in [
        "/1/inputs/unet_name",
        "/2/inputs/clip_name",
        "/3/inputs/vae_name",
        "/5/inputs/text",
        "/7/class_type",
        "/8/inputs/seed",
        "/10/inputs/images",
    ] {
        if workflow.pointer(pointer).is_none() {
            return Err(ComfyUiError::Configuration(
                "workflow does not match the Wan TI2V contract",
            ));
        }
    }
    if workflow.pointer("/7/class_type").and_then(Value::as_str) != Some("Wan22ImageToVideoLatent")
    {
        return Err(ComfyUiError::Configuration(
            "video workflow must use Wan22ImageToVideoLatent",
        ));
    }
    if workflow.pointer("/10/class_type").and_then(Value::as_str) != Some("SaveWEBM") {
        return Err(ComfyUiError::Configuration(
            "video workflow output must use SaveWEBM",
        ));
    }
    Ok(())
}

fn validate_i2v_workflow(workflow: &Value) -> Result<(), ComfyUiError> {
    validate_video_workflow(workflow)?;
    if workflow.pointer("/11/class_type").and_then(Value::as_str) != Some("LoadImage") {
        return Err(ComfyUiError::Configuration(
            "image-to-video workflow must load a source image",
        ));
    }
    if workflow.pointer("/7/inputs/start_image").is_none() {
        return Err(ComfyUiError::Configuration(
            "image-to-video workflow must condition on a start image",
        ));
    }
    Ok(())
}

fn configure_wan_t2v_workflow(
    mut workflow: Value,
    prompt: &str,
    unet: &str,
    clip: &str,
    vae: &str,
    seed: u64,
) -> Result<Value, ComfyUiError> {
    validate_video_workflow(&workflow)?;
    apply_wan_workflow_inputs(&mut workflow, prompt, unet, clip, vae, seed)?;
    Ok(workflow)
}

fn configure_wan_i2v_workflow(
    mut workflow: Value,
    prompt: &str,
    unet: &str,
    clip: &str,
    vae: &str,
    seed: u64,
    image_name: &str,
) -> Result<Value, ComfyUiError> {
    validate_i2v_workflow(&workflow)?;
    apply_wan_workflow_inputs(&mut workflow, prompt, unet, clip, vae, seed)?;
    let image_name = sanitize_upload_name(image_name)?;
    workflow["11"]["inputs"]["image"] = json!(image_name);
    Ok(workflow)
}

fn apply_wan_workflow_inputs(
    workflow: &mut Value,
    prompt: &str,
    unet: &str,
    clip: &str,
    vae: &str,
    seed: u64,
) -> Result<(), ComfyUiError> {
    if prompt.trim().is_empty() || prompt.len() > 100_000 {
        return Err(ComfyUiError::Configuration("prompt is empty or too long"));
    }
    if !is_model_filename(unet) || !is_model_filename(clip) || !is_model_filename(vae) {
        return Err(ComfyUiError::Configuration("invalid video model filename"));
    }
    workflow["1"]["inputs"]["unet_name"] = json!(unet);
    workflow["2"]["inputs"]["clip_name"] = json!(clip);
    workflow["3"]["inputs"]["vae_name"] = json!(vae);
    workflow["5"]["inputs"]["text"] = json!(prompt);
    workflow["8"]["inputs"]["seed"] = json!(seed);
    Ok(())
}

fn normalize_source_mime(mime: &str) -> Result<String, ComfyUiError> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpg" | "image/jpeg" => Ok("image/jpeg".to_string()),
        "image/png" => Ok("image/png".to_string()),
        "image/webp" => Ok("image/webp".to_string()),
        _ => Err(ComfyUiError::Configuration(
            "source image type is not supported",
        )),
    }
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    }
}

fn is_model_filename(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

fn is_video_filename(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".webm") || name.ends_with(".mp4") || name.ends_with(".mkv")
}

fn mime_for_filename(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".webm") {
        "video/webm".to_string()
    } else if name.ends_with(".mp4") {
        "video/mp4".to_string()
    } else if name.ends_with(".mkv") {
        "video/x-matroska".to_string()
    } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if name.ends_with(".webp") {
        "image/webp".to_string()
    } else {
        "image/png".to_string()
    }
}

fn uploaded_image_name(uploaded: &UploadResponse, fallback: &str) -> Result<String, ComfyUiError> {
    if !uploaded.subfolder.trim().is_empty() {
        return Err(ComfyUiError::InvalidResponse(
            "upload returned a nested path",
        ));
    }
    let name = if uploaded.name.trim().is_empty() {
        fallback
    } else {
        uploaded.name.as_str()
    };
    sanitize_upload_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[test]
    fn workflow_mutates_only_approved_inputs() {
        let workflow =
            build_flux_schnell_workflow("a blue fox", "custom-image.safetensors", 42).unwrap();
        assert_eq!(
            workflow["4"]["inputs"]["ckpt_name"],
            "custom-image.safetensors"
        );
        assert_eq!(workflow["6"]["inputs"]["text"], "a blue fox");
        assert_eq!(workflow["3"]["inputs"]["seed"], 42);
        assert_eq!(workflow["3"]["inputs"]["steps"], 4);
        assert_eq!(workflow["5"]["inputs"]["width"], 1024);
        assert!(workflow.get("10").is_none());
    }

    #[test]
    fn img2img_workflow_mutates_only_approved_inputs() {
        let workflow = build_flux_schnell_img2img_workflow(
            "make it dusk",
            "custom-image.safetensors",
            42,
            "zone-img2img-source.png",
        )
        .unwrap();
        assert_eq!(
            workflow["4"]["inputs"]["ckpt_name"],
            "custom-image.safetensors"
        );
        assert_eq!(workflow["6"]["inputs"]["text"], "make it dusk");
        assert_eq!(workflow["3"]["inputs"]["seed"], 42);
        assert_eq!(workflow["3"]["inputs"]["denoise"], 0.75);
        assert_eq!(workflow["3"]["inputs"]["steps"], 4);
        assert_eq!(workflow["10"]["inputs"]["image"], "zone-img2img-source.png");
        assert_eq!(workflow["11"]["inputs"]["width"], 1024);
        assert_eq!(workflow["12"]["class_type"], "VAEEncode");
    }

    #[test]
    fn img2img_workflow_rejects_pathful_filenames() {
        assert!(
            build_flux_schnell_img2img_workflow("fox", "ok.safetensors", 1, "../secret.png")
                .is_err()
        );
        assert!(
            build_flux_schnell_img2img_workflow("fox", "ok.safetensors", 1, "nested/file.png")
                .is_err()
        );
    }

    #[test]
    fn video_workflow_mutates_only_approved_inputs() {
        let workflow = build_wan_t2v_workflow(
            "a moving fox",
            "custom-video.safetensors",
            "custom-clip.safetensors",
            "custom-vae.safetensors",
            42,
        )
        .unwrap();
        assert_eq!(
            workflow["1"]["inputs"]["unet_name"],
            "custom-video.safetensors"
        );
        assert_eq!(
            workflow["2"]["inputs"]["clip_name"],
            "custom-clip.safetensors"
        );
        assert_eq!(
            workflow["3"]["inputs"]["vae_name"],
            "custom-vae.safetensors"
        );
        assert_eq!(workflow["5"]["inputs"]["text"], "a moving fox");
        assert_eq!(workflow["8"]["inputs"]["seed"], 42);
        assert_eq!(workflow["8"]["inputs"]["steps"], 20);
        assert_eq!(workflow["7"]["inputs"]["width"], 832);
        assert!(workflow.get("11").is_none());
    }

    #[test]
    fn i2v_workflow_mutates_only_approved_inputs() {
        let workflow = build_wan_i2v_workflow(
            "make it move",
            "custom-video.safetensors",
            "custom-clip.safetensors",
            "custom-vae.safetensors",
            42,
            "zone-i2v-source.png",
        )
        .unwrap();
        assert_eq!(
            workflow["1"]["inputs"]["unet_name"],
            "custom-video.safetensors"
        );
        assert_eq!(workflow["5"]["inputs"]["text"], "make it move");
        assert_eq!(workflow["8"]["inputs"]["seed"], 42);
        assert_eq!(workflow["11"]["inputs"]["image"], "zone-i2v-source.png");
        assert_eq!(workflow["7"]["class_type"], "Wan22ImageToVideoLatent");
    }

    #[test]
    fn video_workflow_rejects_pathful_filenames() {
        assert!(
            build_wan_t2v_workflow(
                "fox",
                "../secret.safetensors",
                "clip.safetensors",
                "vae.safetensors",
                1
            )
            .is_err()
        );
        assert!(
            build_wan_i2v_workflow(
                "fox",
                "ok.safetensors",
                "clip.safetensors",
                "vae.safetensors",
                1,
                "nested/file.png"
            )
            .is_err()
        );
    }

    #[test]
    fn workflow_rejects_checkpoint_traversal() {
        assert!(build_flux_schnell_workflow("fox", "../secret", 1).is_err());
        assert!(build_flux_schnell_workflow("fox", "models/secret", 1).is_err());
    }

    #[test]
    fn image_client_accepts_empty_video_unet() {
        ComfyUiClient::new(ComfyUiConfig {
            video_unet: String::new(),
            ..Default::default()
        })
        .unwrap();
    }

    #[test]
    fn successful_history_without_video_is_an_error() {
        let nodes = json!({
            "10": {"images": [{"filename": "still.png", "subfolder": "", "type": "output"}]}
        });
        assert!(matches!(
            outputs_from_history_entry("success", nodes.as_object(), OutputMode::Video),
            Err(ComfyUiError::InvalidResponse(
                "workflow completed without a usable output"
            ))
        ));
    }

    #[test]
    fn incomplete_history_without_files_keeps_polling() {
        assert!(
            outputs_from_history_entry("executing", None, OutputMode::Video)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn generate_video_rejects_empty_video_unet() {
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            video_unet: String::new(),
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        assert!(matches!(
            client
                .generate_video("a fox", None, &mut cancel_rx, progress_tx)
                .await,
            Err(ComfyUiError::Configuration(
                "COMFYUI_VIDEO_UNET must be a diffusion model filename"
            ))
        ));
    }

    #[tokio::test]
    async fn submits_recovers_history_and_fetches_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "p1"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/p1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "p1": {"status": {"status_str": "success"}, "outputs": {
                    "7": {"images": [{"filename": "zone.png", "subfolder": "", "type": "temp"}]}
                }}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(vec![1, 2, 3]),
            )
            .mount(&server)
            .await;
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            poll_interval_ms: 50,
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let images = client
            .generate("a fox", None, &mut cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].bytes.as_ref(), &[1, 2, 3]);
        assert_eq!(images[0].filename, "zone.png");
        assert_eq!(progress_rx.recv().await.as_deref(), Some("Image queued..."));
    }

    #[tokio::test]
    async fn cancellation_deletes_specific_prompt_without_global_interrupt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "p2"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/queue"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            poll_interval_ms: 5000,
            ..Default::default()
        })
        .unwrap();
        let (cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            client
                .generate("a fox", None, &mut cancel_rx, progress_tx)
                .await
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        cancel_tx.send(()).unwrap();
        assert!(matches!(task.await.unwrap(), Err(ComfyUiError::Cancelled)));
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.url.path() != "/interrupt")
        );
    }

    #[tokio::test]
    async fn stalled_prompt_request_is_cancellable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(30))
                    .set_body_json(json!({"prompt_id": "never"})),
            )
            .mount(&server)
            .await;
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            request_timeout_secs: 60,
            ..Default::default()
        })
        .unwrap();
        let (cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            client
                .generate("a fox", None, &mut cancel_rx, progress_tx)
                .await
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        cancel_tx.send(()).unwrap();
        assert!(matches!(task.await.unwrap(), Err(ComfyUiError::Cancelled)));
    }

    #[tokio::test]
    async fn img2img_uploads_source_then_submits_encoded_workflow() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "uploaded-source.png",
                "subfolder": "",
                "type": "input"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .and(wiremock::matchers::body_string_contains(
                "uploaded-source.png",
            ))
            .and(wiremock::matchers::body_string_contains("VAEEncode"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "i2i"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/i2i"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "i2i": {"status": {"status_str": "success"}, "outputs": {
                    "9": {"images": [{"filename": "edited.png", "subfolder": "", "type": "temp"}]}
                }}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(vec![9, 8, 7]),
            )
            .mount(&server)
            .await;
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            poll_interval_ms: 50,
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let source = SourceImage::new(vec![1, 2, 3, 4], "image/png").unwrap();
        let images = client
            .generate("make it dusk", Some(&source), &mut cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(images[0].bytes.as_ref(), &[9, 8, 7]);
        assert_eq!(
            progress_rx.recv().await.as_deref(),
            Some("Uploading source image...")
        );
    }

    #[tokio::test]
    async fn video_recovers_webm_history_and_fetches_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .and(wiremock::matchers::body_string_contains(
                "Wan22ImageToVideoLatent",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "v1"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/v1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "v1": {"status": {"status_str": "success"}, "outputs": {
                    "10": {"gifs": [{"filename": "zone.webm", "subfolder": "", "type": "output"}]}
                }}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "video/webm")
                    .set_body_bytes(vec![1, 2, 3, 4]),
            )
            .mount(&server)
            .await;
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            poll_interval_ms: 50,
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let videos = client
            .generate_video("a fox running", None, &mut cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].bytes.as_ref(), &[1, 2, 3, 4]);
        assert_eq!(videos[0].mime, "video/webm");
        assert_eq!(progress_rx.recv().await.as_deref(), Some("Video queued..."));
    }

    #[tokio::test]
    async fn i2v_uploads_source_then_submits_start_image() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "uploaded-source.png",
                "subfolder": "",
                "type": "input"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .and(wiremock::matchers::body_string_contains(
                "uploaded-source.png",
            ))
            .and(wiremock::matchers::body_string_contains("start_image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "i2v"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/i2v"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "i2v": {"status": {"status_str": "success"}, "outputs": {
                    "10": {"videos": [{"filename": "clip.webm", "subfolder": "", "type": "output"}]}
                }}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "video/webm")
                    .set_body_bytes(vec![9, 8, 7, 6]),
            )
            .mount(&server)
            .await;
        let client = ComfyUiClient::new(ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            poll_interval_ms: 50,
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        let source = SourceImage::new(vec![1, 2, 3, 4], "image/png").unwrap();
        let videos = client
            .generate_video("make it move", Some(&source), &mut cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(videos[0].bytes.as_ref(), &[9, 8, 7, 6]);
    }
}
