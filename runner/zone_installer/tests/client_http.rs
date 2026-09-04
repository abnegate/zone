//! HTTP integration tests for the Zone client on desktop, Android, and iOS.
//!
//! These hit the same `ServeKind::Desktop` router the Tauri app embeds. Config
//! paths are the only platform difference, so each lifecycle is run against the
//! desktop, Android, and iOS locations.

use std::fs;
use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;
use zone_installer::frontend::{self, AppMode};
use zone_installer::serve::AppState;
use zone_installer::{ServeKind, router};

#[derive(Clone, Copy, Debug)]
enum Platform {
    Desktop,
    Android,
    Ios,
}

impl Platform {
    fn config_path(self, root: &Path) -> PathBuf {
        match self {
            Self::Desktop => root.join("home/.zone/config.toml"),
            Self::Android => root.join("data/user/0/com.abnegate.zone/files/config.toml"),
            Self::Ios => root.join("Library/Application Support/com.abnegate.zone/config.toml"),
        }
    }
}

fn write_manager(root: &Path) -> PathBuf {
    let manager = root.join("manager");
    fs::create_dir_all(manager.join("assets")).unwrap();
    fs::write(
        manager.join("index.html"),
        "<!DOCTYPE html><html><body>manager-spa</body></html>",
    )
    .unwrap();
    fs::write(manager.join("assets/app.js"), "window.ZONE=1;").unwrap();
    manager
}

async fn desktop_app(manager: PathBuf, config_path: PathBuf, proxy_target: String) -> Router {
    let configured = frontend::is_configured_at(&config_path);
    let state = AppState::desktop(manager, proxy_target)
        .expect("http client")
        .with_config_path(config_path);
    if !configured {
        state.set_mode(AppMode::Setup);
    }
    router(ServeKind::Desktop, state)
}

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let req = builder
        .body(
            body.map(|value| Body::from(value.to_owned()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn start_upstream() -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new()
        .route("/api/health", axum::routing::get(|| async { "ok" }))
        .route(
            "/api/echo",
            axum::routing::post(|body: String| async move { body }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

async fn run_lifecycle(platform: Platform) {
    let root = tempfile::tempdir().unwrap();
    let manager = write_manager(root.path());
    let config_path = platform.config_path(root.path());
    let (upstream, upstream_task) = start_upstream().await;
    let app = desktop_app(
        manager,
        config_path.clone(),
        "https://manager.localhost".into(),
    )
    .await;

    let (status, body) = request(&app, "GET", "/", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert!(
        body.contains("Connect this app to your Zone server"),
        "{platform:?}"
    );
    assert!(body.contains("Android and iOS"), "{platform:?}");

    let (status, body) = request(&app, "GET", "/__zone/info", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["client"], json!(true), "{platform:?}");
    assert_eq!(
        info["host"],
        json!("https://manager.localhost"),
        "{platform:?}"
    );

    let (status, body) = request(&app, "POST", "/api/setup", Some(r#"{"host":"not-a-url"}"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{platform:?}");
    assert!(
        body.contains("http://") || body.contains("https://"),
        "{platform:?}"
    );
    assert!(
        !config_path.exists(),
        "{platform:?} must not write on invalid setup"
    );

    let (status, _) = request(&app, "POST", "/api/setup", Some(r#"{"host":""}"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{platform:?}");

    let payload = json!({ "host": format!("{upstream}/") }).to_string();
    let (status, body) = request(&app, "POST", "/api/setup", Some(&payload)).await;
    assert_eq!(status, StatusCode::OK, "{platform:?} {body}");
    assert_eq!(body, json!({ "ok": true }).to_string());
    assert_eq!(
        frontend::configured_host_from(&config_path).as_deref(),
        Some(upstream.as_str()),
        "{platform:?}"
    );

    let (status, body) = request(&app, "GET", "/", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert!(body.contains("manager-spa"), "{platform:?}");

    let (status, body) = request(&app, "GET", "/assets/app.js", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert_eq!(body, "window.ZONE=1;");

    let (status, body) = request(&app, "GET", "/chats", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert!(body.contains("manager-spa"), "{platform:?}");

    let (status, body) = request(&app, "GET", "/__zone/info", None).await;
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert_eq!(info["host"], json!(upstream), "{platform:?}");

    let (status, body) = request(&app, "GET", "/api/health", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert_eq!(body, "ok", "{platform:?}");

    let (status, body) = request(&app, "POST", "/api/echo", Some("hello-zone")).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert_eq!(body, "hello-zone", "{platform:?}");

    let (status, body) = request(&app, "GET", "/__zone/change-server", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert!(
        body.contains("Connect this app to your Zone server"),
        "{platform:?}"
    );

    let (status, body) = request(&app, "GET", "/", None).await;
    assert_eq!(status, StatusCode::OK, "{platform:?}");
    assert!(
        body.contains("Connect this app to your Zone server"),
        "{platform:?}"
    );

    upstream_task.abort();
}

#[tokio::test]
async fn desktop_client_lifecycle() {
    run_lifecycle(Platform::Desktop).await;
}

#[tokio::test]
async fn android_client_lifecycle() {
    run_lifecycle(Platform::Android).await;
}

#[tokio::test]
async fn ios_client_lifecycle() {
    run_lifecycle(Platform::Ios).await;
}

#[tokio::test]
async fn already_configured_skips_setup() {
    let root = tempfile::tempdir().unwrap();
    let manager = write_manager(root.path());
    let config_path = root.path().join("home/.zone/config.toml");
    frontend::write_host_to(&config_path, "https://zone.example.com").unwrap();
    let app = desktop_app(manager, config_path, "https://zone.example.com".into()).await;

    let (status, body) = request(&app, "GET", "/", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("manager-spa"));
}

#[tokio::test]
async fn missing_frontend_returns_not_found_in_console_mode() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing-manager");
    let config_path = root.path().join("config.toml");
    frontend::write_host_to(&config_path, "https://zone.example.com").unwrap();
    let app = desktop_app(missing, config_path, "https://zone.example.com".into()).await;

    let (status, body) = request(&app, "GET", "/", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Frontend not found"));
}

#[tokio::test]
async fn setup_write_failure_is_internal_error() {
    let root = tempfile::tempdir().unwrap();
    let manager = write_manager(root.path());
    let blocked = root.path().join("blocked-dir");
    fs::create_dir_all(&blocked).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&blocked).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&blocked, perms).unwrap();
    }
    let config_path = blocked.join("nested/config.toml");
    let app = desktop_app(manager, config_path, "https://manager.localhost".into()).await;

    let (status, body) = request(
        &app,
        "POST",
        "/api/setup",
        Some(r#"{"host":"https://zone.example.com"}"#),
    )
    .await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&blocked).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&blocked, perms).unwrap();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("Could not save server URL"));
    }
    #[cfg(not(unix))]
    {
        let _ = (status, body);
    }
}
