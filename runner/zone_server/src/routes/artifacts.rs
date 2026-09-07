//! Authorized generated-artifact serving.

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    db::{chats, workspace_members},
    services::{
        artifact_access::{self, Location},
        artifacts::{ArtifactError, ArtifactStore},
    },
    state::AppState,
};

#[derive(Deserialize)]
pub struct Access {
    expires: Option<i64>,
    signature: Option<String>,
}

pub async fn get(
    State(state): State<AppState>,
    auth: Option<AuthUser>,
    headers: HeaderMap,
    Query(access): Query<Access>,
    Path((workspace_id, chat_id, owner_id, filename)): Path<(Uuid, Uuid, Uuid, String)>,
) -> Response {
    let location = Location {
        workspace_id,
        chat_id,
        owner_id,
        filename: &filename,
    };
    let authorized = match (access.expires, access.signature.as_deref()) {
        (Some(expires), Some(signature)) => artifact_access::verify(
            state.config().jwt_secret(),
            location,
            expires,
            signature,
            Utc::now().timestamp(),
        ),
        _ => {
            let Some(user_id) = auth.and_then(|auth| auth.0.user_id().ok()) else {
                return StatusCode::UNAUTHORIZED.into_response();
            };
            readable(&state, workspace_id, chat_id, user_id).await
        }
    };
    if !authorized {
        // Do not reveal whether an artifact exists to another workspace.
        return StatusCode::NOT_FOUND.into_response();
    }

    let store = ArtifactStore::new(state.config().comfyui.artifact_root.clone());
    let artifact = match store.open(workspace_id, chat_id, owner_id, &filename).await {
        Ok(artifact) => artifact,
        Err(ArtifactError::InvalidPath) => return StatusCode::BAD_REQUEST.into_response(),
        Err(ArtifactError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => {
            tracing::error!("Failed to read artifact: {}", error);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let total = artifact.length();
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (status, start, length, content_range) = match requested(range, total) {
        Requested::Whole => (StatusCode::OK, 0, total, None),
        Requested::Partial { start, end } => (
            StatusCode::PARTIAL_CONTENT,
            start,
            end - start + 1,
            Some(format!("bytes {start}-{end}/{total}")),
        ),
        Requested::Unsatisfiable => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    };

    let bytes = match artifact.read(start, length).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!("Failed to read artifact: {}", error);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mime = match filename.rsplit_once('.').map(|(_, extension)| extension) {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("webm") => "video/webm",
        Some("mp4") => "video/mp4",
        _ => "image/png",
    };
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .header(
            header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if let Some(content_range) = content_range {
        response = response.header(header::CONTENT_RANGE, content_range);
    }
    response
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub async fn signature(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((workspace_id, chat_id, owner_id, filename)): Path<(Uuid, Uuid, Uuid, String)>,
) -> Response {
    let Ok(user_id) = auth.0.user_id() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !readable(&state, workspace_id, chat_id, user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }

    let expires = Utc::now().timestamp() + artifact_access::LIFETIME_SECONDS;
    let url = artifact_access::signed_url(
        state.config().jwt_secret(),
        Location {
            workspace_id,
            chat_id,
            owner_id,
            filename: &filename,
        },
        expires,
    );
    Json(json!({"url": url, "expires_at": expires})).into_response()
}

async fn readable(state: &AppState, workspace_id: Uuid, chat_id: Uuid, user_id: Uuid) -> bool {
    match chats::get_chat(state.db(), chat_id).await {
        Ok(Some(chat)) if chat.workspace_id == Some(workspace_id) => {
            workspace_members::can_read(state.db(), workspace_id, user_id)
                .await
                .unwrap_or(false)
        }
        _ => false,
    }
}

enum Requested {
    Whole,
    Partial { start: u64, end: u64 },
    Unsatisfiable,
}

/// RFC 9110 §14.1: a header a server cannot parse is ignored rather than
/// rejected, so every malformed spelling falls back to the whole body.
fn requested(header: Option<&str>, total: u64) -> Requested {
    let Some((unit, specifier)) = header.and_then(|header| header.split_once('=')) else {
        return Requested::Whole;
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") || specifier.contains(',') {
        return Requested::Whole;
    }
    let Some((first, last)) = specifier.trim().split_once('-') else {
        return Requested::Whole;
    };
    match (first.trim(), last.trim()) {
        ("", suffix) => match suffix.parse::<u64>() {
            Err(_) => Requested::Whole,
            Ok(0) => Requested::Unsatisfiable,
            Ok(_) if total == 0 => Requested::Unsatisfiable,
            Ok(suffix) => Requested::Partial {
                start: total.saturating_sub(suffix),
                end: total - 1,
            },
        },
        (first, "") => match first.parse::<u64>() {
            Ok(start) if start < total => Requested::Partial {
                start,
                end: total - 1,
            },
            Ok(_) => Requested::Unsatisfiable,
            Err(_) => Requested::Whole,
        },
        (first, last) => match (first.parse::<u64>(), last.parse::<u64>()) {
            (Ok(start), Ok(end)) if end < start => Requested::Whole,
            (Ok(start), Ok(_)) if start >= total => Requested::Unsatisfiable,
            (Ok(start), Ok(end)) => Requested::Partial {
                start,
                end: end.min(total - 1),
            },
            _ => Requested::Whole,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{Requested, requested};

    fn resolved(header: &str, total: u64) -> Option<(u64, u64)> {
        match requested(Some(header), total) {
            Requested::Partial { start, end } => Some((start, end)),
            Requested::Whole => None,
            Requested::Unsatisfiable => panic!("{header} over {total} bytes must be satisfiable"),
        }
    }

    fn unsatisfiable(header: &str, total: u64) -> bool {
        matches!(requested(Some(header), total), Requested::Unsatisfiable)
    }

    #[test]
    fn absent_range_serves_the_whole_body() {
        assert!(matches!(requested(None, 100), Requested::Whole));
    }

    #[test]
    fn closed_ranges_clamp_to_the_final_byte() {
        assert_eq!(resolved("bytes=0-99", 100), Some((0, 99)));
        assert_eq!(resolved("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(resolved("bytes=0-0", 100), Some((0, 0)));
        assert_eq!(resolved("bytes=50-4096", 100), Some((50, 99)));
        assert_eq!(resolved("bytes=99-99", 100), Some((99, 99)));
    }

    #[test]
    fn open_ended_ranges_run_to_the_final_byte() {
        assert_eq!(resolved("bytes=0-", 100), Some((0, 99)));
        assert_eq!(resolved("bytes=64-", 100), Some((64, 99)));
        assert_eq!(resolved("bytes=99-", 100), Some((99, 99)));
    }

    #[test]
    fn suffix_ranges_count_back_from_the_end() {
        assert_eq!(resolved("bytes=-10", 100), Some((90, 99)));
        assert_eq!(resolved("bytes=-1", 100), Some((99, 99)));
        assert_eq!(resolved("bytes=-100", 100), Some((0, 99)));
        assert_eq!(resolved("bytes=-4096", 100), Some((0, 99)));
    }

    #[test]
    fn ranges_beyond_the_artifact_are_unsatisfiable() {
        assert!(unsatisfiable("bytes=100-", 100));
        assert!(unsatisfiable("bytes=100-200", 100));
        assert!(unsatisfiable("bytes=-0", 100));
        assert!(unsatisfiable("bytes=0-", 0));
        assert!(unsatisfiable("bytes=-1", 0));
    }

    #[test]
    fn unparseable_ranges_fall_back_to_the_whole_body() {
        for header in [
            "bytes=abc",
            "bytes=",
            "bytes=-",
            "bytes=1-abc",
            "bytes=abc-1",
            "bytes=99-10",
            "bytes=99999999999999999999999-",
            "items=0-10",
            "0-10",
            "bytes 0-10",
        ] {
            assert!(
                matches!(requested(Some(header), 100), Requested::Whole),
                "{header} is not a range this handler can honour, so it must serve the whole body"
            );
        }
    }

    #[test]
    fn multiple_ranges_serve_the_whole_body() {
        assert!(matches!(
            requested(Some("bytes=0-9,20-29"), 100),
            Requested::Whole
        ));
    }

    #[test]
    fn the_range_unit_is_case_insensitive() {
        assert_eq!(resolved("BYTES=0-9", 100), Some((0, 9)));
        assert_eq!(resolved("Bytes=0-9", 100), Some((0, 9)));
    }
}
