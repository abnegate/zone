//! End-to-end coverage for image LoRA browse, inventory, pull, train, frame
//! extraction, and delete.
mod common;

use axum::{
    Json, Router, body::Body, extract::Request, http::StatusCode, response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::ServiceExt;
use zone_server::auth::create_session_access_token;
use zone_server::config::DEFAULT_HUGGINGFACE_MODELS_URL;
use zone_server::db::{sessions, users};
use zone_server::routes::models::{BrowseQuery, HuggingFaceProvider, ModelMediumFilter, ModelSort};

async fn token(pool: &PgPool, secret: &str) -> String {
    let user = users::create_user(
        pool,
        &common::test_email(),
        "password-hash",
        Some("LoRA test user"),
        false,
    )
    .await
    .unwrap();
    let session = sessions::create_session(
        pool,
        user.id,
        &format!("refresh-{}", uuid::Uuid::new_v4()),
        None,
        None,
        None,
        (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
    )
    .await
    .unwrap();

    create_session_access_token(
        user.id,
        "lora@example.com",
        vec![],
        vec![],
        false,
        session.id,
        secret,
        chrono::Duration::hours(1),
    )
    .unwrap()
}

/// A one-pixel PNG, which is what the dataset writer expects an upload to be.
const TINY_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGPgUbIAAACkAGeY0OCYAAAAAElFTkSuQmCC";

fn ffmpeg_installed() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn temp_models() -> PathBuf {
    let root = std::env::temp_dir().join(format!("zone-lora-e2e-{}", uuid::Uuid::new_v4()));
    for directory in [
        "checkpoints",
        "diffusion_models",
        "loras",
        "text_encoders",
        "vae",
        "training",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    root
}

fn gguf_catalog() -> Value {
    json!([{
        "id": "meta-llama/Llama-3-8B",
        "modelId": "meta-llama/Llama-3-8B",
        "downloads": 1000,
        "likes": 10,
        "tags": ["gguf", "llama"],
        "pipeline_tag": "text-generation",
        "gguf": {"totalFileSize": 4000000000_u64, "architecture": "llama"}
    }])
}

fn adapter_catalog() -> Value {
    json!([{
        "id": "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora",
        "modelId": "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora",
        "downloads": 121588,
        "likes": 40,
        "tags": [
            "lora",
            "base_model:adapter:Qwen/Qwen-Image-Edit-2511",
            "image-to-image"
        ],
        "cardData": {
            "license": "openrail++",
            "base_model": "Qwen/Qwen-Image-Edit-2511",
            "pipeline_tag": "image-to-image"
        },
        "siblings": [{
            "rfilename": "qwen-image-edit-plus-nsfw-lora.safetensors",
            "size": 590058864_u64
        }]
    }])
}

async fn start_catalog(handler: fn(&str) -> Value) -> String {
    let router = Router::new().fallback(move |request: Request| async move {
        let query = request.uri().query().unwrap_or("");
        Json(handler(query)).into_response()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}/api/models")
}

fn split_catalog(query: &str) -> Value {
    if query.contains("adapter") {
        adapter_catalog()
    } else {
        gguf_catalog()
    }
}

async fn router_with(
    ollama: &str,
    catalog: &str,
    models_dir: PathBuf,
    train_command: Option<String>,
) -> (axum::Router, String) {
    router_tuned(ollama, catalog, models_dir, train_command, |_| {}).await
}

async fn router_tuned(
    ollama: &str,
    catalog: &str,
    models_dir: PathBuf,
    train_command: Option<String>,
    tune: impl FnOnce(&mut zone_comfy::Config),
) -> (axum::Router, String) {
    let mut config = common::test_config_with_ollama_host(ollama);
    config.huggingface_models_url = catalog.to_string();
    config.comfyui.models_dir = models_dir;
    config.comfyui.train_command = train_command;
    tune(&mut config.comfyui);
    let pool = common::create_test_pool().await;
    let token = token(&pool, config.jwt_secret()).await;
    (
        common::create_test_router(common::create_test_state(config, pool)),
        token,
    )
}

async fn mock_ollama() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new().route("/api/tags", get(|| async { Json(json!({"models": []})) }));
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
#[ignore] // Requires network access to huggingface.co
async fn live_huggingface_adapter_search_finds_qwen_edit_lora() {
    let provider = HuggingFaceProvider::new(DEFAULT_HUGGINGFACE_MODELS_URL);
    let models = provider
        .search_adapters(
            BrowseQuery {
                query: Some("qwen-image-edit-plus-nsfw-lora"),
                cursor: None,
                limit: 8,
                sort: ModelSort::DownloadsDesc,
                family: None,
                size: Default::default(),
                medium: ModelMediumFilter::ImageGeneration,
            },
            &["Qwen/Qwen-Image-Edit-2511".to_string()],
        )
        .await
        .expect("live HuggingFace adapter search");
    assert!(
        models.iter().any(|model| model
            .name
            .to_ascii_lowercase()
            .contains("qwen-image-edit-plus-nsfw-lora")),
        "expected the Qwen-edit LoRA in {models:?}"
    );
    assert!(models.iter().all(|model| {
        model
            .details
            .as_ref()
            .and_then(|details| details.format.as_deref())
            == Some("lora")
    }));
}

#[tokio::test]
async fn browse_image_generation_returns_loras_not_gguf() {
    let catalog = start_catalog(split_catalog).await;
    let ollama = mock_ollama().await;
    let (router, token) = router_with(&ollama, &catalog, temp_models(), None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models?source=huggingface&medium=image_generation")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: Value = serde_json::from_slice(&body).unwrap();
    let models = result["models"].as_array().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(
        models[0]["name"],
        "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora"
    );
    assert_eq!(models[0]["details"]["format"], "lora");
    assert!(
        models[0]["sizes"][0]["name"]
            .as_str()
            .unwrap()
            .ends_with(".safetensors")
    );
}

#[tokio::test]
async fn browse_huggingface_empty_query_stays_gguf() {
    let catalog = start_catalog(split_catalog).await;
    let ollama = mock_ollama().await;
    let (router, token) = router_with(&ollama, &catalog, temp_models(), None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models?source=huggingface")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let result: Value = serde_json::from_slice(&body).unwrap();
    let models = result["models"].as_array().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["details"]["format"], "gguf");
}

#[tokio::test]
async fn list_models_includes_unready_adapter() {
    let models_dir = temp_models();
    fs::write(
        models_dir.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors"),
        b"lora",
    )
    .unwrap();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir, None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let models: Vec<Value> = serde_json::from_slice(&body).unwrap();
    let adapter = models
        .iter()
        .find(|model| model["name"] == "qwen-image-edit-plus-nsfw-lora.safetensors")
        .expect("adapter row");
    assert_eq!(adapter["completion"], false);
    assert_eq!(adapter["ready"], false);
    assert_eq!(adapter["details"]["format"], "lora");
    assert_eq!(adapter["recipe_id"], "qwen-image-edit-adapter");
}

#[tokio::test]
async fn train_endpoint_writes_lora_and_lists_it() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(
        &ollama,
        &catalog,
        models_dir.clone(),
        Some("printf trained > \"$ZONE_TRAIN_OUTPUT\"".into()),
    )
    .await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/models/train")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "name": "studio-style",
                "base": "flux-schnell",
                "trigger": "ohwx",
                "images": [{
                    "filename": "a.png",
                    "caption": "a portrait",
                    "bytes_base64": TINY_PNG
                }]
            })
            .to_string(),
        ))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let output = models_dir.join("loras/studio-style.safetensors");
    assert_eq!(fs::read(&output).unwrap(), b"trained");

    let list = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(list).await.unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let models: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert!(
        models
            .iter()
            .any(|model| model["name"] == "studio-style.safetensors"
                && model["recipe_id"] == "flux-schnell-adapter")
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn delete_removes_comfy_lora() {
    let models_dir = temp_models();
    let lora = models_dir.join("loras/custom-style.safetensors");
    fs::write(&lora, b"lora").unwrap();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let request = axum::http::Request::builder()
        .method("DELETE")
        .uri("/api/models/custom-style.safetensors")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!lora.exists());
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn comfy_pull_writes_lora_from_hub_origin() {
    let models_dir = temp_models();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let hub = Router::new().fallback(move |request: Request| {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            assert!(
                request
                    .uri()
                    .path()
                    .ends_with("/qwen-image-edit-plus-nsfw-lora.safetensors")
            );
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                Body::from(vec![1, 2, 3, 4]),
            )
                .into_response()
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hub_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, hub).await.unwrap();
    });

    let mut config = common::test_config_with_ollama_host("http://127.0.0.1:9");
    config.huggingface_models_url = format!("http://{hub_addr}/api/models");
    config.comfyui.models_dir = models_dir.clone();
    let pool = common::create_test_pool().await;
    let token = token(&pool, config.jwt_secret()).await;
    let router = common::create_test_router(common::create_test_state(config, pool));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/ws/pull")).await.unwrap();
    socket
        .send(Message::Text(
            json!({"type": "auth", "token": token}).to_string().into(),
        ))
        .await
        .unwrap();
    let auth = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(auth.contains("authenticated"));
    socket
        .send(Message::Text(
            json!({
                "model": "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora:qwen-image-edit-plus-nsfw-lora.safetensors",
                "runtime": "comfy",
                "hf_base": "Qwen/Qwen-Image-Edit-2511"
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let mut complete = false;
    while let Some(Ok(Message::Text(text))) = socket.next().await {
        let event: Value = serde_json::from_str(&text).unwrap();
        if event["type"] == "complete" {
            assert_eq!(event["success"], true);
            complete = true;
            break;
        }
        if event["type"] == "error" {
            panic!("pull failed: {event}");
        }
    }
    assert!(complete);
    assert!(hits.load(Ordering::SeqCst) >= 1);
    let dest = models_dir.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors");
    assert_eq!(fs::read(&dest).unwrap(), vec![1, 2, 3, 4]);
    let sidecar: Value = serde_json::from_slice(
        &fs::read(models_dir.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors.zone.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sidecar["recipe_id"], "qwen-image-edit-adapter");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_turns_a_clip_into_training_images() {
    if !ffmpeg_installed() {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
    use base64::Engine;
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;

    let clip = models_dir.join("clip.mp4");
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
            "2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(built.success(), "could not build the test clip");

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/models/train/frames")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "filename": "clip.mp4",
                "bytes_base64": base64::engine::general_purpose::STANDARD
                    .encode(fs::read(&clip).unwrap()),
                "fps": 3,
                "mirror": true,
            })
            .to_string(),
        ))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();

    let frames = body["frames"].as_array().unwrap();
    assert!(!frames.is_empty(), "the clip produced no training frames");
    assert!(
        body["sampled"].as_u64().unwrap() > frames.len() as u64,
        "sampling runs above the rate the frames are kept at"
    );
    assert!(
        frames.iter().any(|frame| frame["mirrored"] == true),
        "half of each second is mirrored"
    );
    assert!(
        frames.iter().all(|frame| frame["group"].as_u64().is_some()
            && !frame["bytes_base64"].as_str().unwrap().is_empty()),
        "every frame carries its shot and its pixels"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_rejects_a_file_that_is_not_a_video() {
    use base64::Engine;
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/models/train/frames")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "filename": "notes.txt",
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode("not a video"),
            })
            .to_string(),
        ))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a file that holds no video is the caller's problem, not the server's"
    );
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "the failure has to say what went wrong: {body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

async fn post_frames(router: axum::Router, token: &str, body: Value) -> (StatusCode, Value) {
    post_json(router, token, "/api/models/train/frames", body).await
}

async fn post_json(
    router: axum::Router,
    token: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn frames_endpoint_rejects_a_body_that_is_not_base64() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;

    let (status, body) = post_frames(
        router,
        &token,
        json!({ "filename": "clip.mp4", "bytes_base64": "this is not base64!" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("base64")),
        "the failure has to name what was wrong: {body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_rejects_an_empty_clip() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;

    let (status, body) = post_frames(
        router,
        &token,
        json!({ "filename": "clip.mp4", "bytes_base64": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().is_some(), "{body}");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_says_so_when_the_decoder_is_not_installed() {
    use base64::Engine;
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_tuned(&ollama, &catalog, models_dir.clone(), None, |comfyui| {
        comfyui.ffmpeg = "zone-has-no-such-decoder".into();
    })
    .await;

    let (status, body) = post_frames(
        router,
        &token,
        json!({
            "filename": "clip.mp4",
            "bytes_base64": base64::engine::general_purpose::STANDARD.encode("pretend clip"),
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "a missing decoder is the server's problem, not the caller's: {body}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("zone-has-no-such-decoder")),
        "the reply has to name the binary an operator needs to install: {body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_maps_decoder_execution_failures() {
    use base64::Engine;
    let models_dir = temp_models();
    let decoder = models_dir.join("not-executable");
    fs::write(&decoder, "not an executable").unwrap();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_tuned(&ollama, &catalog, models_dir.clone(), None, |comfyui| {
        comfyui.ffmpeg = decoder.display().to_string();
    })
    .await;

    let (status, body) = post_frames(
        router,
        &token,
        json!({
            "filename": "clip.mp4",
            "bytes_base64": base64::engine::general_purpose::STANDARD.encode("pretend clip"),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "the server should preserve the operating-system execution failure: {body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn a_second_training_upload_is_refused_while_one_is_running() {
    use base64::Engine;
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;

    // A decoder that blocks whatever it is passed, so the first request is
    // still holding its permit when the second arrives.
    let stub = models_dir.join("slow-ffmpeg");
    fs::write(&stub, "#!/bin/sh\nsleep 30\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = stub.display().to_string();
    let (router, token) = router_tuned(
        &ollama,
        &catalog,
        models_dir.clone(),
        None,
        move |comfyui| {
            comfyui.ffmpeg = path;
        },
    )
    .await;

    let clip = json!({
        "filename": "clip.mp4",
        "bytes_base64": base64::engine::general_purpose::STANDARD.encode("pretend clip"),
    });
    let holder = tokio::spawn({
        let router = router.clone();
        let token = token.clone();
        let clip = clip.clone();
        async move { post_frames(router, &token, clip).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let (status, body) = post_frames(router, &token, clip).await;
    holder.abort();
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "a training upload holds its body and its decoded bytes at once, so a second has to wait: {body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_needs_authentication() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, _) = router_with(&ollama, &catalog, models_dir.clone(), None).await;

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/models/train/frames")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({ "filename": "clip.mp4", "bytes_base64": "" }).to_string(),
        ))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_bases_lists_flux_when_checkpoint_present() {
    let models_dir = temp_models();
    fs::write(
        models_dir.join("checkpoints/flux1-schnell-fp8.safetensors"),
        b"ckpt",
    )
    .unwrap();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models/train/bases")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let bases: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert!(
        bases
            .iter()
            .any(|base| base["id"] == "flux-schnell" && base["edit"] == false)
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn captions_report_configuration_and_preserve_user_drafts() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (disabled, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let body = json!({
        "trigger": "ohwx",
        "images": [{
            "filename": "portrait.png",
            "bytes_base64": TINY_PNG,
            "caption": "ohwx, side light",
            "group": 4
        }]
    });
    let (status, error) =
        post_json(disabled, &token, "/api/models/train/captions", body.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        error["error"]
            .as_str()
            .is_some_and(|message| message.contains("COMFYUI_CAPTION_MODEL"))
    );

    let (enabled, token) = router_tuned(&ollama, &catalog, models_dir.clone(), None, |comfyui| {
        comfyui.caption_model = "vision".to_string()
    })
    .await;
    let (status, response) = post_json(enabled, &token, "/api/models/train/captions", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["captions"], json!(["ohwx, side light"]));
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_maps_disabled_invalid_and_runner_failures() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let request = json!({
        "name": "studio-style",
        "base": "flux-schnell",
        "trigger": "ohwx",
        "images": [{
            "filename": "a.png",
            "caption": "a portrait",
            "bytes_base64": TINY_PNG
        }]
    });

    let (disabled, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let (status, body) = post_json(disabled, &token, "/api/models/train", request.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(
        body["error"],
        "LoRA training is not configured on this server"
    );

    let (invalid, token) = router_with(
        &ollama,
        &catalog,
        models_dir.clone(),
        Some("printf trained > \"$ZONE_TRAIN_OUTPUT\"".into()),
    )
    .await;
    let mut invalid_request = request.clone();
    invalid_request["name"] = Value::String("../escape".to_string());
    let (status, body) = post_json(invalid, &token, "/api/models/train", invalid_request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );

    let (failed, token) =
        router_with(&ollama, &catalog, models_dir.clone(), Some("exit 7".into())).await;
    let (status, body) = post_json(failed, &token, "/api/models/train", request).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message == "trainer exited 7"),
        "{body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}
