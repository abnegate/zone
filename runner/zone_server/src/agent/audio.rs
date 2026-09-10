//! ComfyUI audio generation as an ordinary tool so the model can stay in the loop.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use zone_core::tools::{Tier, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::{WorkspaceScope, string_arg};
use crate::config::ComfyUiConfig;
use crate::db::ai_settings;
use crate::services::artifacts::ArtifactStore;
use zone_comfy::{Client as ComfyUiClient, MediaType};

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
                    "description": "What to generate. Stay faithful to the request: it must not \
                                    present incorrect information and must not promote hatred or \
                                    violence. Before rendering a real person's likeness, ask once \
                                    for a recording of them and work from what they supply. When \
                                    the clip arrives, do not describe it back to the user; they \
                                    can hear it."
                }
            },
            "required": ["prompt"]
        })
    }

    fn tier(&self) -> Tier {
        Tier::Write
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
        let config = effective_comfyui(&self.0).await;
        Ok(run_audio(&self.0, &config, params).await)
    }
}

async fn effective_comfyui(scope: &WorkspaceScope) -> ComfyUiConfig {
    ai_settings::effective_comfyui(
        scope.state.db(),
        scope.workspace_id,
        &scope.state.config().comfyui,
    )
    .await
}

async fn run_audio(scope: &WorkspaceScope, config: &ComfyUiConfig, params: Value) -> ToolResult {
    let Some(chat_id) = scope.chat_id else {
        return ToolResult::error("Media generation requires a chat");
    };
    let prompt = match string_arg(&params, "prompt") {
        Ok(prompt) => prompt.to_string(),
        Err(error) => return error,
    };
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
                chat_id,
                chat_id,
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
    MediaType::for_mime(mime)
        .filter(MediaType::is_audio)
        .unwrap_or(MediaType::FLAC)
        .extension
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, test_config};
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn scope(comfyui_enabled: bool) -> WorkspaceScope {
        let mut config = test_config();
        config.comfyui.enabled = comfyui_enabled;
        let database = sqlx::PgPool::connect_lazy("postgres://localhost/test")
            .expect("a lazy pool needs no server");
        WorkspaceScope {
            state: AppState::new(config, database, None),
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
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
        assert!(tool.tier().mutating());

        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["prompt"]["type"], "string");
        assert_eq!(schema["required"], json!(["prompt"]));
    }

    /// The content rule belongs on the parameter, not only in the system prompt:
    /// the description is what the model reads as it decides to call.
    #[tokio::test]
    async fn the_audio_prompt_carries_the_content_rules() {
        let schema = GenerateAudioTool(scope(true)).parameters_schema();
        let description = schema["properties"]["prompt"]["description"]
            .as_str()
            .expect("generate_audio describes its prompt");

        for rule in [
            "must not present incorrect information",
            "must not promote hatred or violence",
            "real person's likeness, ask once",
            "do not describe it back to the user",
        ] {
            assert!(
                description.contains(rule),
                "the audio prompt description dropped {rule:?}: {description}"
            );
        }
    }

    #[tokio::test]
    async fn a_blank_prompt_fails_before_any_comfyui_work() {
        let scope = scope(true);
        let config = scope.state.config().comfyui.clone();
        for params in [json!({}), json!({"prompt": "   "})] {
            let result = run_audio(&scope, &config, params.clone()).await;
            assert!(!result.success, "{params} should not succeed");
            assert!(
                result.error.unwrap().contains("prompt"),
                "{params} should be rejected for its prompt"
            );
        }
    }

    /// The organization/workspace `model_audio` pin reaches ComfyUI when the
    /// agent calls the tool, not only when the direct lane handles the message.
    #[tokio::test]
    async fn the_resolved_checkpoint_reaches_the_workflow() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let scope = scope(true);
        let config = ComfyUiConfig {
            enabled: true,
            base_url: server.uri(),
            audio_checkpoint: "org-pinned-audio.safetensors".to_string(),
            ..scope.state.config().comfyui.clone()
        };
        assert_ne!(
            config.audio_checkpoint,
            scope.state.config().comfyui.audio_checkpoint,
            "the pin must differ from the process default or this proves nothing"
        );

        let result = run_audio(&scope, &config, json!({"prompt": "forest ambience"})).await;
        assert!(!result.success, "the mocked ComfyUI rejects the prompt");

        let requests = server
            .received_requests()
            .await
            .expect("the mock server records requests");
        let submitted: Value =
            serde_json::from_slice(&requests[0].body).expect("ComfyUI is submitted a JSON prompt");
        assert_eq!(
            submitted["prompt"]["1"]["inputs"]["ckpt_name"], "org-pinned-audio.safetensors",
            "the agent tool ignored the resolved audio checkpoint"
        );
        assert_eq!(
            submitted["prompt"]["5"]["inputs"]["tags"],
            "forest ambience"
        );
    }

    /// The same guarantee across the settings query the tool now performs: a
    /// `model_audio` stored for the organization reaches `ckpt_name`, driven
    /// through the tool's own entry point.
    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL DATABASE_URL"]
    async fn a_pinned_model_audio_reaches_the_workflow_through_the_tool() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let database =
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL required"))
                .await
                .expect("a migrated database");
        let organization_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, $2, $3)")
            .bind(organization_id)
            .bind("Pinned")
            .bind(organization_id.to_string())
            .execute(&database)
            .await
            .expect("an organization");
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) VALUES ($1, $2, $3, $4)",
        )
        .bind(workspace_id)
        .bind(organization_id)
        .bind("Pinned")
        .bind(workspace_id.to_string())
        .execute(&database)
        .await
        .expect("a workspace");
        sqlx::query(
            "INSERT INTO organization_ai_settings (organization_id, model_audio) VALUES ($1, $2)",
        )
        .bind(organization_id)
        .bind("org-pinned-audio.safetensors")
        .execute(&database)
        .await
        .expect("organization AI settings");

        let mut config = test_config();
        config.comfyui.enabled = true;
        config.comfyui.base_url = server.uri();
        let scope = WorkspaceScope {
            state: AppState::new(config, database.clone(), None),
            workspace_id,
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
        };
        assert_ne!(
            scope.state.config().comfyui.audio_checkpoint,
            "org-pinned-audio.safetensors",
            "the pin must differ from the process default or this proves nothing"
        );

        let context = ToolContext {
            cwd: std::path::PathBuf::from("/"),
            env: std::collections::HashMap::new(),
            max_file_size: 1024 * 1024,
            command_timeout: 30,
            unrestricted: false,
        };
        let result = GenerateAudioTool(scope)
            .execute(json!({"prompt": "forest ambience"}), &context)
            .await
            .expect("the tool reports failure in its result, not as an error");
        assert!(!result.success, "the mocked ComfyUI rejects the prompt");

        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization_id)
            .execute(&database)
            .await
            .expect("cleanup");

        let requests = server
            .received_requests()
            .await
            .expect("the mock server records requests");
        let submitted: Value =
            serde_json::from_slice(&requests[0].body).expect("ComfyUI is submitted a JSON prompt");
        assert_eq!(
            submitted["prompt"]["1"]["inputs"]["ckpt_name"], "org-pinned-audio.safetensors",
            "the agent tool ignored the organization's pinned model_audio"
        );
    }

    #[test]
    fn the_stored_extension_follows_the_returned_mime() {
        assert_eq!(extension_for("audio/mpeg"), "mp3");
        assert_eq!(extension_for("audio/ogg"), "opus");
        assert_eq!(extension_for("audio/opus"), "opus");
        assert_eq!(extension_for("audio/wav"), "wav");
        assert_eq!(extension_for("audio/flac"), "flac");
        assert_eq!(extension_for("image/png"), "flac");
        assert_eq!(extension_for("application/octet-stream"), "flac");
    }
}
