//! Authorized generated-artifact serving.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;
use zone_comfy::MediaType;

use crate::{
    auth::AuthUser,
    db::{chats, workspace_members},
    services::artifacts::{ArtifactError, ArtifactStore},
    state::AppState,
};

pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, chat_id, owner_id, filename)): Path<(Uuid, Uuid, Uuid, String)>,
) -> Response {
    let Ok(user_id) = auth.0.user_id() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let authorized = match chats::get_chat(state.db(), chat_id).await {
        Ok(Some(chat)) if chat.workspace_id == Some(workspace_id) => {
            workspace_members::can_read(state.db(), workspace_id, user_id)
                .await
                .unwrap_or(false)
        }
        _ => false,
    };
    if !authorized {
        // Do not reveal whether an artifact exists to another workspace.
        return StatusCode::NOT_FOUND.into_response();
    }

    let store = ArtifactStore::new(state.config().comfyui.artifact_root.clone());
    match store.read(workspace_id, chat_id, owner_id, &filename).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type(&filename))
            .header(
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            )
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(ArtifactError::InvalidPath) => StatusCode::BAD_REQUEST.into_response(),
        Err(ArtifactError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => {
            tracing::error!("Failed to read artifact: {}", error);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Artifacts are served with `nosniff`, so this is the only thing standing
/// between a stored clip and a browser that refuses to decode it.
fn content_type(filename: &str) -> &'static str {
    MediaType::for_filename(filename)
        .unwrap_or(MediaType::PNG)
        .mime
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_artifacts_are_served_as_ogg() {
        assert_eq!(
            content_type("f47ac10b.opus"),
            "audio/ogg",
            "audio/opus is an RTP payload type; canPlayType returns \"\" for it"
        );
    }

    #[test]
    fn every_storable_extension_has_a_content_type() {
        for (filename, expected) in [
            ("a.png", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.webp", "image/webp"),
            ("a.webm", "video/webm"),
            ("a.mp4", "video/mp4"),
            ("a.flac", "audio/flac"),
            ("a.mp3", "audio/mpeg"),
            ("a.opus", "audio/ogg"),
            ("a.wav", "audio/wav"),
        ] {
            assert_eq!(content_type(filename), expected, "{filename}");
        }
    }

    #[test]
    fn unknown_and_suffixless_names_fall_back_to_png() {
        assert_eq!(content_type("a.bin"), "image/png");
        assert_eq!(content_type("a"), "image/png");
    }
}
