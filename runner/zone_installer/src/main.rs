//! Zone installer binary.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use zone_installer::frontend::{self, AppMode, FrontendKind};
use zone_installer::serve::{self as serve_mod, serve as serve_http};
use zone_installer::{ServeKind, bind, router};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zone_installer=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mode = frontend::app_mode();
    let kind = match mode {
        AppMode::Console | AppMode::Setup => FrontendKind::Manager,
        AppMode::Install => FrontendKind::Installer,
    };
    let frontend_dir = frontend::resolve_frontend_dir(kind);
    let bind_addr = frontend::bind_addr();
    let proxy_target = frontend::proxy_target();
    let serve_kind = match mode {
        AppMode::Console | AppMode::Setup => ServeKind::ConsoleOnly,
        AppMode::Install => ServeKind::InstallOnly,
    };

    tracing::info!(
        ?mode,
        %bind_addr,
        %proxy_target,
        frontend = %frontend_dir.display(),
        "Starting Zone installer"
    );

    let state = serve_mod::AppState::new(mode, frontend_dir, proxy_target);
    let app = router(serve_kind, state);
    let (listener, bound) = bind(&bind_addr)
        .await
        .unwrap_or_else(|err| panic!("Failed to bind to {bind_addr}: {err}"));
    tracing::info!("Listening on http://{bound}");
    serve_http(listener, app).await.expect("Server error");
}
