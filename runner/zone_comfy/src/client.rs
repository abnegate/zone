//! Direct ComfyUI API client. Graphs come from packaged recipes; chat only
//! supplies prompt, seed, checkpoint filename, and an optional source image.

use reqwest::Client as HttpClient;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::config::Config;
use crate::media::MediaType;
use crate::recipe::{
    Fill, PromptMode, Recipe, RecipeCatalog, sanitize_upload_name, sanitize_weight_filename,
};

pub const MAX_SOURCE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const PACKAGED_VIDEO_WORKFLOW: &str =
    include_str!("../../../comfyui/workflows/wan2.2-ti2v-5b-api.json");
const PACKAGED_I2V_WORKFLOW: &str =
    include_str!("../../../comfyui/workflows/wan2.2-ti2v-5b-i2v-api.json");
const PACKAGED_AUDIO_WORKFLOW: &str =
    include_str!("../../../comfyui/workflows/ace-step-v1-3.5b-api.json");

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    pub fn new(bytes: impl Into<bytes::Bytes>, mime: &str) -> Result<Self, Error> {
        let mime = normalize_source_mime(mime)?;
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_SOURCE_IMAGE_BYTES {
            return Err(Error::Configuration("source image is empty or too large"));
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
pub struct Client {
    config: Config,
    client: HttpClient,
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
    Audio,
}

fn collect_output_files(node: &Value, mode: OutputMode) -> Result<Vec<OutputImage>, Error> {
    let keys = match mode {
        OutputMode::Image => &["images"][..],
        OutputMode::Video => &["videos", "gifs", "images"][..],
        OutputMode::Audio => &["audio"][..],
    };
    let mut files = Vec::new();
    for key in keys {
        let Some(items) = node.get(*key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let file: OutputImage = serde_json::from_value(item.clone())
                .map_err(|_| Error::InvalidResponse("invalid media output"))?;
            match mode {
                OutputMode::Image => {
                    if file.r#type != "temp" {
                        return Err(Error::InvalidResponse(
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
                        return Err(Error::InvalidResponse(
                            "workflow returned an unsupported video location",
                        ));
                    }
                    files.push(file);
                }
                OutputMode::Audio => {
                    if !is_audio_filename(&file.filename) {
                        continue;
                    }
                    if file.r#type != "temp" {
                        return Err(Error::InvalidResponse(
                            "workflow returned a non-temporary audio file",
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
) -> Result<Option<Vec<OutputImage>>, Error> {
    if status == "error" {
        return Err(Error::InvalidResponse("workflow execution failed"));
    }
    let mut files = Vec::new();
    if let Some(nodes) = nodes {
        for node in nodes.values() {
            files.extend(collect_output_files(node, mode)?);
        }
    }
    if files.is_empty() {
        if status == "success" {
            return Err(Error::InvalidResponse(
                "workflow completed without a usable output",
            ));
        }
        return Ok(None);
    }
    Ok(Some(files))
}

impl Client {
    pub fn new(config: Config) -> Result<Self, Error> {
        if config.base_url.trim().is_empty() {
            return Err(Error::Configuration("COMFYUI_BASE_URL is empty"));
        }
        sanitize_weight_filename(&config.checkpoint).map_err(|_| {
            Error::Configuration("COMFYUI_CHECKPOINT must be a checkpoint filename")
        })?;
        let client = HttpClient::builder()
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

    pub fn prompt_mode(&self) -> PromptMode {
        self.image_recipe()
            .map(|recipe| recipe.prompt_mode)
            .unwrap_or(PromptMode::ClipScene)
    }

    fn image_recipe(&self) -> Result<&Recipe, Error> {
        let selected = self.config.checkpoint.as_str();
        if self.config.models_dir.is_dir() {
            let items = crate::inventory::scan(&self.config.models_dir, &self.catalog);
            if let Some(item) = crate::inventory::find(&items, selected)
                && let Some(recipe) = self.catalog.get(&item.recipe_id)
            {
                return Ok(recipe);
            }
        }
        self.catalog.image_recipe_for(selected)
    }

    fn video_workflows(&self) -> Result<(Value, Value), Error> {
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
                return Err(Error::Configuration(message));
            }
        }
        let video_workflow = load_video_workflow(&self.config.video_workflow_path)?;
        validate_video_workflow(&video_workflow)?;
        let i2v_workflow = load_i2v_workflow(&self.config.video_workflow_path)?;
        validate_i2v_workflow(&i2v_workflow)?;
        Ok((video_workflow, i2v_workflow))
    }

    fn audio_workflow(&self) -> Result<Value, Error> {
        sanitize_weight_filename(&self.config.audio_checkpoint).map_err(|_| {
            Error::Configuration("COMFYUI_AUDIO_CHECKPOINT must be a checkpoint filename")
        })?;
        let workflow = load_audio_workflow(&self.config.audio_workflow_path)?;
        validate_audio_workflow(&workflow)?;
        Ok(workflow)
    }

    pub async fn generate(
        &self,
        prompt: &str,
        source: Option<&SourceImage>,
        cancel: &mut broadcast::Receiver<()>,
        progress: mpsc::UnboundedSender<String>,
    ) -> Result<Vec<GeneratedImage>, Error> {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }

        if cancel.try_recv().is_ok() {
            return Err(Error::Cancelled);
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.config.generation_timeout_secs);
        let prompt = if prompt.trim().is_empty() {
            if source.is_some() {
                "edit this image"
            } else {
                return Err(Error::Configuration("prompt is empty or too long"));
            }
        } else {
            prompt
        };
        let recipe = self.image_recipe()?;
        let weights = recipe.weight_map(&self.config.checkpoint)?;
        let fill_weights: HashMap<&str, &str> = weights
            .iter()
            .map(|(name, filename)| (name.as_str(), filename.as_str()))
            .collect();
        let workflow = if let Some(source) = source {
            let _ = progress.send("Uploading source image...".to_string());
            let uploaded = self.upload_source(source, cancel, deadline).await?;
            recipe.apply(Fill {
                prompt,
                seed: rand::random::<u64>() & i64::MAX as u64,
                weights: fill_weights,
                source: Some(uploaded.as_str()),
            })?
        } else {
            recipe.apply(Fill {
                prompt,
                seed: rand::random::<u64>() & i64::MAX as u64,
                weights: fill_weights,
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
    ) -> Result<Vec<GeneratedImage>, Error> {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }

        if cancel.try_recv().is_ok() {
            return Err(Error::Cancelled);
        }
        let (video_workflow, i2v_workflow) = self.video_workflows()?;
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(self.config.video_generation_timeout_secs);
        let prompt = if prompt.trim().is_empty() {
            if source.is_some() {
                "animate this image"
            } else {
                return Err(Error::Configuration("prompt is empty or too long"));
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

    pub async fn generate_audio(
        &self,
        prompt: &str,
        cancel: &mut broadcast::Receiver<()>,
        progress: mpsc::UnboundedSender<String>,
    ) -> Result<Vec<GeneratedImage>, Error> {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }

        if cancel.try_recv().is_ok() {
            return Err(Error::Cancelled);
        }
        let audio_workflow = self.audio_workflow()?;
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(self.config.audio_generation_timeout_secs);
        let workflow = configure_ace_step_workflow(
            audio_workflow,
            prompt,
            &self.config.audio_checkpoint,
            rand::random::<u64>() & i64::MAX as u64,
        )?;
        self.submit_and_collect(
            workflow,
            cancel,
            deadline,
            progress,
            "Audio queued...",
            "Generating audio...",
            "Saving generated audio...",
            OutputMode::Audio,
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
    ) -> Result<Vec<GeneratedImage>, Error> {
        let started = std::time::Instant::now();
        let kind = match mode {
            OutputMode::Image => "image",
            OutputMode::Video => "video",
            OutputMode::Audio => "audio",
        };
        let result = self
            .submit_and_collect_inner(
                workflow, cancel, deadline, progress, queued, generating, saving, mode,
            )
            .await;
        let status = match &result {
            Ok(_) => "ok",
            Err(Error::Disabled) => "disabled",
            Err(Error::Timeout) => "timeout",
            Err(Error::Cancelled) => "cancelled",
            Err(Error::Http(_)) => "http_error",
            Err(Error::Configuration(_)) => "config_error",
            Err(Error::InvalidResponse(_)) => "invalid_response",
        };
        crate::observe::record(kind, status, started.elapsed());
        result
    }

    async fn submit_and_collect_inner(
        &self,
        workflow: Value,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
        progress: mpsc::UnboundedSender<String>,
        queued: &str,
        generating: &str,
        saving: &str,
        mode: OutputMode,
    ) -> Result<Vec<GeneratedImage>, Error> {
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
                    return Err(Error::Cancelled);
                }
                _ = tokio::time::sleep_until(deadline) => {
                    self.cancel(&prompt_id).await;
                    return Err(Error::Timeout);
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
                            if matches!(result, Err(Error::Cancelled | Error::Timeout)) {
                                self.cancel(&prompt_id).await;
                            }
                            return result;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if matches!(error, Error::Cancelled | Error::Timeout) {
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
    ) -> Result<String, Error> {
        let filename = sanitize_upload_name(&source.filename)?;
        let part = reqwest::multipart::Part::bytes(source.bytes.to_vec())
            .file_name(filename.clone())
            .mime_str(&source.mime)
            .map_err(|_| Error::Configuration("source image type is not supported"))?;
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
    ) -> Result<T, Error>
    where
        F: Future<Output = Result<T, reqwest::Error>>,
    {
        tokio::select! {
            biased;
            _ = cancel.recv() => Err(Error::Cancelled),
            _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout),
            result = request => result.map_err(Error::Http),
        }
    }

    async fn history_outputs(
        &self,
        prompt_id: &str,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
        mode: OutputMode,
    ) -> Result<Option<Vec<OutputImage>>, Error> {
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
    ) -> Result<Vec<GeneratedImage>, Error> {
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
                        .filter(|value| {
                            value.starts_with("image/")
                                || value.starts_with("video/")
                                || value.starts_with("audio/")
                        })
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
) -> Result<Value, Error> {
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
) -> Result<Value, Error> {
    RecipeCatalog::packaged()?
        .image_recipe_for("flux1-schnell-fp8.safetensors")?
        .apply(Fill {
            prompt,
            seed,
            weights: HashMap::from([("checkpoint", checkpoint)]),
            source: Some(image_name),
        })
}

fn load_workflow_file(path: &std::path::Path) -> Result<Value, Error> {
    let contents = std::fs::read_to_string(path)
        .map_err(|_| Error::Configuration("COMFYUI_WORKFLOW_PATH is not readable"))?;
    serde_json::from_str(&contents)
        .map_err(|_| Error::Configuration("COMFYUI_WORKFLOW_PATH is not valid JSON"))
}

fn load_video_workflow(path: &std::path::Path) -> Result<Value, Error> {
    if path.is_file() {
        return load_workflow_file(path)
            .map_err(|_| Error::Configuration("video workflow path is not readable"));
    }
    serde_json::from_str(PACKAGED_VIDEO_WORKFLOW)
        .map_err(|_| Error::Configuration("packaged video workflow is not valid JSON"))
}

fn load_i2v_workflow(text_to_video_path: &std::path::Path) -> Result<Value, Error> {
    let sibling = text_to_video_path
        .parent()
        .map(|directory| directory.join("wan2.2-ti2v-5b-i2v-api.json"));
    if let Some(path) = sibling.filter(|path| path.is_file()) {
        return load_workflow_file(&path)
            .map_err(|_| Error::Configuration("image-to-video workflow path is not readable"));
    }
    serde_json::from_str(PACKAGED_I2V_WORKFLOW)
        .map_err(|_| Error::Configuration("packaged image-to-video workflow is not valid JSON"))
}

fn load_audio_workflow(path: &std::path::Path) -> Result<Value, Error> {
    if path.is_file() {
        return load_workflow_file(path)
            .map_err(|_| Error::Configuration("audio workflow path is not readable"));
    }
    serde_json::from_str(PACKAGED_AUDIO_WORKFLOW)
        .map_err(|_| Error::Configuration("packaged audio workflow is not valid JSON"))
}

/// Build the text-to-video workflow and mutate only approved inputs.
pub fn build_wan_t2v_workflow(
    prompt: &str,
    unet: &str,
    clip: &str,
    vae: &str,
    seed: u64,
) -> Result<Value, Error> {
    let workflow = serde_json::from_str(PACKAGED_VIDEO_WORKFLOW)
        .map_err(|_| Error::Configuration("packaged video workflow is not valid JSON"))?;
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
) -> Result<Value, Error> {
    let workflow = serde_json::from_str(PACKAGED_I2V_WORKFLOW)
        .map_err(|_| Error::Configuration("packaged image-to-video workflow is not valid JSON"))?;
    configure_wan_i2v_workflow(workflow, prompt, unet, clip, vae, seed, image_name)
}

/// Build the text-to-audio workflow and mutate only approved inputs.
pub fn build_ace_step_workflow(prompt: &str, checkpoint: &str, seed: u64) -> Result<Value, Error> {
    let workflow = serde_json::from_str(PACKAGED_AUDIO_WORKFLOW)
        .map_err(|_| Error::Configuration("packaged audio workflow is not valid JSON"))?;
    configure_ace_step_workflow(workflow, prompt, checkpoint, seed)
}

fn validate_video_workflow(workflow: &Value) -> Result<(), Error> {
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
            return Err(Error::Configuration(
                "workflow does not match the Wan TI2V contract",
            ));
        }
    }
    if workflow.pointer("/7/class_type").and_then(Value::as_str) != Some("Wan22ImageToVideoLatent")
    {
        return Err(Error::Configuration(
            "video workflow must use Wan22ImageToVideoLatent",
        ));
    }
    if workflow.pointer("/10/class_type").and_then(Value::as_str) != Some("SaveWEBM") {
        return Err(Error::Configuration(
            "video workflow output must use SaveWEBM",
        ));
    }
    Ok(())
}

fn validate_i2v_workflow(workflow: &Value) -> Result<(), Error> {
    validate_video_workflow(workflow)?;
    if workflow.pointer("/11/class_type").and_then(Value::as_str) != Some("LoadImage") {
        return Err(Error::Configuration(
            "image-to-video workflow must load a source image",
        ));
    }
    if workflow.pointer("/7/inputs/start_image").is_none() {
        return Err(Error::Configuration(
            "image-to-video workflow must condition on a start image",
        ));
    }
    Ok(())
}

fn validate_audio_workflow(workflow: &Value) -> Result<(), Error> {
    for pointer in [
        "/1/inputs/ckpt_name",
        "/5/inputs/tags",
        "/5/inputs/lyrics",
        "/8/inputs/seed",
        "/10/inputs/audio",
    ] {
        if workflow.pointer(pointer).is_none() {
            return Err(Error::Configuration(
                "workflow does not match the ACE-Step contract",
            ));
        }
    }
    // Node 5 is where the caller's prompt lands, so pin its class as tightly as
    // the output node: any other node type with a `tags` input would otherwise
    // pass and receive the prompt.
    if workflow.pointer("/5/class_type").and_then(Value::as_str) != Some("TextEncodeAceStepAudio") {
        return Err(Error::Configuration(
            "audio workflow must encode the prompt with TextEncodeAceStepAudio",
        ));
    }
    if workflow.pointer("/7/class_type").and_then(Value::as_str) != Some("EmptyAceStepLatentAudio")
    {
        return Err(Error::Configuration(
            "audio workflow must use EmptyAceStepLatentAudio",
        ));
    }
    if workflow.pointer("/9/class_type").and_then(Value::as_str) != Some("VAEDecodeAudio") {
        return Err(Error::Configuration(
            "audio workflow must decode audio with VAEDecodeAudio",
        ));
    }
    if workflow.pointer("/10/class_type").and_then(Value::as_str) != Some("PreviewAudio") {
        return Err(Error::Configuration(
            "audio workflow output must use temporary PreviewAudio storage",
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
) -> Result<Value, Error> {
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
) -> Result<Value, Error> {
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
) -> Result<(), Error> {
    if prompt.trim().is_empty() || prompt.len() > 100_000 {
        return Err(Error::Configuration("prompt is empty or too long"));
    }
    if !is_model_filename(unet) || !is_model_filename(clip) || !is_model_filename(vae) {
        return Err(Error::Configuration("invalid video model filename"));
    }
    workflow["1"]["inputs"]["unet_name"] = json!(unet);
    workflow["2"]["inputs"]["clip_name"] = json!(clip);
    workflow["3"]["inputs"]["vae_name"] = json!(vae);
    workflow["5"]["inputs"]["text"] = json!(prompt);
    workflow["8"]["inputs"]["seed"] = json!(seed);
    Ok(())
}

fn configure_ace_step_workflow(
    mut workflow: Value,
    prompt: &str,
    checkpoint: &str,
    seed: u64,
) -> Result<Value, Error> {
    validate_audio_workflow(&workflow)?;
    apply_ace_step_workflow_inputs(&mut workflow, prompt, checkpoint, seed)?;
    Ok(workflow)
}

fn apply_ace_step_workflow_inputs(
    workflow: &mut Value,
    prompt: &str,
    checkpoint: &str,
    seed: u64,
) -> Result<(), Error> {
    if prompt.trim().is_empty() || prompt.len() > 100_000 {
        return Err(Error::Configuration("prompt is empty or too long"));
    }
    let checkpoint = sanitize_weight_filename(checkpoint)?;
    workflow["1"]["inputs"]["ckpt_name"] = json!(checkpoint);
    workflow["5"]["inputs"]["tags"] = json!(prompt);
    // Overwrite rather than trust the graph: lyrics authored into an
    // operator-supplied workflow would otherwise be sung over every generation.
    workflow["5"]["inputs"]["lyrics"] = json!("");
    workflow["8"]["inputs"]["seed"] = json!(seed);
    Ok(())
}

fn normalize_source_mime(mime: &str) -> Result<String, Error> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpg" | "image/jpeg" => Ok("image/jpeg".to_string()),
        "image/png" => Ok("image/png".to_string()),
        "image/webp" => Ok("image/webp".to_string()),
        _ => Err(Error::Configuration("source image type is not supported")),
    }
}

fn extension_for_mime(mime: &str) -> &'static str {
    MediaType::for_mime(mime)
        .filter(MediaType::is_image)
        .unwrap_or(MediaType::PNG)
        .extension
}

fn is_model_filename(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

fn is_video_filename(name: &str) -> bool {
    MediaType::for_filename(name).is_some_and(|media| media.is_video())
}

fn is_audio_filename(name: &str) -> bool {
    MediaType::for_filename(name).is_some_and(|media| media.is_audio())
}

fn mime_for_filename(name: &str) -> String {
    MediaType::for_filename(name)
        .unwrap_or(MediaType::PNG)
        .mime
        .to_string()
}

fn uploaded_image_name(uploaded: &UploadResponse, fallback: &str) -> Result<String, Error> {
    if !uploaded.subfolder.trim().is_empty() {
        return Err(Error::InvalidResponse("upload returned a nested path"));
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
    fn audio_workflow_mutates_only_approved_inputs() {
        let workflow =
            build_ace_step_workflow("forest ambience", "custom-audio.safetensors", 42).unwrap();
        assert_eq!(
            workflow["1"]["inputs"]["ckpt_name"],
            "custom-audio.safetensors"
        );
        assert_eq!(workflow["5"]["inputs"]["tags"], "forest ambience");
        assert_eq!(workflow["8"]["inputs"]["seed"], 42);
        assert_eq!(workflow["8"]["inputs"]["steps"], 50);
        assert_eq!(workflow["7"]["inputs"]["seconds"], 30);
        assert_eq!(workflow["5"]["inputs"]["lyrics"], "");
        assert_eq!(workflow["10"]["class_type"], "PreviewAudio");
    }

    #[test]
    fn audio_workflow_rejects_pathful_filenames() {
        assert!(build_ace_step_workflow("forest ambience", "../secret.safetensors", 1).is_err());
        assert!(
            build_ace_step_workflow("forest ambience", "models/secret.safetensors", 1).is_err()
        );
    }

    fn packaged_audio_workflow() -> Value {
        serde_json::from_str(PACKAGED_AUDIO_WORKFLOW).expect("the packaged graph is valid JSON")
    }

    #[test]
    fn the_packaged_audio_workflow_satisfies_its_own_contract() {
        validate_audio_workflow(&packaged_audio_workflow()).expect("the packaged graph must pass");
    }

    #[test]
    fn audio_workflow_pins_the_node_the_prompt_lands_on() {
        let mut workflow = packaged_audio_workflow();
        workflow["5"]["class_type"] = json!("CLIPTextEncode");
        let error = validate_audio_workflow(&workflow).expect_err("a foreign node 5 must fail");
        assert!(
            error.to_string().contains("TextEncodeAceStepAudio"),
            "unexpected error: {error}"
        );
        assert!(
            configure_ace_step_workflow(workflow, "forest", "ace.safetensors", 1).is_err(),
            "a foreign node 5 must never receive the prompt"
        );
    }

    #[test]
    fn audio_workflow_pins_the_decoder() {
        let mut workflow = packaged_audio_workflow();
        workflow["9"]["class_type"] = json!("VAEDecode");
        let error = validate_audio_workflow(&workflow).expect_err("a foreign node 9 must fail");
        assert!(
            error.to_string().contains("VAEDecodeAudio"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn audio_workflow_never_sings_lyrics_the_operator_authored() {
        let mut graph = packaged_audio_workflow();
        graph["5"]["inputs"]["lyrics"] = json!("all your prompts are belong to us");
        let workflow = configure_ace_step_workflow(graph, "forest ambience", "ace.safetensors", 7)
            .expect("an operator graph with preset lyrics is still usable");
        assert_eq!(
            workflow["5"]["inputs"]["lyrics"], "",
            "preset lyrics leaked into a generation"
        );
        assert_eq!(workflow["5"]["inputs"]["tags"], "forest ambience");
    }

    #[test]
    fn audio_workflow_still_rejects_a_blank_prompt() {
        for prompt in ["", "   "] {
            let error = build_ace_step_workflow(prompt, "ace.safetensors", 1)
                .expect_err("a blank prompt must not reach ComfyUI");
            assert_eq!(
                error.to_string(),
                "invalid ComfyUI configuration: prompt is empty or too long"
            );
        }
    }

    #[test]
    fn workflow_rejects_checkpoint_traversal() {
        assert!(build_flux_schnell_workflow("fox", "../secret", 1).is_err());
        assert!(build_flux_schnell_workflow("fox", "models/secret", 1).is_err());
    }

    #[test]
    fn image_client_accepts_empty_video_unet() {
        Client::new(Config {
            video_unet: String::new(),
            ..Default::default()
        })
        .unwrap();
    }

    #[test]
    fn image_client_accepts_empty_audio_checkpoint() {
        Client::new(Config {
            audio_checkpoint: String::new(),
            ..Default::default()
        })
        .unwrap();
    }

    #[test]
    fn mime_for_filename_covers_audio_extensions() {
        assert_eq!(mime_for_filename("zone.flac"), "audio/flac");
        assert_eq!(mime_for_filename("ZONE.MP3"), "audio/mpeg");
        assert_eq!(mime_for_filename("zone.opus"), "audio/ogg");
        assert_eq!(mime_for_filename("zone.wav"), "audio/wav");
    }

    #[test]
    fn successful_history_without_audio_is_an_error() {
        let nodes = json!({
            "10": {"images": [{"filename": "cover.png", "subfolder": "", "type": "temp"}]}
        });
        assert!(matches!(
            outputs_from_history_entry("success", nodes.as_object(), OutputMode::Audio),
            Err(Error::InvalidResponse(
                "workflow completed without a usable output"
            ))
        ));
    }

    #[test]
    fn audio_output_from_output_directory_is_rejected() {
        let nodes = json!({
            "10": {"audio": [{"filename": "zone.flac", "subfolder": "", "type": "output"}]}
        });
        assert!(matches!(
            outputs_from_history_entry("success", nodes.as_object(), OutputMode::Audio),
            Err(Error::InvalidResponse(
                "workflow returned a non-temporary audio file"
            ))
        ));
    }

    #[test]
    fn successful_history_without_video_is_an_error() {
        let nodes = json!({
            "10": {"images": [{"filename": "still.png", "subfolder": "", "type": "output"}]}
        });
        assert!(matches!(
            outputs_from_history_entry("success", nodes.as_object(), OutputMode::Video),
            Err(Error::InvalidResponse(
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
        let client = Client::new(Config {
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
            Err(Error::Configuration(
                "COMFYUI_VIDEO_UNET must be a diffusion model filename"
            ))
        ));
    }

    #[tokio::test]
    async fn generate_audio_rejects_empty_audio_checkpoint() {
        let client = Client::new(Config {
            enabled: true,
            audio_checkpoint: String::new(),
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        assert!(matches!(
            client
                .generate_audio("forest ambience", &mut cancel_rx, progress_tx)
                .await,
            Err(Error::Configuration(
                "COMFYUI_AUDIO_CHECKPOINT must be a checkpoint filename"
            ))
        ));
    }

    #[tokio::test]
    async fn generate_audio_rejects_a_blank_prompt_without_reaching_comfyui() {
        let client = Client::new(Config {
            enabled: true,
            base_url: "http://127.0.0.1:1".to_string(),
            ..Default::default()
        })
        .unwrap();
        for prompt in ["", "   "] {
            let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
            let (progress_tx, _) = mpsc::unbounded_channel();
            assert!(
                matches!(
                    client
                        .generate_audio(prompt, &mut cancel_rx, progress_tx)
                        .await,
                    Err(Error::Configuration("prompt is empty or too long"))
                ),
                "{prompt:?} should be rejected before any request"
            );
        }
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
        let client = Client::new(Config {
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
    async fn adapter_generate_loads_lora_and_keeps_instruction_prompt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .and(wiremock::matchers::body_string_contains(
                "LoraLoaderModelOnly",
            ))
            .and(wiremock::matchers::body_string_contains(
                "qwen-image-edit-plus-nsfw-lora.safetensors",
            ))
            .and(wiremock::matchers::body_string_contains("remove the sign"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "lora"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/lora"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "lora": {"status": {"status_str": "success"}, "outputs": {
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
        let client = Client::new(Config {
            enabled: true,
            base_url: server.uri(),
            checkpoint: "qwen-image-edit-plus-nsfw-lora.safetensors".into(),
            poll_interval_ms: 50,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            client.prompt_mode(),
            crate::recipe::PromptMode::EditInstruction
        );
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        let images = client
            .generate("remove the sign", None, &mut cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(images[0].bytes.as_ref(), &[9, 8, 7]);
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
        let client = Client::new(Config {
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
        assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
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
        let client = Client::new(Config {
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
        assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
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
        let client = Client::new(Config {
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
        let client = Client::new(Config {
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
        let client = Client::new(Config {
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

    #[tokio::test]
    async fn audio_recovers_flac_history_and_fetches_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .and(wiremock::matchers::body_string_contains(
                "EmptyAceStepLatentAudio",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id": "a1"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/a1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "a1": {"status": {"status_str": "success"}, "outputs": {
                    "10": {"audio": [{"filename": "zone.flac", "subfolder": "", "type": "temp"}]}
                }}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/flac")
                    .set_body_bytes(vec![4, 3, 2, 1]),
            )
            .mount(&server)
            .await;
        let client = Client::new(Config {
            enabled: true,
            base_url: server.uri(),
            poll_interval_ms: 50,
            ..Default::default()
        })
        .unwrap();
        let (_cancel_tx, mut cancel_rx) = broadcast::channel(1);
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let clips = client
            .generate_audio("shuffling through a forest", &mut cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].bytes.as_ref(), &[4, 3, 2, 1]);
        assert_eq!(clips[0].mime, "audio/flac");
        assert_eq!(clips[0].filename, "zone.flac");
        assert_eq!(progress_rx.recv().await.as_deref(), Some("Audio queued..."));
    }
}
