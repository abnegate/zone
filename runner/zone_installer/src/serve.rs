//! HTTP routers for install, console, and desktop modes.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

use crate::frontend::AppMode;
use crate::install;
use crate::proxy;
use crate::setup;

#[derive(Clone)]
pub struct AppState {
    pub frontend_dir: PathBuf,
    pub installer_dir: PathBuf,
    pub manager_dir: PathBuf,
    pub mode: Arc<RwLock<AppMode>>,
    pub proxy_target: Arc<RwLock<String>>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(mode: AppMode, frontend_dir: PathBuf, proxy_target: String) -> Self {
        Self {
            installer_dir: frontend_dir.clone(),
            manager_dir: frontend_dir.clone(),
            frontend_dir,
            mode: Arc::new(RwLock::new(mode)),
            proxy_target: Arc::new(RwLock::new(proxy_target)),
            http: proxy::http_client(),
        }
    }

    pub fn desktop(manager_dir: PathBuf, proxy_target: String) -> Self {
        Self {
            frontend_dir: manager_dir.clone(),
            manager_dir,
            installer_dir: PathBuf::new(),
            mode: Arc::new(RwLock::new(AppMode::Console)),
            proxy_target: Arc::new(RwLock::new(proxy_target)),
            http: proxy::http_client(),
        }
    }

    pub fn mode(&self) -> AppMode {
        self.mode
            .read()
            .map(|mode| *mode)
            .unwrap_or(AppMode::Console)
    }

    pub fn set_mode(&self, mode: AppMode) {
        if let Ok(mut current) = self.mode.write() {
            *current = mode;
        }
    }

    pub fn proxy_target(&self) -> String {
        self.proxy_target
            .read()
            .map(|target| target.clone())
            .unwrap_or_default()
    }

    pub fn set_proxy_target(&self, target: String) {
        if let Ok(mut current) = self.proxy_target.write() {
            *current = target;
        }
    }

    pub fn active_frontend(&self) -> PathBuf {
        match self.mode() {
            AppMode::Install => self.installer_dir.clone(),
            AppMode::Console | AppMode::Setup => self.manager_dir.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeKind {
    InstallOnly,
    ConsoleOnly,
    Desktop,
}

pub fn router(kind: ServeKind, state: AppState) -> Router {
    match kind {
        ServeKind::InstallOnly => {
            let assets = state.frontend_dir.join("assets");
            Router::new()
                .route("/", get(serve_index))
                .route("/api/install", post(install::handle_install))
                .nest_service("/assets", ServeDir::new(assets))
                .with_state(state)
        }
        ServeKind::ConsoleOnly => Router::new()
            .route("/api/{*rest}", any(proxy::proxy_http))
            .route("/ws", get(proxy::proxy_ws))
            .route("/ws/{*rest}", get(proxy::proxy_ws))
            .fallback(serve_spa)
            .with_state(state),
        ServeKind::Desktop => Router::new()
            .route("/api/setup", post(setup::handle_setup))
            .route("/api/{*rest}", any(proxy::proxy_http))
            .route("/ws", get(proxy::proxy_ws))
            .route("/ws/{*rest}", get(proxy::proxy_ws))
            .fallback(serve_spa)
            .with_state(state),
    }
}

async fn serve_index(State(state): State<AppState>) -> impl IntoResponse {
    serve_file(state.active_frontend().join("index.html")).await
}

async fn serve_spa(State(state): State<AppState>, uri: Uri) -> Response {
    if state.mode() == AppMode::Setup {
        return setup::page();
    }
    let root = state.active_frontend();
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() {
        root.join("index.html")
    } else {
        root.join(path)
    };
    if candidate.is_file() {
        return serve_file(candidate).await;
    }
    serve_file(root.join("index.html")).await
}

async fn serve_file(path: PathBuf) -> Response {
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_for(&path);
            Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, mime)
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(err) => {
            tracing::error!(error = ?err, path = %path.display(), "Failed to read frontend file");
            if path.file_name().and_then(|n| n.to_str()) == Some("index.html") {
                (StatusCode::NOT_FOUND, "Frontend not found").into_response()
            } else {
                (StatusCode::NOT_FOUND, "Not found").into_response()
            }
        }
    }
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("woff2") => "font/woff2",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

pub async fn bind(addr: &str) -> std::io::Result<(TcpListener, String)> {
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?.to_string();
    Ok((listener, bound))
}

pub async fn serve(listener: TcpListener, app: Router) -> std::io::Result<()> {
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_starts_in_console_mode() {
        let state = AppState::desktop(
            PathBuf::from("/tmp/manager"),
            "https://zone.example.com".into(),
        );
        assert_eq!(state.active_frontend(), PathBuf::from("/tmp/manager"));
        assert_eq!(state.mode(), AppMode::Console);
        state.set_mode(AppMode::Setup);
        assert_eq!(state.mode(), AppMode::Setup);
        state.set_proxy_target("https://other.example".into());
        assert_eq!(state.proxy_target(), "https://other.example");
    }
}
