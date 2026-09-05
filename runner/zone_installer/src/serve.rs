//! HTTP routers for console and desktop modes.

use std::path::{Component, Path, PathBuf};
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

use crate::frontend::AppMode;
use crate::proxy;
use crate::setup;

#[derive(Clone)]
pub struct AppState {
    pub frontend_dir: PathBuf,
    pub manager_dir: PathBuf,
    pub mode: Arc<RwLock<AppMode>>,
    pub proxy_target: Arc<RwLock<String>>,
    pub config_path: PathBuf,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(
        mode: AppMode,
        frontend_dir: PathBuf,
        proxy_target: String,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            manager_dir: frontend_dir.clone(),
            frontend_dir,
            mode: Arc::new(RwLock::new(mode)),
            proxy_target: Arc::new(RwLock::new(proxy_target)),
            config_path: crate::frontend::config_file(),
            http: proxy::http_client()?,
        })
    }

    pub fn desktop(manager_dir: PathBuf, proxy_target: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            frontend_dir: manager_dir.clone(),
            manager_dir,
            mode: Arc::new(RwLock::new(AppMode::Console)),
            proxy_target: Arc::new(RwLock::new(proxy_target)),
            config_path: crate::frontend::config_file(),
            http: proxy::http_client()?,
        })
    }

    pub fn with_config_path(mut self, config_path: PathBuf) -> Self {
        self.config_path = config_path;
        self
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
        self.manager_dir.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeKind {
    ConsoleOnly,
    Desktop,
}

pub fn router(kind: ServeKind, state: AppState) -> Router {
    match kind {
        ServeKind::ConsoleOnly => Router::new()
            .route("/api/{*rest}", any(proxy::proxy_http))
            .route("/ws", get(proxy::proxy_ws))
            .route("/ws/{*rest}", get(proxy::proxy_ws))
            .fallback(serve_spa)
            .with_state(state),
        ServeKind::Desktop => Router::new()
            .route("/__zone/info", get(setup::client_info))
            .route("/__zone/change-server", get(setup::handle_change_server))
            .route("/api/setup", post(setup::handle_setup))
            .route("/api/{*rest}", any(proxy::proxy_http))
            .route("/ws", get(proxy::proxy_ws))
            .route("/ws/{*rest}", get(proxy::proxy_ws))
            .fallback(serve_spa)
            .with_state(state),
    }
}

async fn serve_spa(State(state): State<AppState>, uri: Uri) -> Response {
    if state.mode() == AppMode::Setup {
        return setup::page();
    }
    serve_under_root(&state.active_frontend(), uri.path()).await
}

/// Join `request_path` onto `root` only when every component is a normal
/// relative segment (`foo/bar`, not `..` or `/etc`).
fn safe_join(root: &Path, request_path: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in Path::new(request_path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if relative.as_os_str().is_empty() {
        relative.push("index.html");
    }
    let candidate = root.join(relative);
    candidate.starts_with(root).then_some(candidate)
}

async fn serve_under_root(root: &Path, request_path: &str) -> Response {
    let Ok(public_path) = root.canonicalize() else {
        return frontend_missing();
    };
    let joined =
        safe_join(&public_path, request_path).unwrap_or_else(|| public_path.join("index.html"));
    let file_path = match joined.canonicalize() {
        Ok(path) if path.is_file() => path,
        _ => match public_path.join("index.html").canonicalize() {
            Ok(path) => path,
            Err(_) => return frontend_missing(),
        },
    };
    // CodeQL rust/path-injection: canonicalize, then confirm the resolved
    // path stays inside the frontend root before any filesystem read.
    if !file_path.starts_with(&public_path) {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    match tokio::fs::read(&file_path).await {
        Ok(bytes) => {
            let mime = mime_for(&file_path);
            Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, mime)
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(err) => {
            tracing::error!(error = ?err, path = %file_path.display(), "Failed to read frontend file");
            if file_path.file_name().and_then(|n| n.to_str()) == Some("index.html") {
                frontend_missing()
            } else {
                (StatusCode::NOT_FOUND, "Not found").into_response()
            }
        }
    }
}

fn frontend_missing() -> Response {
    (StatusCode::NOT_FOUND, "Frontend not found").into_response()
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
        )
        .expect("http client");
        assert_eq!(state.active_frontend(), PathBuf::from("/tmp/manager"));
        assert_eq!(state.mode(), AppMode::Console);
        state.set_mode(AppMode::Setup);
        assert_eq!(state.mode(), AppMode::Setup);
        state.set_proxy_target("https://other.example".into());
        assert_eq!(state.proxy_target(), "https://other.example");
        let state = state.with_config_path(PathBuf::from("/tmp/zone-mobile.toml"));
        assert_eq!(state.config_path, PathBuf::from("/tmp/zone-mobile.toml"));
    }

    #[test]
    fn safe_join_keeps_assets_inside_root() {
        let root = PathBuf::from("/tmp/zone-manager");
        assert_eq!(
            safe_join(&root, "assets/app.js").unwrap(),
            root.join("assets/app.js")
        );
        assert_eq!(safe_join(&root, "/").unwrap(), root.join("index.html"));
    }

    #[test]
    fn safe_join_rejects_parent_and_absolute_paths() {
        let root = PathBuf::from("/tmp/zone-manager");
        assert_eq!(safe_join(&root, "../etc/passwd"), None);
        assert_eq!(safe_join(&root, "assets/../../etc/passwd"), None);
        // HTTP paths always start with `/`; that is relative to the frontend root.
        assert_eq!(
            safe_join(&root, "/etc/passwd").unwrap(),
            root.join("etc/passwd")
        );
    }

    #[test]
    fn contained_file_stays_inside_canonical_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("frontend");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), b"ok").unwrap();
        std::fs::write(tmp.path().join("secret"), b"no").unwrap();
        let public = root.canonicalize().unwrap();
        let index = safe_join(&public, "index.html")
            .and_then(|p| p.canonicalize().ok())
            .expect("index");
        assert!(index.starts_with(&public));
        assert!(safe_join(&public, "../secret").is_none());
    }
}
