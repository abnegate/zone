//! End-to-end coverage for image LoRA browse, inventory, pull, train, frame
//! extraction, and delete.
mod common;

use axum::{
    Json, Router,
    body::Body,
    extract::Request,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
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

fn ollama_tag(name: &str, parameter_size: &str) -> Value {
    json!({
        "name": name,
        "size": 1,
        "digest": "sha256:test",
        "modified_at": "2024-01-15T10:30:00Z",
        "details": {
            "format": "gguf",
            "family": "llama",
            "parameter_size": parameter_size,
            "quantization_level": "Q4_0"
        }
    })
}

async fn mock_ollama_tags() -> Json<Value> {
    Json(json!({
        "models": [
            ollama_tag("llama3.2:1b", "1B"),
            ollama_tag("llama3.2:3b", "3.2B"),
            ollama_tag("qwen3.8:27b", "27.3B"),
            ollama_tag("qwen3-embedding:0.6b", "0.6B"),
            ollama_tag("llava:7b", "7B")
        ]
    }))
}

async fn mock_ollama_show(Json(payload): Json<Value>) -> Result<Json<Value>, StatusCode> {
    let name = payload["model"]
        .as_str()
        .or_else(|| payload["name"].as_str())
        .unwrap_or("");
    let (capabilities, parameter_size) = match name {
        "llama3.2:1b" => (json!(["completion"]), "1B"),
        "llama3.2:3b" => (json!(["completion"]), "3.2B"),
        "qwen3.8:27b" => (json!(["completion", "vision"]), "27.3B"),
        "qwen3-embedding:0.6b" => (json!(["embedding"]), "0.6B"),
        "llava:7b" => (json!(["vision"]), "7B"),
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok(Json(json!({
        "capabilities": capabilities,
        "details": { "parameter_size": parameter_size }
    })))
}

async fn mock_ollama() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/api/tags", get(mock_ollama_tags))
        .route("/api/show", post(mock_ollama_show));
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

async fn dead_ollama() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

fn plant_image_bases(models_dir: &std::path::Path) {
    fs::write(
        models_dir.join("checkpoints/flux1-schnell-fp8.safetensors"),
        b"ckpt",
    )
    .unwrap();
    fs::write(
        models_dir.join("loras/flux-uncensored.safetensors"),
        b"uncensored",
    )
    .unwrap();
    fs::write(
        models_dir.join("checkpoints/lustifySDXLNSFW_ggwpV7.safetensors"),
        b"people",
    )
    .unwrap();
}

fn finetune(base: &Value) -> bool {
    base.get("finetune")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn complete_language_host_job(models_dir: PathBuf) {
    tokio::spawn(async move {
        for _ in 0..500 {
            if let Some(job) = zone_comfy::host_train::current(&models_dir) {
                let dir = zone_comfy::host_train::job_dir(&models_dir, job.id);
                if dir.join("data/train.jsonl").is_file() {
                    let mut finished = zone_comfy::host_train::read_job(&dir).unwrap();
                    if finished.busy() {
                        finished.status = zone_comfy::host_train::HostStatus::Succeeded;
                        zone_comfy::host_train::write_job(&dir, &finished).unwrap();
                    }
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });
}

const LANGUAGE_JSONL: &[u8] =
    br#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#;

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
    let lora = models_dir.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors");
    fs::write(&lora, b"lora").unwrap();
    fs::write(
        models_dir.join("loras/qwen-image-edit-plus-nsfw-lora.safetensors.zone.json"),
        serde_json::to_vec(&serde_json::json!({
            "recipe_id": "qwen-image-edit-adapter",
            "hf_base": "Qwen/Qwen-Image-Edit-2511"
        }))
        .unwrap(),
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
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let job = wait_train_job(&router, &token).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["filename"], "studio-style.safetensors");
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
    fs::write(
        models_dir.join("loras/custom-style.safetensors.zone.json"),
        serde_json::to_vec(&serde_json::json!({
            "recipe_id": "flux-schnell-adapter",
            "hf_base": "black-forest-labs/FLUX.1-schnell"
        }))
        .unwrap(),
    )
    .unwrap();
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
    if !ffmpeg_installed() {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
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

fn multipart(boundary: &str, fields: &[(&str, Option<&str>, &[u8])]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, filename, bytes) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(filename) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn post_multipart(
    router: axum::Router,
    token: &str,
    uri: &str,
    content_type: String,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", content_type)
        .body(Body::from(body))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get_json(router: axum::Router, token: &str, uri: &str) -> (StatusCode, Value) {
    let request = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    if body.is_empty() {
        return (status, Value::Null);
    }
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get_bytes(
    router: axum::Router,
    token: &str,
    uri: &str,
) -> (StatusCode, bytes::Bytes, Option<String>) {
    let request = axum::http::Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body, content_type)
}

async fn wait_train_job(router: &axum::Router, token: &str) -> Value {
    for _ in 0..200 {
        let (status, body) = get_json(router.clone(), token, "/api/models/train").await;
        if status == StatusCode::NO_CONTENT {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            continue;
        }
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["status"] != "running" {
            return body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("training job did not finish");
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
async fn concurrent_frame_extracts_queue_instead_of_429() {
    use base64::Engine;
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_tuned(&ollama, &catalog, models_dir.clone(), None, |comfyui| {
        comfyui.ffmpeg = "zone-has-no-such-decoder".into();
    })
    .await;

    let clip = json!({
        "filename": "clip.mp4",
        "bytes_base64": base64::engine::general_purpose::STANDARD.encode("pretend clip"),
    });
    let first = tokio::spawn({
        let router = router.clone();
        let token = token.clone();
        let clip = clip.clone();
        async move { post_frames(router, &token, clip).await }
    });
    let second = tokio::spawn({
        let router = router.clone();
        let token = token.clone();
        async move { post_frames(router, &token, clip).await }
    });
    let (first_status, first_body) = first.await.unwrap();
    let (second_status, second_body) = second.await.unwrap();
    assert_ne!(first_status, StatusCode::TOO_MANY_REQUESTS, "{first_body}");
    assert_ne!(
        second_status,
        StatusCode::TOO_MANY_REQUESTS,
        "{second_body}"
    );
    assert_eq!(
        first_status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{first_body}"
    );
    assert_eq!(
        second_status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{second_body}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn frames_endpoint_accepts_a_multipart_clip() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_tuned(&ollama, &catalog, models_dir.clone(), None, |comfyui| {
        comfyui.ffmpeg = "zone-has-no-such-decoder".into();
    })
    .await;

    let (content_type, body) = multipart(
        "ZoneTestBoundary",
        &[
            ("filename", None, b"clip.mp4"),
            ("mirror", None, b"false"),
            ("video", Some("clip.mp4"), b"pretend clip"),
        ],
    );
    let (status, response) = post_multipart(
        router,
        &token,
        "/api/models/train/frames",
        content_type,
        body,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{response}");
    assert!(
        response["error"]
            .as_str()
            .is_some_and(|error| error.contains("zone-has-no-such-decoder")),
        "{response}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_endpoint_accepts_multipart_images() {
    use base64::Engine;
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let png = base64::engine::general_purpose::STANDARD
        .decode(TINY_PNG)
        .unwrap();
    let images = json!([{ "filename": "a.png", "caption": "a portrait" }]).to_string();
    let (content_type, body) = multipart(
        "ZoneTrainBoundary",
        &[
            ("name", None, b"studio-style"),
            ("base", None, b"flux-schnell"),
            ("trigger", None, b"ohwx"),
            ("images", None, images.as_bytes()),
            ("image_0", Some("a.png"), &png),
        ],
    );
    let (status, response) =
        post_multipart(router, &token, "/api/models/train", content_type, body).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{response}");
    assert_eq!(
        response["error"],
        "LoRA training is not configured on this server"
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
    fs::write(
        models_dir.join("loras/flux-uncensored.safetensors"),
        b"uncensored",
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
async fn train_bases_rejects_an_invalid_overlay_without_packaged_fallback() {
    let models_dir = temp_models();
    let workflows = models_dir.join("workflows");
    let recipes = models_dir.join("recipes");
    fs::create_dir(&workflows).unwrap();
    fs::create_dir(&recipes).unwrap();
    fs::write(recipes.join("catalog.json"), b"{\"schema_version\":1}").unwrap();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_tuned(&ollama, &catalog, models_dir.clone(), None, |comfy| {
        comfy.workflow_path = workflows.join("unused.json");
    })
    .await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models/train/bases")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
            .is_some_and(|message| message.contains("no vision model is available"))
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
    let (status, body) = post_json(failed.clone(), &token, "/api/models/train", request).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_train_job(&failed, &token).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"], "trainer exited 7", "{job}");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_is_a_background_job_that_survives_the_request() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let body = json!({
        "name": "studio-style",
        "base": "flux-schnell",
        "trigger": "ohwx",
        "images": [{
            "filename": "a.png",
            "caption": "a portrait",
            "bytes_base64": TINY_PNG
        }]
    });
    let (idle, token) = router_with(
        &ollama,
        &catalog,
        models_dir.clone(),
        Some("sleep 1; printf trained > \"$ZONE_TRAIN_OUTPUT\"".into()),
    )
    .await;
    let (status, none) = get_json(idle.clone(), &token, "/api/models/train").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{none}");

    let (status, started) =
        post_json(idle.clone(), &token, "/api/models/train", body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    assert_eq!(started["status"], "running");
    assert_eq!(started["name"], "studio-style");

    let (status, conflict) = post_json(idle.clone(), &token, "/api/models/train", body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"], "a training job is already running");

    let job = wait_train_job(&idle, &token).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["filename"], "studio-style.safetensors");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_status_reads_a_host_job_when_the_registry_is_empty() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let mut job =
        zone_comfy::host_train::HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
    job.status = zone_comfy::host_train::HostStatus::Running;
    job.step = Some(9);
    job.total = Some(40);
    let dir = zone_comfy::host_train::job_dir(&models_dir, job.id);
    zone_comfy::host_train::write_job(&dir, &job).unwrap();
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let (status, body) = get_json(router, &token, "/api/models/train").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "jerry");
    assert_eq!(body["status"], "running");
    assert_eq!(body["step"], 9);
    assert_eq!(body["total"], 40);
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn a_busy_host_job_refuses_a_second_train() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let job =
        zone_comfy::host_train::HostJob::create("jerry", "finetune", "ohwx", "base.safetensors");
    let dir = zone_comfy::host_train::job_dir(&models_dir, job.id);
    zone_comfy::host_train::write_job(&dir, &job).unwrap();
    let (router, token) = router_with(
        &ollama,
        &catalog,
        models_dir.clone(),
        Some("printf trained > \"$ZONE_TRAIN_OUTPUT\"".into()),
    )
    .await;
    let body = json!({
        "name": "studio-style",
        "base": "flux-schnell",
        "trigger": "ohwx",
        "images": [{
            "filename": "a.png",
            "caption": "a portrait",
            "bytes_base64": TINY_PNG
        }]
    });
    let (status, conflict) = post_json(router, &token, "/api/models/train", body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"], "a training job is already running");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_preview_serves_png_rejects_traversal_and_missing_job() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let mut job =
        zone_comfy::host_train::HostJob::create("jerry", "lora", "ohwx", "base.safetensors");
    job.status = zone_comfy::host_train::HostStatus::Running;
    let dir = zone_comfy::host_train::job_dir(&models_dir, job.id);
    zone_comfy::host_train::write_job(&dir, &job).unwrap();
    let previews = dir.join("previews");
    fs::create_dir_all(&previews).unwrap();
    fs::write(previews.join("step-250-0.png"), b"preview-png").unwrap();
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;

    let (status, body, content_type) = get_bytes(
        router.clone(),
        &token,
        "/api/models/train/previews/step-250-0.png",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{status}");
    assert_eq!(content_type.as_deref(), Some("image/png"));
    assert_eq!(&body[..], b"preview-png");

    let (status, _, _) = get_bytes(
        router.clone(),
        &token,
        "/api/models/train/previews/..%2Fjob.json",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let empty = temp_models();
    let (empty_router, empty_token) = router_with(&ollama, &catalog, empty.clone(), None).await;
    let (status, _, _) = get_bytes(
        empty_router,
        &empty_token,
        "/api/models/train/previews/step-250-0.png",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = fs::remove_dir_all(models_dir);
    let _ = fs::remove_dir_all(empty);
}

fn language_multipart(name: &[u8], base: &[u8], method: &[u8]) -> (String, Vec<u8>) {
    let images = json!([{ "filename": "train.jsonl", "caption": "" }]).to_string();
    multipart(
        "ZoneTrainBoundary",
        &[
            ("name", None, name),
            ("base", None, base),
            ("subject", None, b"language"),
            ("method", None, method),
            ("images", None, images.as_bytes()),
            ("image_0", Some("train.jsonl"), LANGUAGE_JSONL),
        ],
    )
}

#[tokio::test]
async fn train_bases_lists_chat_models_alongside_image_rows() {
    let models_dir = temp_models();
    plant_image_bases(&models_dir);
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let (status, body) = get_json(router, &token, "/api/models/train/bases").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let bases = body.as_array().expect("bases list");
    let flux = bases
        .iter()
        .find(|base| base["id"] == "flux-schnell")
        .expect("image bases remain listed");
    assert_eq!(flux["edit"], false);
    let people = bases
        .iter()
        .find(|base| base["id"] == "sdxl-people")
        .expect("people base is listed when its checkpoint is present");
    assert_eq!(people["subject"], "person");
    assert!(finetune(people));
    let llama = bases
        .iter()
        .find(|base| base["id"] == "llama3.2:1b")
        .expect("small chat models are listed");
    assert_eq!(llama["label"], "llama3.2:1b");
    assert_eq!(llama["subject"], "language");
    assert_eq!(llama["edit"], false);
    assert!(finetune(llama));
    let qwen = bases
        .iter()
        .find(|base| base["id"] == "qwen3.8:27b")
        .expect("completion+vision chat models are listed");
    assert_eq!(qwen["subject"], "language");
    assert!(!finetune(qwen));
    assert!(
        bases.iter().any(|base| base["id"] == "llama3.2:3b"),
        "{bases:?}"
    );
    assert!(
        bases
            .iter()
            .all(|base| base["id"] != "qwen3-embedding:0.6b"),
        "embed-only models are not training bases: {bases:?}"
    );
    assert!(
        bases.iter().all(|base| base["id"] != "llava:7b"),
        "vision-only models are not training bases: {bases:?}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_bases_keep_image_rows_when_ollama_is_down() {
    let models_dir = temp_models();
    plant_image_bases(&models_dir);
    let ollama = dead_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, token) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let (status, body) = get_json(router, &token, "/api/models/train/bases").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let bases = body.as_array().expect("bases list");
    assert!(
        bases
            .iter()
            .any(|base| base["id"] == "flux-schnell" && base["edit"] == false)
    );
    assert!(
        bases.iter().any(|base| base["id"] == "sdxl-people"),
        "{bases:?}"
    );
    assert!(
        bases.iter().all(|base| base["subject"] != "language"
            && !base["id"].as_str().unwrap_or("").contains(':')),
        "chat bases are omitted when Ollama is unreachable: {bases:?}"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_language_multipart_starts_a_host_job() {
    let models_dir = temp_models();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    complete_language_host_job(models_dir.clone());
    let (router, token) = router_with(
        &ollama,
        &catalog,
        models_dir.clone(),
        Some("printf trained > \"$ZONE_TRAIN_OUTPUT\"".into()),
    )
    .await;
    let (content_type, body) = language_multipart(b"support-bot", b"llama3.2:1b", b"lora");
    let (status, response) = post_multipart(
        router.clone(),
        &token,
        "/api/models/train",
        content_type,
        body,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{response}");
    let job = wait_train_job(&router, &token).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    let host = zone_comfy::host_train::current(&models_dir).expect("host job");
    assert_eq!(host.subject, "language");
    assert_eq!(host.checkpoint, "llama3.2:1b");
    assert_eq!(host.method, "lora");
    let dir = zone_comfy::host_train::job_dir(&models_dir, host.id);
    assert!(
        dir.join("data/train.jsonl").is_file(),
        "language dumps are staged as jsonl"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_language_rejects_finetune_on_a_large_chat_model() {
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
    let body = json!({
        "name": "support-bot",
        "base": "qwen3.8:27b",
        "subject": "language",
        "method": "finetune",
        "images": [{
            "filename": "train.jsonl",
            "caption": "",
            "bytes_base64": ""
        }]
    });
    let (status, response) = post_json(router, &token, "/api/models/train", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert_eq!(
        response["error"],
        "fine-tune is only available for small chat models"
    );
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_language_rejects_video() {
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
    let body = json!({
        "name": "support-bot",
        "base": "llama3.2:1b",
        "subject": "language",
        "method": "video",
        "images": [{
            "filename": "train.jsonl",
            "caption": "",
            "bytes_base64": ""
        }]
    });
    let (status, response) = post_json(router, &token, "/api/models/train", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert_eq!(response["error"], "not available for a chat model");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_language_rejects_mixed_document_formats() {
    use base64::Engine;
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
    let chat =
        r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#;
    let body = json!({
        "name": "support-bot",
        "base": "llama3.2:1b",
        "subject": "language",
        "method": "lora",
        "images": [
            {
                "filename": "chat.jsonl",
                "caption": "",
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(chat)
            },
            {
                "filename": "notes.txt",
                "caption": "",
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode("loose")
            }
        ]
    });
    let (status, response) = post_json(router, &token, "/api/models/train", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert_eq!(response["error"], "training documents must be one format");
    let _ = fs::remove_dir_all(models_dir);
}

#[tokio::test]
async fn train_language_rejects_an_unknown_base() {
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
    let body = json!({
        "name": "support-bot",
        "base": "qwen3-embedding:0.6b",
        "subject": "language",
        "method": "lora",
        "images": [{
            "filename": "train.jsonl",
            "caption": "",
            "bytes_base64": ""
        }]
    });
    let (status, response) = post_json(router, &token, "/api/models/train", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert_eq!(response["error"], "unknown training base");
    let _ = fs::remove_dir_all(models_dir);
}
