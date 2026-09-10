//! Upload boundaries: the name a client attaches to a clip is a label, not a
//! destination, and the bytes are what decide whether it is media at all.
mod common;

use axum::http::StatusCode;
use base64::Engine;
use common::{TestClient, test_config, test_email, test_password};
use serde_json::json;
use uuid::Uuid;

async fn registered(client: &TestClient) -> String {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    registered["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("registration failed: {registered}"))
        .to_string()
}

/// The frame extractor takes the container format from the submitted name, so
/// the name is the one part of a video upload an attacker fully controls.
#[tokio::test]
async fn a_submitted_clip_name_cannot_choose_where_the_clip_lands() {
    let root = std::env::temp_dir().join(format!("zone-boundary-upload-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut config = test_config();
    config.comfyui.models_dir = root.clone();
    let client = TestClient::with_config(config).await;
    let token = registered(&client).await;

    let planted = root.join("planted.mp4");
    let escapes = [
        format!("..{}planted.mp4", std::path::MAIN_SEPARATOR),
        "../../../../../../../../".to_string() + planted.to_str().unwrap(),
        planted.to_str().unwrap().to_string(),
        "clip.mp4\u{0000}.sh".to_string(),
        format!("clip.{}", "a".repeat(4096)),
        "clip.%2e%2e%2fetc%2fpasswd".to_string(),
        ".".repeat(300),
        String::new(),
    ];
    for filename in escapes {
        let response = client
            .post_json_auth(
                "/api/models/train/frames",
                &json!({
                    "filename": filename,
                    "bytes_base64": base64::engine::general_purpose::STANDARD
                        .encode(b"not a video at all"),
                }),
                &token,
            )
            .await;
        assert_ne!(
            response.status,
            StatusCode::OK,
            "{filename:?} was accepted as a decodable clip: {}",
            response.text()
        );
        assert!(
            !planted.exists(),
            "{filename:?} placed a clip at {}",
            planted.display()
        );
    }
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        0,
        "a refused upload must leave nothing behind in the models directory"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// An empty body is refused before any decoder runs, and a body that is not
/// base64 is refused before it is written anywhere.
#[tokio::test]
async fn a_clip_that_is_not_media_is_refused() {
    let client = TestClient::with_db().await;
    let token = registered(&client).await;

    for body in [
        json!({"filename": "clip.mp4", "bytes_base64": ""}),
        json!({"filename": "clip.mp4", "bytes_base64": "!!!not base64!!!"}),
        json!({"filename": "clip.mp4", "bytes_base64": "PD9waHAgc3lzdGVtKCQxKTs/Pg=="}),
    ] {
        let response = client
            .post_json_auth("/api/models/train/frames", &body, &token)
            .await;
        assert_ne!(
            response.status,
            StatusCode::OK,
            "{body} was accepted as a clip: {}",
            response.text()
        );
    }
}
