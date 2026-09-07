//! End-to-end coverage for image LoRA browse, inventory, pull, train, and delete.
mod common;

use axum::{
    Json, Router, body::Body, extract::Request, http::StatusCode, response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::ServiceExt;
use zone_server::auth::create_access_token;
use zone_server::config::DEFAULT_HUGGINGFACE_MODELS_URL;
use zone_server::routes::models::{BrowseQuery, HuggingFaceProvider, ModelMediumFilter, ModelSort};

fn token(secret: &str) -> String {
    create_access_token(
        uuid::Uuid::new_v4(),
        "lora@example.com",
        vec![],
        vec![],
        false,
        secret,
        chrono::Duration::hours(1),
    )
    .unwrap()
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
    let mut config = common::test_config_with_ollama_host(ollama);
    config.huggingface_models_url = catalog.to_string();
    config.comfyui.models_dir = models_dir;
    config.comfyui.train_command = train_command;
    let secret = config.jwt_secret.clone();
    let pool = PgPoolOptions::new()
        .connect_lazy(&config.database_url)
        .unwrap();
    (
        common::create_test_router(common::create_test_state(config, pool)),
        secret,
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
    let (router, secret) = router_with(&ollama, &catalog, temp_models(), None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models?source=huggingface&medium=image_generation")
        .header("Authorization", format!("Bearer {}", token(&secret)))
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
    let (router, secret) = router_with(&ollama, &catalog, temp_models(), None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models?source=huggingface")
        .header("Authorization", format!("Bearer {}", token(&secret)))
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
    let (router, secret) = router_with(&ollama, &catalog, models_dir, None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models")
        .header("Authorization", format!("Bearer {}", token(&secret)))
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
    let (router, secret) = router_with(
        &ollama,
        &catalog,
        models_dir.clone(),
        Some("printf trained > \"$ZONE_TRAIN_OUTPUT\"".into()),
    )
    .await;
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.encode([137_u8, 80, 78, 71]);
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/models/train")
        .header("Authorization", format!("Bearer {}", token(&secret)))
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "name": "studio-style",
                "base": "flux-schnell",
                "trigger": "ohwx",
                "images": [{
                    "filename": "a.png",
                    "caption": "a portrait",
                    "bytes_base64": png
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
        .header("Authorization", format!("Bearer {}", token(&secret)))
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
    let (router, secret) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let request = axum::http::Request::builder()
        .method("DELETE")
        .uri("/api/models/custom-style.safetensors")
        .header("Authorization", format!("Bearer {}", token(&secret)))
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
    let token = token(&config.jwt_secret);
    let pool = PgPoolOptions::new()
        .connect_lazy(&config.database_url)
        .unwrap();
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
async fn train_bases_lists_flux_when_checkpoint_present() {
    let models_dir = temp_models();
    fs::write(
        models_dir.join("checkpoints/flux1-schnell-fp8.safetensors"),
        b"ckpt",
    )
    .unwrap();
    let ollama = mock_ollama().await;
    let catalog = start_catalog(split_catalog).await;
    let (router, secret) = router_with(&ollama, &catalog, models_dir.clone(), None).await;
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/api/models/train/bases")
        .header("Authorization", format!("Bearer {}", token(&secret)))
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
