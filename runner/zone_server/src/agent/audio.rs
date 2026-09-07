//! ComfyUI audio generation as an ordinary tool so the model can stay in the loop.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::{WorkspaceScope, string_arg};
use crate::services::{artifacts::ArtifactStore, comfyui::ComfyUiClient};

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    if !scope.state.config().comfyui.enabled {
        return;
    }
    registry.register(Arc::new(GenerateAudioTool(scope.clone())));
}

struct GenerateAudioTool(WorkspaceScope);

#[async_trait]
impl Tool for GenerateAudioTool {
    fn name(&self) -> &str {
        "generate_audio"
    }

    fn description(&self) -> &str {
        "Generate an audio clip with ComfyUI from a text prompt: music, ambience, or sound \
         effects. Use this instead of describing the sound in words."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What to generate."
                }
            },
            "required": ["prompt"]
        })
    }

    fn mutating(&self) -> bool {
        true
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(
            self.0
                .state
                .config()
                .comfyui
                .audio_generation_timeout_secs
                .saturating_add(30),
        )
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(run_audio(&self.0, params).await)
    }
}

async fn run_audio(scope: &WorkspaceScope, params: Value) -> ToolResult {
    let prompt = match string_arg(&params, "prompt") {
        Ok(prompt) => prompt.to_string(),
        Err(error) => return error,
    };
    let config = scope.state.config().comfyui.clone();
    let client = match ComfyUiClient::new(config.clone()) {
        Ok(client) => client,
        Err(error) => {
            return ToolResult::error(format!("Audio generation is not configured: {error}"));
        }
    };
    let store = ArtifactStore::new(config.artifact_root.clone());

    let (_cancel_tx, mut cancel) = broadcast::channel(1);
    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    let clips = match client
        .generate_audio(&prompt, &mut cancel, progress_tx)
        .await
    {
        Ok(clips) => clips,
        Err(error) => return ToolResult::error(format!("Audio generation failed: {error}")),
    };

    let mut urls = Vec::new();
    for clip in clips {
        match store
            .persist(
                scope.workspace_id,
                scope.chat_id,
                scope.chat_id,
                extension_for(&clip.mime),
                &clip.bytes,
            )
            .await
        {
            Ok(url) => urls.push(url),
            Err(error) => {
                tracing::error!("Failed to persist generated audio: {error}");
                return ToolResult::error("Audio generation failed: could not store the audio");
            }
        }
    }
    if urls.is_empty() {
        return ToolResult::error("Audio generation completed without a usable audio clip");
    }

    let listed = urls
        .iter()
        .enumerate()
        .map(|(index, url)| format!("{}. {}", index + 1, url))
        .collect::<Vec<_>>()
        .join("\n");
    ToolResult::success(format!("Generated {} audio clip(s):\n{listed}", urls.len()))
        .with_images(urls)
}

fn extension_for(mime: &str) -> &'static str {
    match mime {
        "audio/mpeg" => "mp3",
        "audio/opus" => "opus",
        "audio/wav" => "wav",
        _ => "flac",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, test_config};
    use uuid::Uuid;

    fn scope(comfyui_enabled: bool) -> WorkspaceScope {
        let mut config = test_config();
        config.comfyui.enabled = comfyui_enabled;
        let database = sqlx::PgPool::connect_lazy("postgres://localhost/test")
            .expect("a lazy pool needs no server");
        WorkspaceScope {
            state: AppState::new(config, database, None),
            workspace_id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
        }
    }

    #[tokio::test]
    async fn generate_audio_is_registered_only_when_comfyui_is_enabled() {
        let mut disabled = ToolRegistry::new();
        register(&mut disabled, &scope(false));
        assert!(
            disabled.names().is_empty(),
            "audio tool leaked into a workspace with ComfyUI disabled: {:?}",
            disabled.names()
        );

        let mut enabled = ToolRegistry::new();
        register(&mut enabled, &scope(true));
        assert_eq!(enabled.names(), vec!["generate_audio"]);
    }

    #[tokio::test]
    async fn generate_audio_takes_a_required_prompt() {
        let tool = GenerateAudioTool(scope(true));
        assert_eq!(tool.name(), "generate_audio");
        assert!(tool.mutating());

        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["prompt"]["type"], "string");
        assert_eq!(schema["required"], json!(["prompt"]));
    }

    #[tokio::test]
    async fn a_blank_prompt_fails_before_any_comfyui_work() {
        for params in [json!({}), json!({"prompt": "   "})] {
            let result = run_audio(&scope(true), params.clone()).await;
            assert!(!result.success, "{params} should not succeed");
            assert!(
                result.error.unwrap().contains("prompt"),
                "{params} should be rejected for its prompt"
            );
        }
    }

    #[test]
    fn the_stored_extension_follows_the_returned_mime() {
        assert_eq!(extension_for("audio/mpeg"), "mp3");
        assert_eq!(extension_for("audio/opus"), "opus");
        assert_eq!(extension_for("audio/wav"), "wav");
        assert_eq!(extension_for("audio/flac"), "flac");
        assert_eq!(extension_for("application/octet-stream"), "flac");
    }
}
