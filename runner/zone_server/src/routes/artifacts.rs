//! Authorized generated-artifact serving.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

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
        Ok(bytes) => {
            let mime = match filename.rsplit_once('.').map(|(_, ext)| ext) {
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("webp") => "image/webp",
                Some("webm") => "video/webm",
                Some("mp4") => "video/mp4",
                _ => "image/png",
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                )
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
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
