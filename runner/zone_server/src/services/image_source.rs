//! Turn a chat image attachment into bytes ComfyUI can upload.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::Value;
use uuid::Uuid;

use crate::services::{
    artifacts::ArtifactStore,
    comfyui::{ComfyUiError, SourceImage},
};

#[derive(Debug, thiserror::Error)]
pub enum SourceImageError {
    #[error("the attached image could not be read")]
    Unreadable,
    #[error("the attached image is empty or too large")]
    TooLarge,
    #[error("the attached image type is not supported")]
    UnsupportedType,
    #[error(transparent)]
    Comfy(#[from] ComfyUiError),
}

/// First usable image attachment on the current turn, if any.
pub fn has_image_attachment(metadata: Option<&Value>) -> bool {
    image_attachment_refs(metadata).next().is_some()
}

/// Decode a data URL or load a same-chat artifact. Remote HTTP URLs are
/// ignored so image-to-image cannot be used as an open fetch proxy.
pub async fn resolve_source_image(
    metadata: Option<&Value>,
    workspace_id: Uuid,
    chat_id: Uuid,
    store: &ArtifactStore,
) -> Result<Option<SourceImage>, SourceImageError> {
    resolve_source_image_from(std::iter::once(metadata), workspace_id, chat_id, store).await
}

/// Prefer the current turn's image, then walk earlier messages newest-first.
pub async fn resolve_source_image_from<'a, I>(
    metadata: I,
    workspace_id: Uuid,
    chat_id: Uuid,
    store: &ArtifactStore,
) -> Result<Option<SourceImage>, SourceImageError>
where
    I: IntoIterator<Item = Option<&'a Value>>,
{
    let Some((mime, url)) = metadata
        .into_iter()
        .filter_map(|value| value.and_then(|value| image_attachment_refs(Some(value)).next()))
        .next()
    else {
        return Ok(None);
    };
    if let Some(source) = decode_data_url(url)? {
        return Ok(Some(source));
    }
    if let Some((artifact_workspace, artifact_chat, owner_id, filename)) = parse_artifact_url(url) {
        if artifact_workspace != workspace_id || artifact_chat != chat_id {
            return Err(SourceImageError::Unreadable);
        }
        let bytes = store
            .read(artifact_workspace, artifact_chat, owner_id, &filename)
            .await
            .map_err(|_| SourceImageError::Unreadable)?;
        return Ok(Some(SourceImage::new(bytes, mime)?));
    }
    Err(SourceImageError::Unreadable)
}

impl From<SourceImageError> for ComfyUiError {
    fn from(error: SourceImageError) -> Self {
        match error {
            SourceImageError::Comfy(error) => error,
            SourceImageError::TooLarge => {
                ComfyUiError::Configuration("source image is empty or too large")
            }
            SourceImageError::UnsupportedType => {
                ComfyUiError::Configuration("source image type is not supported")
            }
            SourceImageError::Unreadable => {
                ComfyUiError::Configuration("source image could not be read")
            }
        }
    }
}

fn image_attachment_refs(metadata: Option<&Value>) -> impl Iterator<Item = (&str, &str)> {
    metadata
        .and_then(|value| value.get("attachments"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attachment| {
            let mime = attachment.get("mime").and_then(Value::as_str)?;
            let url = attachment.get("url").and_then(Value::as_str)?;
            (mime.starts_with("image/") && !url.is_empty()).then_some((mime, url))
        })
}

fn decode_data_url(url: &str) -> Result<Option<SourceImage>, SourceImageError> {
    let Some(rest) = url.strip_prefix("data:") else {
        return Ok(None);
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(SourceImageError::Unreadable);
    };
    if !meta
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        return Err(SourceImageError::Unreadable);
    }
    let mime = meta
        .split(';')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(SourceImageError::UnsupportedType)?;
    let bytes = BASE64
        .decode(payload.trim())
        .map_err(|_| SourceImageError::Unreadable)?;
    if bytes.is_empty() {
        return Err(SourceImageError::TooLarge);
    }
    Ok(Some(SourceImage::new(bytes, mime)?))
}

fn parse_artifact_url(url: &str) -> Option<(Uuid, Uuid, Uuid, String)> {
    let path = url.strip_prefix("/api/artifacts/")?;
    let mut parts = path.split('/');
    let workspace_id = Uuid::parse_str(parts.next()?).ok()?;
    let chat_id = Uuid::parse_str(parts.next()?).ok()?;
    let owner_id = Uuid::parse_str(parts.next()?).ok()?;
    let filename = parts.next()?.to_string();
    if filename.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((workspace_id, chat_id, owner_id, filename))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::comfyui::MAX_SOURCE_IMAGE_BYTES;

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xF8,
        0xCF, 0xC0, 0x50, 0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0xA4, 0xA9, 0x8C, 0x21, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn png_data_url() -> String {
        format!("data:image/png;base64,{}", BASE64.encode(PNG_1X1))
    }

    #[test]
    fn detects_image_attachments() {
        assert!(!has_image_attachment(None));
        assert!(!has_image_attachment(Some(&serde_json::json!({
            "attachments": [{"name": "notes.md", "mime": "text/markdown", "url": "https://x"}]
        }))));
        assert!(has_image_attachment(Some(&serde_json::json!({
            "attachments": [{"name": "shot.png", "mime": "image/png", "url": png_data_url()}]
        }))));
    }

    #[tokio::test]
    async fn decodes_png_data_urls_and_rejects_remote_fetches() {
        let store = ArtifactStore::new(std::env::temp_dir().join("unused-img2img"));
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let source = resolve_source_image(
            Some(&serde_json::json!({
                "attachments": [{"name": "shot.png", "mime": "image/png", "url": png_data_url()}]
            })),
            workspace,
            chat,
            &store,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(source.bytes.as_ref(), PNG_1X1);
        assert_eq!(source.mime, "image/png");
        assert!(source.filename.starts_with("zone-img2img-"));
        assert!(source.filename.ends_with(".png"));

        assert!(matches!(
            resolve_source_image(
                Some(&serde_json::json!({
                    "attachments": [{
                        "name": "remote.png",
                        "mime": "image/png",
                        "url": "https://example.test/photo.png"
                    }]
                })),
                workspace,
                chat,
                &store,
            )
            .await,
            Err(SourceImageError::Unreadable)
        ));
    }

    #[tokio::test]
    async fn loads_same_chat_artifacts_and_rejects_cross_chat() {
        let root = std::env::temp_dir().join(format!("zone-img2img-src-{}", Uuid::new_v4()));
        let store = ArtifactStore::new(root.clone());
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let url = store
            .persist(workspace, chat, owner, "png", PNG_1X1)
            .await
            .unwrap();

        let source = resolve_source_image(
            Some(&serde_json::json!({
                "attachments": [{"name": "generated-image-1.png", "mime": "image/png", "url": url}]
            })),
            workspace,
            chat,
            &store,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(source.bytes.as_ref(), PNG_1X1);

        assert!(matches!(
            resolve_source_image(
                Some(&serde_json::json!({
                    "attachments": [{"name": "generated-image-1.png", "mime": "image/png", "url": url}]
                })),
                workspace,
                Uuid::new_v4(),
                &store,
            )
            .await,
            Err(SourceImageError::Unreadable)
        ));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn source_image_rejects_oversize_and_svg() {
        assert!(SourceImage::new(vec![0; MAX_SOURCE_IMAGE_BYTES + 1], "image/png").is_err());
        assert!(SourceImage::new(PNG_1X1.to_vec(), "image/svg+xml").is_err());
    }

    #[tokio::test]
    async fn walks_earlier_messages_when_the_current_turn_has_no_image() {
        let store = ArtifactStore::new(std::env::temp_dir().join("unused-img2img-history"));
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let current = serde_json::json!({ "attachments": [] });
        let earlier = serde_json::json!({
            "attachments": [{"name": "shot.png", "mime": "image/png", "url": png_data_url()}]
        });
        let source =
            resolve_source_image_from([Some(&current), Some(&earlier)], workspace, chat, &store)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(source.bytes.as_ref(), PNG_1X1);
    }
}
