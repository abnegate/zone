//! Byte-range serving for generated media through the real authenticated router.
mod common;

use common::context::Harness;
use common::{test_email, test_password};
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

const CLIP: &[u8] = b"fLaC\x00\x00\x00\x22generated-audio-artifact-payload-0123456789";

struct Artifact {
    harness: Harness,
    root: PathBuf,
    url: String,
}

impl Artifact {
    async fn new(filename: &str, bytes: &[u8]) -> Self {
        let mut harness = Harness::new(Some(32768), false, vec![]).await;
        let root = std::env::temp_dir().join(format!("zone-artifact-range-{}", Uuid::new_v4()));
        let owner = Uuid::new_v4();
        let directory = root
            .join(harness.workspace.to_string())
            .join(harness.chat.to_string())
            .join(owner.to_string());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(filename), bytes).unwrap();
        harness.config.comfyui.artifact_root = root.clone();
        harness.restart().await;
        let url = format!(
            "/api/artifacts/{}/{}/{}/{}",
            harness.workspace, harness.chat, owner, filename
        );
        Self { harness, root, url }
    }
}

impl Drop for Artifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn a_whole_artifact_advertises_byte_ranges() {
    let artifact = Artifact::new("clip.flac", CLIP).await;
    let response = artifact
        .harness
        .client
        .get_auth(&artifact.url, &artifact.harness.token)
        .await;

    response.assert_status(axum::http::StatusCode::OK);
    assert_eq!(response.bytes(), CLIP);
    assert_eq!(
        response.header("accept-ranges"),
        Some("bytes"),
        "an audio artifact Safari can seek must advertise range support on every response"
    );
    assert_eq!(response.header("content-range"), None);
    assert_eq!(response.header("content-type"), Some("audio/flac"));
    assert_eq!(
        response.header("content-length"),
        Some(CLIP.len().to_string().as_str())
    );
    assert_eq!(
        response.header("cache-control"),
        Some("private, max-age=31536000, immutable")
    );
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
}

#[tokio::test]
async fn a_range_request_returns_only_the_requested_bytes() {
    let artifact = Artifact::new("clip.flac", CLIP).await;
    let response = artifact
        .harness
        .client
        .get_range_auth(&artifact.url, "bytes=10-19", &artifact.harness.token)
        .await;

    response.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.bytes(), &CLIP[10..=19]);
    assert_eq!(
        response.header("content-range"),
        Some(format!("bytes 10-19/{}", CLIP.len()).as_str())
    );
    assert_eq!(response.header("content-length"), Some("10"));
    assert_eq!(response.header("accept-ranges"), Some("bytes"));
    assert_eq!(response.header("content-type"), Some("audio/flac"));
    assert_eq!(
        response.header("cache-control"),
        Some("private, max-age=31536000, immutable")
    );
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
}

#[tokio::test]
async fn open_ended_and_suffix_ranges_reach_the_end_of_the_artifact() {
    let artifact = Artifact::new("clip.flac", CLIP).await;
    let total = CLIP.len();

    let open_ended = artifact
        .harness
        .client
        .get_range_auth(&artifact.url, "bytes=32-", &artifact.harness.token)
        .await;
    open_ended.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(open_ended.bytes(), &CLIP[32..]);
    assert_eq!(
        open_ended.header("content-range"),
        Some(format!("bytes 32-{}/{total}", total - 1).as_str())
    );

    let suffix = artifact
        .harness
        .client
        .get_range_auth(&artifact.url, "bytes=-16", &artifact.harness.token)
        .await;
    suffix.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(suffix.bytes(), &CLIP[total - 16..]);
    assert_eq!(
        suffix.header("content-range"),
        Some(format!("bytes {}-{}/{total}", total - 16, total - 1).as_str())
    );

    let past_the_end = artifact
        .harness
        .client
        .get_range_auth(&artifact.url, "bytes=32-99999", &artifact.harness.token)
        .await;
    past_the_end.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(past_the_end.bytes(), &CLIP[32..]);
}

#[tokio::test]
async fn an_unsatisfiable_range_is_rejected_with_the_artifact_length() {
    let artifact = Artifact::new("clip.flac", CLIP).await;
    let response = artifact
        .harness
        .client
        .get_range_auth(
            &artifact.url,
            &format!("bytes={}-", CLIP.len()),
            &artifact.harness.token,
        )
        .await;

    response.assert_status(axum::http::StatusCode::RANGE_NOT_SATISFIABLE);
    assert!(response.bytes().is_empty());
    assert_eq!(
        response.header("content-range"),
        Some(format!("bytes */{}", CLIP.len()).as_str()),
        "a 416 must tell the player how long the artifact actually is"
    );
    assert_eq!(response.header("accept-ranges"), Some("bytes"));
}

#[tokio::test]
async fn a_malformed_range_serves_the_whole_artifact() {
    let artifact = Artifact::new("clip.flac", CLIP).await;
    for range in ["bytes=cheese", "bytes=", "seconds=0-10", "bytes=19-10"] {
        let response = artifact
            .harness
            .client
            .get_range_auth(&artifact.url, range, &artifact.harness.token)
            .await;

        response.assert_status(axum::http::StatusCode::OK);
        assert_eq!(
            response.bytes(),
            CLIP,
            "{range} is unparseable, so the whole artifact must still be served"
        );
        assert_eq!(response.header("accept-ranges"), Some("bytes"));
        assert_eq!(response.header("content-range"), None);
    }
}

#[tokio::test]
async fn range_requests_stay_behind_the_workspace_check() {
    let artifact = Artifact::new("clip.flac", CLIP).await;
    let stranger = artifact
        .harness
        .client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let stranger = stranger["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("registration failed: {stranger}"))
        .to_owned();

    let whole = artifact
        .harness
        .client
        .get_auth(&artifact.url, &stranger)
        .await;
    whole.assert_status(axum::http::StatusCode::NOT_FOUND);

    let partial = artifact
        .harness
        .client
        .get_range_auth(&artifact.url, "bytes=0-9", &stranger)
        .await;
    partial.assert_status(axum::http::StatusCode::NOT_FOUND);
    assert!(
        partial.bytes().is_empty(),
        "a range request from another workspace must not leak artifact bytes"
    );
}
