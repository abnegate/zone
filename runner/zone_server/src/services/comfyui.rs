//! Direct ComfyUI API client and versioned FLUX.1 Schnell workflow.

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::config::ComfyUiConfig;

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

#[derive(Clone)]
pub struct ComfyUiClient {
    config: ComfyUiConfig,
    client: Client,
    workflow: Value,
}

#[derive(Deserialize)]
struct PromptResponse {
    prompt_id: String,
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
        if config.checkpoint.is_empty()
            || config.checkpoint.contains('/')
            || config.checkpoint.contains('\\')
            || config.checkpoint.contains("..")
        {
            return Err(ComfyUiError::Configuration(
                "COMFYUI_CHECKPOINT must be a checkpoint filename",
            ));
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let workflow = std::fs::read_to_string(&config.workflow_path)
            .map_err(|_| ComfyUiError::Configuration("COMFYUI_WORKFLOW_PATH is not readable"))
            .and_then(|contents| {
                serde_json::from_str(&contents).map_err(|_| {
                    ComfyUiError::Configuration("COMFYUI_WORKFLOW_PATH is not valid JSON")
                })
            })?;
        validate_workflow(&workflow)?;
        Ok(Self {
            config,
            client,
            workflow,
        })
    }

    pub async fn generate(
        &self,
        prompt: &str,
        mut cancel: broadcast::Receiver<()>,
        progress: mpsc::UnboundedSender<String>,
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        if !self.config.enabled {
            return Err(ComfyUiError::Disabled);
        }

        let workflow = configure_flux_schnell_workflow(
            self.workflow.clone(),
            prompt,
            &self.config.checkpoint,
            rand::random::<u64>() & i64::MAX as u64,
        )?;
        let response = self
            .client
            .post(format!("{}/prompt", self.config.base_url))
            .json(&json!({
                "prompt": workflow,
                "client_id": Uuid::new_v4().to_string()
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<PromptResponse>()
            .await?;
        let prompt_id = response.prompt_id;
        let _ = progress.send("Image queued...".to_string());

        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.config.generation_timeout_secs);
        let mut announced_generation = false;
        loop {
            tokio::select! {
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
                    if let Some(outputs) = self.history_outputs(&prompt_id).await? {
                        let _ = progress.send("Saving generated image...".to_string());
                        return self.fetch_outputs(outputs).await;
                    }
                }
            }
        }
    }

    async fn history_outputs(
        &self,
        prompt_id: &str,
    ) -> Result<Option<Vec<OutputImage>>, ComfyUiError> {
        let history = self
            .client
            .get(format!("{}/history/{}", self.config.base_url, prompt_id))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
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
                    images.push(
                        serde_json::from_value(item.clone())
                            .map_err(|_| ComfyUiError::InvalidResponse("invalid image output"))?,
                    );
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
    ) -> Result<Vec<GeneratedImage>, ComfyUiError> {
        let mut generated = Vec::with_capacity(outputs.len());
        for output in outputs {
            let response = self
                .client
                .get(format!("{}/view", self.config.base_url))
                .query(&[
                    ("filename", output.filename.as_str()),
                    ("subfolder", output.subfolder.as_str()),
                    ("type", output.r#type.as_str()),
                ])
                .send()
                .await?
                .error_for_status()?;
            let mime = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .filter(|value| value.starts_with("image/"))
                .unwrap_or("image/png")
                .to_string();
            generated.push(GeneratedImage {
                bytes: response.bytes().await?,
                mime,
                filename: output.filename,
            });
        }
        Ok(generated)
    }

    async fn cancel(&self, prompt_id: &str) {
        if let Err(error) = self
            .client
            .post(format!("{}/queue", self.config.base_url))
            .json(&json!({ "delete": [prompt_id] }))
            .send()
            .await
        {
            tracing::warn!("Failed to cancel ComfyUI prompt {}: {}", prompt_id, error);
        }
        // ComfyUI only removes queued work above; a running workflow requires
        // the process-wide interrupt endpoint. Zone serializes its image jobs
        // before invoking this method so it cannot interrupt another Zone user.
        if let Err(error) = self
            .client
            .post(format!("{}/interrupt", self.config.base_url))
            .json(&json!({ "prompt_id": prompt_id }))
            .send()
            .await
        {
            tracing::warn!(
                "Failed to interrupt ComfyUI prompt {}: {}",
                prompt_id,
                error
            );
        }
    }
}

/// Build the fixed workflow and mutate only prompt, checkpoint, and random seed.
pub fn build_flux_schnell_workflow(
    prompt: &str,
    checkpoint: &str,
    seed: u64,
) -> Result<Value, ComfyUiError> {
    let workflow = serde_json::from_str(include_str!(
        "../../../../comfyui/workflows/flux1-schnell-fp8-api.json"
    ))
    .map_err(|_| ComfyUiError::Configuration("packaged workflow is not valid JSON"))?;
    configure_flux_schnell_workflow(workflow, prompt, checkpoint, seed)
}

fn validate_workflow(workflow: &Value) -> Result<(), ComfyUiError> {
    for pointer in [
        "/3/inputs/seed",
        "/4/inputs/ckpt_name",
        "/5/inputs/width",
        "/5/inputs/height",
        "/6/inputs/text",
        "/9/inputs/filename_prefix",
    ] {
        if workflow.pointer(pointer).is_none() {
            return Err(ComfyUiError::Configuration(
                "workflow does not match the FLUX Schnell contract",
            ));
        }
    }
    Ok(())
}

fn configure_flux_schnell_workflow(
    mut workflow: Value,
    prompt: &str,
    checkpoint: &str,
    seed: u64,
) -> Result<Value, ComfyUiError> {
    if prompt.trim().is_empty() || prompt.len() > 100_000 {
        return Err(ComfyUiError::Configuration("prompt is empty or too long"));
    }
    if checkpoint.is_empty()
        || checkpoint.contains('/')
        || checkpoint.contains('\\')
        || checkpoint.contains("..")
    {
        return Err(ComfyUiError::Configuration("invalid checkpoint filename"));
    }
    validate_workflow(&workflow)?;
    workflow["3"]["inputs"]["seed"] = json!(seed);
    workflow["4"]["inputs"]["ckpt_name"] = json!(checkpoint);
    workflow["6"]["inputs"]["text"] = json!(prompt);
    workflow["9"]["inputs"]["filename_prefix"] = json!("Zone/flux1-schnell");
    Ok(workflow)
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
        let workflow = build_flux_schnell_workflow("a blue fox", "flux.safetensors", 42).unwrap();
        assert_eq!(workflow["4"]["inputs"]["ckpt_name"], "flux.safetensors");
        assert_eq!(workflow["6"]["inputs"]["text"], "a blue fox");
        assert_eq!(workflow["3"]["inputs"]["seed"], 42);
        assert_eq!(workflow["3"]["inputs"]["steps"], 4);
        assert_eq!(workflow["5"]["inputs"]["width"], 1024);
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
                    "7": {"images": [{"filename": "zone.png", "subfolder": "", "type": "output"}]}
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
        let (_cancel_tx, cancel_rx) = broadcast::channel(1);
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let images = client
            .generate("a fox", cancel_rx, progress_tx)
            .await
            .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].bytes.as_ref(), &[1, 2, 3]);
        assert_eq!(images[0].filename, "zone.png");
        assert_eq!(progress_rx.recv().await.as_deref(), Some("Image queued..."));
    }

    #[tokio::test]
    async fn cancellation_deletes_specific_prompt() {
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
        Mock::given(method("POST"))
            .and(path("/interrupt"))
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
        let (cancel_tx, cancel_rx) = broadcast::channel(1);
        let (progress_tx, _) = mpsc::unbounded_channel();
        let task =
            tokio::spawn(async move { client.generate("a fox", cancel_rx, progress_tx).await });
        tokio::time::sleep(Duration::from_millis(25)).await;
        cancel_tx.send(()).unwrap();
        assert!(matches!(task.await.unwrap(), Err(ComfyUiError::Cancelled)));
    }
}
