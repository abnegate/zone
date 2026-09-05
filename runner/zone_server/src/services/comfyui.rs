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
    #[error("image generation timed out")]
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
            .build()?;
        let catalog = RecipeCatalog::load(Some(config.workflow_path.as_path()))?;
        let _ = catalog.image_recipe_for(&config.checkpoint)?;
        Ok(Self {
            config,
            client,
            catalog,
        })
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
        let _ = progress.send("Image queued...".to_string());

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
                        let _ = progress.send("Generating image...".to_string());
                        announced_generation = true;
                    }
                    match self.history_outputs(&prompt_id, cancel, deadline).await {
                        Ok(Some(outputs)) => {
                            let _ = progress.send("Saving generated image...".to_string());
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
        if status == "error" {
            return Err(ComfyUiError::InvalidResponse("workflow execution failed"));
        }
        let Some(nodes) = entry.get("outputs").and_then(Value::as_object) else {
            return Ok(None);
        };
        let mut images = Vec::new();
        for node in nodes.values() {
            if let Some(items) = node.get("images").and_then(Value::as_array) {
                for item in items {
                    let image: OutputImage = serde_json::from_value(item.clone())
                        .map_err(|_| ComfyUiError::InvalidResponse("invalid image output"))?;
                    if image.r#type != "temp" {
                        return Err(ComfyUiError::InvalidResponse(
                            "workflow returned a non-temporary image",
                        ));
                    }
                    images.push(image);
                }
            }
        }
        if images.is_empty() {
            return Ok(None);
        }
        Ok(Some(images))
    }

    async fn fetch_outputs(
        &self,
        outputs: Vec<OutputImage>,
        cancel: &mut broadcast::Receiver<()>,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        let mut generated = Vec::with_capacity(outputs.len());
        for output in outputs {
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
                        .filter(|value| value.starts_with("image/"))
                        .unwrap_or("image/png")
                        .to_string();
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
    fn workflow_rejects_checkpoint_traversal() {
        assert!(build_flux_schnell_workflow("fox", "../secret", 1).is_err());
        assert!(build_flux_schnell_workflow("fox", "models/secret", 1).is_err());
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
}
