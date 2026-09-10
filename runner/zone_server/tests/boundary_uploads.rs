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

fn ffmpeg_installed() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// A clip the decoder actually accepts. Submitting bytes that are not media
/// makes every traversal attempt fail on the bytes, so the name is never
/// reached and the assertion below holds no matter what the name guard does.
fn clip_bytes(root: &std::path::Path) -> Vec<u8> {
    let clip = root.join("source.mp4");
    let built = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=30",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&clip)
        .status()
        .expect("ffmpeg runs");
    assert!(built.success(), "could not build the test clip");
    let bytes = std::fs::read(&clip).expect("the clip is readable");
    std::fs::remove_file(&clip).expect("the source clip is removed");
    bytes
}

/// The submitted name is a label, not a destination: the extractor reads an
/// extension off it and writes `clip.<extension>` into a fresh temporary
/// directory. So a hostile name is *accepted* -- what must hold is that no
/// byte of it reaches a path.
///
/// Submitting bytes that are not media hides all of this: every name is then
/// refused on the bytes, before the name is looked at, and the test passes
/// whatever the name guard does.
#[tokio::test]
async fn a_submitted_clip_name_cannot_choose_where_the_clip_lands() {
    let root = std::env::temp_dir().join(format!("zone-boundary-upload-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut config = test_config();
    config.comfyui.models_dir = root.clone();
    let client = TestClient::with_config(config).await;
    let token = registered(&client).await;

    if !ffmpeg_installed() {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
    let clip = clip_bytes(&root);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&clip);

    // Positive control: the same bytes under a name nobody objects to must be
    // accepted, or a refusal below says nothing about the name.
    let accepted = client
        .post_json_auth(
            "/api/models/train/frames",
            &json!({ "filename": "clip.mp4", "bytes_base64": encoded, "fps": 1 }),
            &token,
        )
        .await;
    assert_eq!(
        accepted.status,
        StatusCode::OK,
        "the fixture clip must be accepted, or the names below prove nothing: {}",
        accepted.text()
    );
    for entry in std::fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap().path();
        if entry.is_dir() {
            std::fs::remove_dir_all(&entry).unwrap();
        } else {
            std::fs::remove_file(&entry).unwrap();
        }
    }

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
                &json!({ "filename": filename, "bytes_base64": encoded, "fps": 1 }),
                &token,
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{filename:?} was refused, so it never reached the name handling \
             this pins: {}",
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
