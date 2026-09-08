//! Turn a chat attachment into bytes ComfyUI can upload.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::Value;
use uuid::Uuid;

use crate::services::artifacts::ArtifactStore;
use zone_comfy::{Error as ComfyUiError, SourceImage, SourceVideo};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the attached media could not be read")]
    Unreadable,
    #[error("the attached media is empty or too large")]
    TooLarge,
    #[error("the attached media type is not supported")]
    UnsupportedType,
    #[error(transparent)]
    Comfy(#[from] ComfyUiError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Video,
}

impl Kind {
    fn of(mime: &str) -> Option<Self> {
        if mime.starts_with("image/") {
            Some(Self::Image)
        } else if mime.starts_with("video/") {
            Some(Self::Video)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    Image(SourceImage),
    Video(SourceVideo),
}

impl Source {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Image(_) => Kind::Image,
            Self::Video(_) => Kind::Video,
        }
    }

    fn new(bytes: Vec<u8>, mime: &str) -> Result<Self, Error> {
        match Kind::of(mime) {
            Some(Kind::Image) => Ok(Self::Image(SourceImage::new(bytes, mime)?)),
            Some(Kind::Video) => Ok(Self::Video(SourceVideo::new(bytes, mime)?)),
            None => Err(Error::UnsupportedType),
        }
    }
}

/// First usable image attachment on the current turn, if any.
pub fn has_image_attachment(metadata: Option<&Value>) -> bool {
    attachment_refs(metadata, Some(Kind::Image))
        .next()
        .is_some()
}

/// First usable image or video attachment on the current turn, if any.
pub fn has_media_attachment(metadata: Option<&Value>) -> bool {
    any_attachment_refs(metadata).next().is_some()
}

/// Decode a data URL or load a same-chat artifact. Remote HTTP URLs are
/// ignored so image-to-image cannot be used as an open fetch proxy.
pub async fn resolve_source_image(
    metadata: Option<&Value>,
    workspace_id: Uuid,
    chat_id: Uuid,
    store: &ArtifactStore,
) -> Result<Option<SourceImage>, Error> {
    resolve_source_image_from(std::iter::once(metadata), workspace_id, chat_id, store).await
}

/// Prefer the current turn's image, then walk earlier messages newest-first.
pub async fn resolve_source_image_from<'a, I>(
    metadata: I,
    workspace_id: Uuid,
    chat_id: Uuid,
    store: &ArtifactStore,
) -> Result<Option<SourceImage>, Error>
where
    I: IntoIterator<Item = Option<&'a Value>>,
{
    let resolved = resolve_from(metadata, workspace_id, chat_id, store, Some(Kind::Image)).await?;
    match resolved {
        Some(Source::Image(image)) => Ok(Some(image)),
        Some(Source::Video(_)) => Err(Error::UnsupportedType),
        None => Ok(None),
    }
}

/// Newest image or video on the thread. `wanted` takes a whole pass of its own,
/// so naming the kind wins over a newer attachment of the other kind.
pub async fn resolve_source_media_from<'a, I>(
    metadata: I,
    workspace_id: Uuid,
    chat_id: Uuid,
    store: &ArtifactStore,
    wanted: Option<Kind>,
) -> Result<Option<Source>, Error>
where
    I: IntoIterator<Item = Option<&'a Value>>,
{
    let messages: Vec<Option<&Value>> = metadata.into_iter().collect();
    if let Some(kind) = wanted
        && let Some(source) = resolve_from(
            messages.iter().copied(),
            workspace_id,
            chat_id,
            store,
            Some(kind),
        )
        .await?
    {
        return Ok(Some(source));
    }
    resolve_from(messages, workspace_id, chat_id, store, None).await
}

async fn resolve_from<'a, I>(
    metadata: I,
    workspace_id: Uuid,
    chat_id: Uuid,
    store: &ArtifactStore,
    wanted: Option<Kind>,
) -> Result<Option<Source>, Error>
where
    I: IntoIterator<Item = Option<&'a Value>>,
{
    let Some((mime, url)) = metadata
        .into_iter()
        .filter_map(|value| value.and_then(|value| attachment_refs(Some(value), wanted).next()))
        .next()
    else {
        return Ok(None);
    };
    if let Some(source) = decode_data_url(url)? {
        return Ok(Some(source));
    }
    if let Some((artifact_workspace, artifact_chat, owner_id, filename)) = parse_artifact_url(url) {
        if artifact_workspace != workspace_id || artifact_chat != chat_id {
            return Err(Error::Unreadable);
        }
        let bytes = store
            .read(artifact_workspace, artifact_chat, owner_id, &filename)
            .await
            .map_err(|_| Error::Unreadable)?;
        return Ok(Some(Source::new(bytes, mime)?));
    }
    Err(Error::Unreadable)
}

impl From<Error> for ComfyUiError {
    fn from(error: Error) -> Self {
        match error {
            Error::Comfy(error) => error,
            Error::TooLarge => ComfyUiError::Configuration("source media is empty or too large"),
            Error::UnsupportedType => {
                ComfyUiError::Configuration("source media type is not supported")
            }
            Error::Unreadable => ComfyUiError::Configuration("source media could not be read"),
        }
    }
}

fn any_attachment_refs(metadata: Option<&Value>) -> impl Iterator<Item = (&str, &str)> {
    metadata
        .and_then(|value| value.get("attachments"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attachment| {
            let mime = attachment.get("mime").and_then(Value::as_str)?;
            let url = attachment.get("url").and_then(Value::as_str)?;
            (Kind::of(mime).is_some() && !url.is_empty()).then_some((mime, url))
        })
}

fn attachment_refs(
    metadata: Option<&Value>,
    wanted: Option<Kind>,
) -> impl Iterator<Item = (&str, &str)> {
    any_attachment_refs(metadata)
        .filter(move |(mime, _)| wanted.is_none_or(|kind| Kind::of(mime) == Some(kind)))
}

fn decode_data_url(url: &str) -> Result<Option<Source>, Error> {
    let Some(rest) = url.strip_prefix("data:") else {
        return Ok(None);
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(Error::Unreadable);
    };
    if !meta
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        return Err(Error::Unreadable);
    }
    let mime = meta
        .split(';')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(Error::UnsupportedType)?;
    let bytes = BASE64
        .decode(payload.trim())
        .map_err(|_| Error::Unreadable)?;
    if bytes.is_empty() {
        return Err(Error::TooLarge);
    }
    Ok(Some(Source::new(bytes, mime)?))
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
    use zone_comfy::client::{MAX_SOURCE_IMAGE_BYTES, MAX_SOURCE_VIDEO_BYTES};

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xF8,
        0xCF, 0xC0, 0x50, 0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0xA4, 0xA9, 0x8C, 0x21, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    const WEBM: &[u8] = &[0x1A, 0x45, 0xDF, 0xA3, 0x00, 0x00, 0x00, 0x00];

    fn png_data_url() -> String {
        format!("data:image/png;base64,{}", BASE64.encode(PNG_1X1))
    }

    fn attachment(name: &str, mime: &str, url: &str) -> Value {
        serde_json::json!({ "attachments": [{ "name": name, "mime": mime, "url": url }] })
    }

    #[test]
    fn detects_image_attachments() {
        assert!(!has_image_attachment(None));
        assert!(!has_image_attachment(Some(&attachment(
            "notes.md",
            "text/markdown",
            "https://x"
        ))));
        assert!(has_image_attachment(Some(&attachment(
            "shot.png",
            "image/png",
            &png_data_url()
        ))));
    }

    #[test]
    fn video_attachments_are_media_but_not_images() {
        let video = attachment("clip.webm", "video/webm", "/api/artifacts/a/b/c/clip.webm");
        assert!(!has_image_attachment(Some(&video)));
        assert!(has_media_attachment(Some(&video)));
    }

    #[tokio::test]
    async fn decodes_png_data_urls_and_rejects_remote_fetches() {
        let store = ArtifactStore::new(std::env::temp_dir().join("unused-img2img"));
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let source = resolve_source_image(
            Some(&attachment("shot.png", "image/png", &png_data_url())),
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
                Some(&attachment(
                    "remote.png",
                    "image/png",
                    "https://example.test/photo.png"
                )),
                workspace,
                chat,
                &store,
            )
            .await,
            Err(Error::Unreadable)
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
            Some(&attachment("generated-image-1.png", "image/png", &url)),
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
                Some(&attachment("generated-image-1.png", "image/png", &url)),
                workspace,
                Uuid::new_v4(),
                &store,
            )
            .await,
            Err(Error::Unreadable)
        ));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn source_media_rejects_oversize_and_svg() {
        assert!(SourceImage::new(vec![0; MAX_SOURCE_IMAGE_BYTES + 1], "image/png").is_err());
        assert!(SourceImage::new(PNG_1X1.to_vec(), "image/svg+xml").is_err());
        assert!(SourceVideo::new(vec![0; MAX_SOURCE_VIDEO_BYTES + 1], "video/webm").is_err());
        assert!(SourceVideo::new(WEBM.to_vec(), "video/quicktime").is_err());
    }

    #[tokio::test]
    async fn walks_earlier_messages_when_the_current_turn_has_no_image() {
        let store = ArtifactStore::new(std::env::temp_dir().join("unused-img2img-history"));
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let current = serde_json::json!({ "attachments": [] });
        let earlier = attachment("shot.png", "image/png", &png_data_url());
        let source =
            resolve_source_image_from([Some(&current), Some(&earlier)], workspace, chat, &store)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(source.bytes.as_ref(), PNG_1X1);
    }

    #[tokio::test]
    async fn a_clip_loads_from_its_own_chat_and_nowhere_else() {
        let root = std::env::temp_dir().join(format!("zone-media-clip-{}", Uuid::new_v4()));
        let store = ArtifactStore::new(root.clone());
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let url = store
            .persist(workspace, chat, owner, "webm", WEBM)
            .await
            .unwrap();
        let attached = attachment("clip.webm", "video/webm", &url);

        let source = resolve_source_media_from(
            [Some(&attached)],
            workspace,
            chat,
            &store,
            Some(Kind::Video),
        )
        .await
        .unwrap()
        .unwrap();
        let Source::Video(video) = source else {
            panic!("a webm artifact must resolve as a clip");
        };
        assert_eq!(video.bytes.as_ref(), WEBM);
        assert_eq!(video.mime, "video/webm");

        // Another chat's clip is off limits, exactly as another chat's photo is.
        assert!(matches!(
            resolve_source_media_from(
                [Some(&attached)],
                workspace,
                Uuid::new_v4(),
                &store,
                Some(Kind::Video),
            )
            .await,
            Err(Error::Unreadable)
        ));
        // So is anything fetched over the network.
        let remote = attachment("clip.webm", "video/webm", "https://example.test/clip.webm");
        assert!(matches!(
            resolve_source_media_from([Some(&remote)], workspace, chat, &store, None).await,
            Err(Error::Unreadable)
        ));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn image_resolution_skips_a_newer_video() {
        let root = std::env::temp_dir().join(format!("zone-media-skip-{}", Uuid::new_v4()));
        let store = ArtifactStore::new(root.clone());
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let video_url = store
            .persist(workspace, chat, owner, "webm", WEBM)
            .await
            .unwrap();
        let newer = attachment("clip.webm", "video/webm", &video_url);
        let older = attachment("shot.png", "image/png", &png_data_url());

        let source =
            resolve_source_image_from([Some(&newer), Some(&older)], workspace, chat, &store)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(source.bytes.as_ref(), PNG_1X1);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn naming_a_kind_outranks_a_newer_attachment_of_the_other_kind() {
        let root = std::env::temp_dir().join(format!("zone-media-wanted-{}", Uuid::new_v4()));
        let store = ArtifactStore::new(root.clone());
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let video_url = store
            .persist(workspace, chat, owner, "webm", WEBM)
            .await
            .unwrap();
        let newest = attachment("shot.png", "image/png", &png_data_url());
        let older = attachment("clip.webm", "video/webm", &video_url);
        let thread = [Some(&newest), Some(&older)];

        let wanted = resolve_source_media_from(thread, workspace, chat, &store, Some(Kind::Video))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(wanted.kind(), Kind::Video);

        let newest_of_either = resolve_source_media_from(thread, workspace, chat, &store, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(newest_of_either.kind(), Kind::Image);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn a_wanted_kind_that_is_absent_falls_back_to_what_is_there() {
        let store = ArtifactStore::new(std::env::temp_dir().join("unused-media-fallback"));
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let only_image = attachment("shot.png", "image/png", &png_data_url());
        let source = resolve_source_media_from(
            [Some(&only_image)],
            workspace,
            chat,
            &store,
            Some(Kind::Video),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(source.kind(), Kind::Image);
    }
}
