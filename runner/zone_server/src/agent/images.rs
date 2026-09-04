//! ComfyUI generate/edit as ordinary tools so the model can stay in the loop.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::{WorkspaceScope, optional_string_arg, string_arg};
use crate::db::chats;
use crate::services::{
    artifacts::ArtifactStore,
    comfyui::{ComfyUiClient, SourceImage},
    image_source::resolve_source_image_from,
};

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    if !scope.state.config().comfyui.enabled {
        return;
    }
    registry.register(Arc::new(GenerateImageTool(scope.clone())));
    registry.register(Arc::new(EditImageTool(scope.clone())));
}

struct GenerateImageTool(WorkspaceScope);
struct EditImageTool(WorkspaceScope);

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
                .generation_timeout_secs
                .saturating_add(30),
        )
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(run_image(&self.0, params, false).await)
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
                    "description": "How to change the image."
                },
                "image_url": {
                    "type": "string",
                    "description": "Artifact or data URL of the source image."
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
        Ok(run_image(&self.0, params, true).await)
    }
}

async fn run_image(scope: &WorkspaceScope, params: Value, edit: bool) -> ToolResult {
    let prompt = match string_arg(&params, "prompt") {
        Ok(prompt) => prompt.to_string(),
        Err(error) => return error,
    };
    let config = scope.state.config().comfyui.clone();
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
                scope.chat_id,
                scope.chat_id,
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
    if let Some(url) = image_url {
        let metadata = json!({"attachments":[{"name":"source","mime":"image/png","url":url}]});
        return resolve_source_image_from(
            std::iter::once(Some(&metadata)),
            scope.workspace_id,
            scope.chat_id,
            store,
        )
        .await
        .map_err(|error| error.to_string());
    }
    if !edit {
        return Ok(None);
    }
    let history = chats::list_messages(scope.state.db(), scope.chat_id)
        .await
        .map_err(|_| "Could not load earlier images in this chat.".to_string())?;
    resolve_source_image_from(
        history
            .iter()
            .rev()
            .map(|message| message.metadata.as_ref()),
        scope.workspace_id,
        scope.chat_id,
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
