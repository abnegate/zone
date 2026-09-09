//! Direct media generation contracts across the authenticated chat WebSocket.

mod common;

use common::context::{Harness, Socket, finish, next, send};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

static MEDIA: Mutex<()> = Mutex::const_new(());
const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

async fn configure(harness: &mut Harness, comfy: &MockServer, root: &Path) {
    harness.config.comfyui.enabled = true;
    harness.config.comfyui.base_url = comfy.uri();
    harness.config.comfyui.poll_interval_ms = 25;
    harness.config.comfyui.artifact_root = root.to_path_buf();
    harness.restart().await;
}

async fn status(socket: &mut Socket, expected: &str) {
    loop {
        let frame = next(socket).await;
        if frame["type"] == "status" && frame["message"] == expected {
            return;
        }
        assert!(
            !matches!(
                frame["type"].as_str(),
                Some("error" | "cancelled" | "message_end")
            ),
            "expected {expected}: {frame}"
        );
    }
}

fn request(lane: &str) -> (Value, &'static str) {
    match lane {
        "image" => (
            json!({"type":"send","content":"Generate an image", "metadata":{"image_generation":true}}),
            "Preparing image generation...",
        ),
        "video" => (
            json!({"type":"send","content":"Generate a video", "metadata":{"video_generation":true}}),
            "Preparing video generation...",
        ),
        "audio" => (
            json!({"type":"send","content":"Generate audio", "metadata":{"audio_generation":true}}),
            "Preparing audio generation...",
        ),
        "upscale" => (
            json!({
                "type":"send",
                "content":"Upscale this image",
                "metadata":{
                    "upscale":true,
                    "attachments":[{"name":"source.png","mime":"image/png","url":PNG}]
                }
            }),
            "Preparing to upscale the image...",
        ),
        _ => panic!("unsupported test lane {lane}"),
    }
}

fn files(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                files(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[tokio::test]
async fn cancellation_while_waiting_for_capacity_closes_every_media_lane() {
    let _serial = MEDIA.lock().await;
    let occupied = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id":"occupied"})))
        .expect(1)
        .mount(&occupied)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/occupied"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&occupied)
        .await;
    Mock::given(method("POST"))
        .and(path("/queue"))
        .and(body_json(json!({"delete":["occupied"]})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&occupied)
        .await;

    let holder_root = std::env::temp_dir().join(format!("zone-media-holder-{}", Uuid::new_v4()));
    let mut holder = Harness::new(Some(32768), false, vec![]).await;
    configure(&mut holder, &occupied, &holder_root).await;
    let mut holding = holder.connect().await;
    let (message, expected) = request("image");
    send(&mut holding, message).await;
    status(&mut holding, expected).await;
    status(&mut holding, "Image queued...").await;

    for lane in ["image", "video", "audio", "upscale"] {
        let unused = MockServer::start().await;
        let root = std::env::temp_dir().join(format!("zone-media-wait-{lane}-{}", Uuid::new_v4()));
        let mut waiter = Harness::new(Some(32768), false, vec![]).await;
        configure(&mut waiter, &unused, &root).await;
        let mut socket = waiter.connect().await;
        let (message, expected) = request(lane);
        send(&mut socket, message).await;
        status(&mut socket, expected).await;

        send(&mut socket, json!({"type":"cancel"})).await;
        let frames = finish(&mut socket).await;
        let terminal = frames.last().unwrap();
        assert_eq!(terminal["type"], "cancelled", "{lane}: {frames:?}");
        assert!(
            terminal["message_id"].as_str().is_some(),
            "the admitted {lane} turn has an identity: {terminal}"
        );
        assert!(
            unused.received_requests().await.unwrap().is_empty(),
            "capacity cancellation must happen before {lane} submission"
        );
        let replacement = waiter
            .store()
            .acquire(Uuid::new_v4(), Duration::from_secs(30))
            .await
            .unwrap_or_else(|error| panic!("{lane} cancellation retained its lease: {error}"));
        waiter.store().release(&replacement).await.unwrap();
        let chat = waiter
            .client
            .get_auth(&format!("/api/chats/{}", waiter.chat), &waiter.token)
            .await
            .json_value();
        assert!(
            chat["chat"]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|message| message["role"] != "assistant"),
            "cancelled {lane} must not persist an assistant bubble: {chat}"
        );
        assert!(files(&root).is_empty(), "cancelled {lane} left artifacts");
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    send(&mut holding, json!({"type":"cancel"})).await;
    assert_eq!(
        finish(&mut holding).await.last().unwrap()["type"],
        "cancelled"
    );
    occupied.verify().await;
    let _ = tokio::fs::remove_dir_all(holder_root).await;
}

#[tokio::test]
async fn unreadable_sources_fail_closed_before_image_video_or_upscale_submission() {
    let _serial = MEDIA.lock().await;
    let comfy = MockServer::start().await;
    let root = std::env::temp_dir().join(format!("zone-media-source-{}", Uuid::new_v4()));
    let mut harness = Harness::new(Some(32768), false, vec![]).await;
    configure(&mut harness, &comfy, &root).await;
    let mut socket = harness.connect().await;

    for (flag, subject) in [
        ("image_generation", "Image generation"),
        ("video_generation", "Video generation"),
        ("upscale", "Upscaling"),
    ] {
        send(
            &mut socket,
            json!({
                "type":"send",
                "content":"Use this remote source",
                "metadata":{
                    flag:true,
                    "attachments":[{
                        "name":"remote.png",
                        "mime":"image/png",
                        "url":"https://untrusted.example/source.png"
                    }]
                }
            }),
        )
        .await;
        let frames = finish(&mut socket).await;
        let terminal = frames.last().unwrap();
        assert_eq!(terminal["type"], "error", "{subject}: {frames:?}");
        assert_eq!(
            terminal["message"],
            format!("{subject} failed: the attached media could not be read")
        );
        assert!(
            frames.iter().all(|frame| frame["type"] != "message_start"),
            "{subject} source failure created an empty assistant bubble: {frames:?}"
        );
    }

    assert!(
        comfy.received_requests().await.unwrap().is_empty(),
        "unreadable sources must never be uploaded or submitted"
    );
    let chat = harness
        .client
        .get_auth(&format!("/api/chats/{}", harness.chat), &harness.token)
        .await
        .json_value();
    assert!(
        chat["chat"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["role"] != "assistant")
    );
    assert!(files(&root).is_empty());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn video_audio_and_upscale_cancel_submitted_prompts_without_persistence() {
    let _serial = MEDIA.lock().await;
    for lane in ["video", "audio", "upscale"] {
        let comfy = MockServer::start().await;
        let prompt = format!("cancel-{lane}");
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"name":"source.png","subfolder":""})),
            )
            .expect(u64::from(lane == "upscale"))
            .mount(&comfy)
            .await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id":prompt})))
            .expect(1)
            .mount(&comfy)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/history/{prompt}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&comfy)
            .await;
        Mock::given(method("POST"))
            .and(path("/queue"))
            .and(body_json(json!({"delete":[prompt]})))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&comfy)
            .await;

        let root =
            std::env::temp_dir().join(format!("zone-media-active-{lane}-{}", Uuid::new_v4()));
        let mut harness = Harness::new(Some(32768), false, vec![]).await;
        configure(&mut harness, &comfy, &root).await;
        let mut socket = harness.connect().await;
        let (message, expected) = request(lane);
        send(&mut socket, message).await;
        status(&mut socket, expected).await;
        status(
            &mut socket,
            match lane {
                "video" => "Video queued...",
                "audio" => "Audio queued...",
                "upscale" => "Upscale queued...",
                _ => unreachable!(),
            },
        )
        .await;

        send(&mut socket, json!({"type":"cancel"})).await;
        let frames = finish(&mut socket).await;
        let terminal = frames.last().unwrap();
        assert_eq!(terminal["type"], "cancelled", "{lane}: {frames:?}");
        assert!(
            terminal["message_id"].is_null(),
            "media is not announced until it is durably stored: {terminal}"
        );
        assert!(
            frames.iter().all(|frame| !matches!(
                frame["type"].as_str(),
                Some("message_start" | "message_end" | "image" | "video" | "audio")
            )),
            "cancelled {lane} created media or an assistant bubble: {frames:?}"
        );
        let chat = harness
            .client
            .get_auth(&format!("/api/chats/{}", harness.chat), &harness.token)
            .await
            .json_value();
        assert!(
            chat["chat"]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .all(|message| message["role"] != "assistant")
        );
        assert!(files(&root).is_empty(), "cancelled {lane} left artifacts");
        comfy.verify().await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}

#[tokio::test]
async fn lease_loss_after_submission_removes_the_owned_comfy_prompt() {
    let _serial = MEDIA.lock().await;
    let comfy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id":"fenced"})))
        .expect(1)
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/fenced"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&comfy)
        .await;
    Mock::given(method("POST"))
        .and(path("/queue"))
        .and(body_json(json!({"delete":["fenced"]})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&comfy)
        .await;

    let root = std::env::temp_dir().join(format!("zone-media-fenced-{}", Uuid::new_v4()));
    let mut harness = Harness::new(Some(32768), false, vec![]).await;
    configure(&mut harness, &comfy, &root).await;
    let mut socket = harness.connect().await;
    let (message, expected) = request("image");
    send(&mut socket, message).await;
    status(&mut socket, expected).await;
    status(&mut socket, "Image queued...").await;

    sqlx::query(
        "UPDATE chat_leases SET expires_at = clock_timestamp() - interval '1 second' WHERE chat_id = $1",
    )
    .bind(harness.chat)
    .execute(&harness.pool)
    .await
    .unwrap();
    let replacement = harness
        .store()
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .expect("the test must fence the submitted generation");

    let frames = tokio::time::timeout(Duration::from_secs(15), finish(&mut socket))
        .await
        .expect("lease loss must terminate an active media request");
    let terminal = frames.last().unwrap();
    assert_eq!(terminal["type"], "error", "{frames:?}");
    assert!(
        terminal["message"]
            .as_str()
            .unwrap()
            .contains("ownership was lost"),
        "{terminal}"
    );
    assert!(
        frames.iter().all(|frame| !matches!(
            frame["type"].as_str(),
            Some("message_start" | "message_end" | "image")
        )),
        "a fenced media request must not announce output: {frames:?}"
    );
    assert!(files(&root).is_empty());
    comfy.verify().await;
    harness.store().release(&replacement).await.unwrap();
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn video_audio_and_upscale_surface_comfy_failures_without_empty_messages() {
    let _serial = MEDIA.lock().await;
    for (lane, subject) in [
        ("video", "Video generation"),
        ("audio", "Audio generation"),
        ("upscale", "Upscaling"),
    ] {
        let comfy = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"name":"source.png","subfolder":""})),
            )
            .expect(u64::from(lane != "audio"))
            .mount(&comfy)
            .await;
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&comfy)
            .await;

        let root =
            std::env::temp_dir().join(format!("zone-media-failure-{lane}-{}", Uuid::new_v4()));
        let mut harness = Harness::new(Some(32768), false, vec![]).await;
        configure(&mut harness, &comfy, &root).await;
        let mut socket = harness.connect().await;
        let (mut message, _) = request(lane);
        if lane == "video" {
            message["metadata"]["attachments"] = json!([{
                "name":"source.png",
                "mime":"image/png",
                "url":PNG
            }]);
        }
        send(&mut socket, message).await;
        let frames = finish(&mut socket).await;
        let terminal = frames.last().unwrap();
        assert_eq!(terminal["type"], "error", "{lane}: {frames:?}");
        let error = terminal["message"].as_str().unwrap();
        assert!(error.starts_with(&format!("{subject} failed:")), "{error}");
        assert!(error.contains("ComfyUI request failed"), "{error}");
        assert!(
            frames.iter().all(|frame| !matches!(
                frame["type"].as_str(),
                Some("message_start" | "message_end" | "image" | "video" | "audio")
            )),
            "failed {lane} created media or an assistant bubble: {frames:?}"
        );
        assert!(files(&root).is_empty(), "failed {lane} left artifacts");
        comfy.verify().await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}

#[tokio::test]
async fn message_finalization_failure_removes_the_already_stored_artifact() {
    let _serial = MEDIA.lock().await;
    let comfy = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"prompt_id":"finish"})))
        .expect(1)
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/finish"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "finish": {
                "status":{"status_str":"success"},
                "outputs":{"7":{"images":[{
                    "filename":"finish.png","subfolder":"","type":"temp"
                }]}}
            }
        })))
        .mount(&comfy)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(600))
                .insert_header("content-type", "image/png")
                .set_body_bytes(vec![1, 2, 3, 4]),
        )
        .expect(1)
        .mount(&comfy)
        .await;
    Mock::given(method("POST"))
        .and(path("/history"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&comfy)
        .await;

    let root = std::env::temp_dir().join(format!("zone-media-finalize-{}", Uuid::new_v4()));
    let mut harness = Harness::new(Some(32768), false, vec![]).await;
    configure(&mut harness, &comfy, &root).await;
    let mut socket = harness.connect().await;
    let (message, _) = request("image");
    send(&mut socket, message).await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if comfy
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path() == "/view")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("ComfyUI output download must start");

    let turn: Uuid =
        sqlx::query_scalar("SELECT id FROM chat_turns WHERE chat_id = $1 AND status = 'running'")
            .bind(harness.chat)
            .fetch_one(&harness.pool)
            .await
            .unwrap();
    let envelope = format!("finish-contract-{}", Uuid::new_v4());
    sqlx::query(
        "INSERT INTO chat_entries (chat_id,id,turn_id,message,consumed,legacy) VALUES ($1,$2,$3,$4,FALSE,FALSE)",
    )
    .bind(harness.chat)
    .bind(&envelope)
    .bind(turn)
    .bind(json!({"version":1,"role":"assistant","tool_calls":[{
        "id":"unfinished","type":"function","function":{"name":"read_file","arguments":"{}"}
    }]}))
    .execute(&harness.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_calls (chat_id,id,turn_id,envelope_id,mutating) VALUES ($1,'unfinished',$2,$3,FALSE)",
    )
    .bind(harness.chat)
    .bind(turn)
    .bind(&envelope)
    .execute(&harness.pool)
    .await
    .unwrap();

    let frames = finish(&mut socket).await;
    let terminal = frames.last().unwrap();
    assert_eq!(terminal["type"], "error", "{frames:?}");
    assert_eq!(
        terminal["message"],
        "Image generation failed: could not save the message"
    );
    assert!(
        frames.iter().all(|frame| !matches!(
            frame["type"].as_str(),
            Some("message_start" | "message_end" | "image")
        )),
        "failed finalization must not announce the deleted artifact: {frames:?}"
    );
    assert!(
        files(&root).is_empty(),
        "message finalization failure left stored artifacts: {:?}",
        files(&root)
    );
    let chat = harness
        .client
        .get_auth(&format!("/api/chats/{}", harness.chat), &harness.token)
        .await
        .json_value();
    assert!(
        chat["chat"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["role"] != "assistant")
    );
    let replacement = harness
        .store()
        .acquire(Uuid::new_v4(), Duration::from_secs(30))
        .await
        .expect("failed finalization must release generation ownership");
    harness.store().release(&replacement).await.unwrap();
    comfy.verify().await;
    let _ = tokio::fs::remove_dir_all(root).await;
}
