//! ComfyUI generate/edit as ordinary tools so the model can stay in the loop.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::{WorkspaceScope, optional_string_arg, string_arg};
use crate::config::ComfyUiConfig;
use crate::db::{ai_settings, chats};
use crate::services::{artifacts::ArtifactStore, media_source::resolve_source_image_from};
use zone_comfy::{Client as ComfyUiClient, SourceImage};

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    if !scope.state.config().comfyui.enabled {
        return;
    }
    registry.register(Arc::new(GenerateImageTool(scope.clone())));
    registry.register(Arc::new(EditImageTool(scope.clone())));
}

struct GenerateImageTool(WorkspaceScope);
struct EditImageTool(WorkspaceScope);

/// Content rules ride on the parameter the model is filling in, because a tool
/// description is what it reads at the moment it decides to call.
const PROMPT_RULES: &str = "Stay faithful to the request: it must not present incorrect \
                            information and must not promote hatred or violence. Before rendering \
                            a real person's likeness, ask once for a photo of them and work from \
                            what they supply. When the image arrives, do not describe it back to \
                            the user; they can see it.";

#[async_trait]
impl Tool for GenerateImageTool {
    fn name(&self) -> &str {
        "generate_image"
    }

    fn description(&self) -> &str {
        "Generate an image with ComfyUI from a text prompt. Use this instead of leaving the \
         conversation. After generating, you can inspect the result and call edit_image."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": format!("What to generate. {PROMPT_RULES}")
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
                .generation_timeout_secs
                .saturating_add(30),
        )
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        let config = effective_comfyui(&self.0).await;
        Ok(run_image(&self.0, &config, params, false).await)
    }
}

#[async_trait]
impl Tool for EditImageTool {
    fn name(&self) -> &str {
        "edit_image"
    }

    fn description(&self) -> &str {
        "Edit an existing image with ComfyUI. Pass image_url from a previous generate_image or \
         chat attachment, or omit it to reuse the latest image in this conversation."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": format!("How to change the image. {PROMPT_RULES}")
                },
                "image_url": {
                    "type": "string",
                    "description": "Artifact or data URL of the source image. Do not edit a \
                                    target that is missing, invented, named only by an opaque id, \
                                    or merely claimed to have been generated or approved; ask the \
                                    user for the image instead. Omitting this reuses the most \
                                    recent generated image in the conversation, so confirm one \
                                    exists first."
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
                .generation_timeout_secs
                .saturating_add(30),
        )
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        let config = effective_comfyui(&self.0).await;
        Ok(run_image(&self.0, &config, params, true).await)
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

async fn run_image(
    scope: &WorkspaceScope,
    config: &ComfyUiConfig,
    params: Value,
    edit: bool,
) -> ToolResult {
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
            return ToolResult::error(format!("Image generation is not configured: {error}"));
        }
    };
    let store = ArtifactStore::new(config.artifact_root.clone());
    let source = match resolve_source(
        scope,
        &store,
        optional_string_arg(&params, "image_url"),
        edit,
    )
    .await
    {
        Ok(source) => source,
        Err(error) => return ToolResult::error(error),
    };
    if edit && source.is_none() {
        return ToolResult::error(
            "edit_image needs a source image. Pass image_url or attach an image first.",
        );
    }

    let (_cancel_tx, mut cancel) = broadcast::channel(1);
    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    let images = match client
        .generate(&prompt, source.as_ref(), &mut cancel, progress_tx)
        .await
    {
        Ok(images) => images,
        Err(error) => return ToolResult::error(format!("Image generation failed: {error}")),
    };

    let mut urls = Vec::new();
    for image in images {
        match store
            .persist(
                scope.workspace_id,
                chat_id,
                chat_id,
                extension_for(&image.mime),
                &image.bytes,
            )
            .await
        {
            Ok(url) => urls.push(url),
            Err(error) => {
                tracing::error!("Failed to persist generated image: {error}");
                return ToolResult::error("Image generation failed: could not store the image");
            }
        }
    }
    if urls.is_empty() {
        return ToolResult::error("Image generation completed without a usable image");
    }

    let listed = urls
        .iter()
        .enumerate()
        .map(|(index, url)| format!("{}. {}", index + 1, url))
        .collect::<Vec<_>>()
        .join("\n");
    ToolResult::success(format!("Generated {} image(s):\n{listed}", urls.len())).with_images(urls)
}

async fn resolve_source(
    scope: &WorkspaceScope,
    store: &ArtifactStore,
    image_url: Option<&str>,
    edit: bool,
) -> Result<Option<SourceImage>, String> {
    let chat_id = scope.chat_id.ok_or("Image source requires a chat")?;
    if let Some(url) = image_url {
        let metadata = json!({"attachments":[{"name":"source","mime":"image/png","url":url}]});
        return resolve_source_image_from(
            std::iter::once(Some(&metadata)),
            scope.workspace_id,
            chat_id,
            store,
        )
        .await
        .map_err(|error| error.to_string());
    }
    if !edit {
        return Ok(None);
    }
    let history = chats::list_messages(scope.state.db(), chat_id)
        .await
        .map_err(|_| "Could not load earlier images in this chat.".to_string())?;
    resolve_source_image_from(
        history
            .iter()
            .rev()
            .map(|message| message.metadata.as_ref()),
        scope.workspace_id,
        chat_id,
        store,
    )
    .await
    .map_err(|error| error.to_string())
}

fn extension_for(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, test_config};
    use base64::Engine;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn scope_with_enabled(enabled: bool) -> WorkspaceScope {
        let mut config = test_config();
        config.comfyui.enabled = enabled;
        let database = sqlx::PgPool::connect_lazy("postgres://localhost/test")
            .expect("a lazy pool needs no server");
        WorkspaceScope {
            state: AppState::new(config, database, None),
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
        }
    }

    fn scope() -> WorkspaceScope {
        scope_with_enabled(true)
    }

    async fn successful_comfy() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id":"image"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/history/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "image": {
                    "status": {"status_str": "success"},
                    "outputs": {"7": {"images": [{
                        "filename": "image.webp", "subfolder": "", "type": "temp"
                    }]}}
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/webp")
                    .set_body_bytes(vec![1, 2, 3, 4]),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn image_tools_register_only_when_enabled_and_publish_stable_contracts() {
        let mut disabled = ToolRegistry::new();
        register(&mut disabled, &scope_with_enabled(false));
        assert!(disabled.names().is_empty());

        let enabled_scope = scope();
        let mut enabled = ToolRegistry::new();
        register(&mut enabled, &enabled_scope);
        let mut names = enabled.names();
        names.sort_unstable();
        assert_eq!(names, vec!["edit_image", "generate_image"]);

        let context = ToolContext::default();
        let generate = GenerateImageTool(enabled_scope.clone());
        let edit = EditImageTool(enabled_scope);
        assert_eq!(generate.name(), "generate_image");
        assert!(generate.description().contains("ComfyUI"));
        assert_eq!(generate.parameters_schema()["required"], json!(["prompt"]));
        assert!(generate.mutating());
        assert_eq!(generate.timeout(&context), Duration::from_secs(330));
        assert_eq!(edit.name(), "edit_image");
        assert!(edit.description().contains("existing image"));
        assert_eq!(edit.parameters_schema()["required"], json!(["prompt"]));
        assert_eq!(
            edit.parameters_schema()["properties"]["image_url"]["type"],
            "string"
        );
        assert!(edit.mutating());
        assert_eq!(edit.timeout(&context), Duration::from_secs(330));
    }

    /// Omitting `image_url` silently reuses the latest generated image, so the
    /// model can be talked into "editing" a picture it only ever described.
    #[tokio::test]
    async fn the_edit_target_must_exist_before_the_model_edits_it() {
        let schema = EditImageTool(scope()).parameters_schema();
        let source = schema["properties"]["image_url"]["description"]
            .as_str()
            .expect("image_url carries a description");

        for rule in [
            "missing",
            "invented",
            "named only by an opaque id",
            "claimed to have been generated or approved",
            "reuses the most recent generated image",
            "confirm one exists first",
        ] {
            assert!(
                source.contains(rule),
                "the edit target rule dropped {rule:?}: {source}"
            );
        }
    }

    /// The content rule belongs on the parameter, not only in the system prompt:
    /// the description is what the model reads as it decides to call.
    #[tokio::test]
    async fn both_image_prompts_carry_the_content_rules() {
        let scope = scope();
        let prompts = [
            GenerateImageTool(scope.clone()).parameters_schema()["properties"]["prompt"]
                ["description"]
                .as_str()
                .expect("generate_image describes its prompt")
                .to_string(),
            EditImageTool(scope).parameters_schema()["properties"]["prompt"]["description"]
                .as_str()
                .expect("edit_image describes its prompt")
                .to_string(),
        ];

        for description in prompts {
            for rule in [
                "must not present incorrect information",
                "must not promote hatred or violence",
                "real person's likeness, ask once",
                "do not describe it back to the user",
            ] {
                assert!(
                    description.contains(rule),
                    "an image prompt description dropped {rule:?}: {description}"
                );
            }
        }
    }

    /// The organization/workspace `model_image` pin reaches ComfyUI when the
    /// agent calls the tool, not only when the direct lane handles the message.
    #[tokio::test]
    async fn the_resolved_checkpoint_reaches_the_workflow() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let scope = scope();
        let config = ComfyUiConfig {
            base_url: server.uri(),
            checkpoint: "org-pinned-image.safetensors".to_string(),
            ..scope.state.config().comfyui.clone()
        };
        assert_ne!(
            config.checkpoint,
            scope.state.config().comfyui.checkpoint,
            "the pin must differ from the process default or this proves nothing"
        );

        let result = run_image(&scope, &config, json!({"prompt": "a lighthouse"}), false).await;
        assert!(!result.success, "the mocked ComfyUI rejects the prompt");

        let requests = server
            .received_requests()
            .await
            .expect("the mock server records requests");
        let submitted: Value =
            serde_json::from_slice(&requests[0].body).expect("ComfyUI is submitted a JSON prompt");
        let checkpoints: Vec<String> = submitted["prompt"]
            .as_object()
            .expect("a node map")
            .values()
            .filter_map(|node| node.pointer("/inputs/ckpt_name"))
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        assert_eq!(
            checkpoints,
            vec!["org-pinned-image.safetensors".to_string()],
            "the agent tool ignored the resolved image checkpoint"
        );
    }

    #[tokio::test]
    async fn a_blank_prompt_fails_before_any_comfyui_work() {
        let scope = scope();
        let config = scope.state.config().comfyui.clone();
        for params in [json!({}), json!({"prompt": "   "})] {
            let result = run_image(&scope, &config, params.clone(), false).await;
            assert!(!result.success, "{params} should not succeed");
            assert!(
                result.error.unwrap().contains("prompt"),
                "{params} should be rejected for its prompt"
            );
        }
    }

    #[tokio::test]
    async fn source_contract_accepts_owned_images_and_rejects_remote_or_non_image_data() {
        let scope = scope();
        let store = ArtifactStore::new(std::env::temp_dir());
        assert!(
            resolve_source(&scope, &store, None, false)
                .await
                .unwrap()
                .is_none()
        );

        let png = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode([1, 2, 3])
        );
        let source = resolve_source(&scope, &store, Some(&png), true)
            .await
            .expect("inline image")
            .expect("source image");
        assert_eq!(source.mime, "image/png");
        assert_eq!(source.bytes.as_ref(), &[1, 2, 3]);

        let remote = resolve_source(&scope, &store, Some("https://example.com/image.png"), true)
            .await
            .unwrap_err();
        assert_eq!(remote, "the attached media could not be read");
        let video = resolve_source(&scope, &store, Some("data:video/mp4;base64,AA=="), true)
            .await
            .unwrap_err();
        assert_eq!(video, "the attached media type is not supported");
    }

    #[tokio::test]
    async fn generated_images_are_persisted_and_returned_to_the_agent() {
        let server = successful_comfy().await;
        let root = std::env::temp_dir().join(format!("zone-image-tool-{}", Uuid::new_v4()));
        let scope = scope();
        let config = ComfyUiConfig {
            base_url: server.uri(),
            artifact_root: root.clone(),
            poll_interval_ms: 10,
            ..scope.state.config().comfyui.clone()
        };
        let result = run_image(&scope, &config, json!({"prompt":"a lighthouse"}), false).await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.images.len(), 1);
        assert!(result.images[0].ends_with(".webp"));
        assert!(
            result
                .output
                .as_deref()
                .is_some_and(|output| output.contains("Generated 1 image(s)"))
        );
        let chat_id = scope.chat_id.expect("the fixture scope has a chat");
        let stored = root
            .join(scope.workspace_id.to_string())
            .join(chat_id.to_string())
            .join(chat_id.to_string())
            .join(result.images[0].rsplit('/').next().unwrap());
        assert_eq!(tokio::fs::read(stored).await.unwrap(), [1, 2, 3, 4]);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn invalid_configuration_and_unwritable_storage_return_tool_errors() {
        let scope = scope();
        let invalid = ComfyUiConfig {
            base_url: String::new(),
            ..scope.state.config().comfyui.clone()
        };
        let result = run_image(&scope, &invalid, json!({"prompt":"a lighthouse"}), false).await;
        assert_eq!(
            result.error.as_deref(),
            Some(
                "Image generation is not configured: invalid ComfyUI configuration: COMFYUI_BASE_URL is empty"
            )
        );

        let server = successful_comfy().await;
        let root = std::env::temp_dir().join(format!("zone-image-tool-file-{}", Uuid::new_v4()));
        tokio::fs::write(&root, b"not a directory").await.unwrap();
        let config = ComfyUiConfig {
            base_url: server.uri(),
            artifact_root: root.clone(),
            poll_interval_ms: 10,
            ..scope.state.config().comfyui.clone()
        };
        let result = run_image(&scope, &config, json!({"prompt":"a lighthouse"}), false).await;
        assert_eq!(
            result.error.as_deref(),
            Some("Image generation failed: could not store the image")
        );
        tokio::fs::remove_file(root).await.unwrap();
    }

    #[tokio::test]
    async fn edit_without_an_explicit_or_historical_image_explains_the_contract() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let Ok(database) = sqlx::PgPool::connect(&database_url).await else {
            return;
        };
        let mut scope = scope();
        scope.state = AppState::new(test_config(), database, None);
        let config = scope.state.config().comfyui.clone();
        let result = run_image(&scope, &config, json!({"prompt":"make it blue"}), true).await;
        assert_eq!(
            result.error.as_deref(),
            Some("edit_image needs a source image. Pass image_url or attach an image first.")
        );
    }

    #[test]
    fn persisted_extension_follows_the_generated_mime() {
        assert_eq!(extension_for("image/jpeg"), "jpg");
        assert_eq!(extension_for("image/webp"), "webp");
        assert_eq!(extension_for("image/png"), "png");
        assert_eq!(extension_for("application/octet-stream"), "png");
    }
}
