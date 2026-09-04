#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use zone_installer::frontend::{self, AppMode};
use zone_installer::serve::AppState;
use zone_installer::{ServeKind, bind, router};

fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zone_desktop=info,zone_installer=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async move { setup_desktop(handle).await })
        })
        .on_menu_event(|app, event| {
            let Some(state) = app.try_state::<DesktopState>() else {
                return;
            };
            if event.id().as_ref() == "change-server" {
                state.server.set_mode(AppMode::Setup);
                reload_main(app);
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Zone");
}

struct DesktopState {
    server: AppState,
}

async fn setup_desktop(app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let manager_dir = manager_dir(&app);
    let proxy_target = frontend::proxy_target();
    let state = AppState::desktop(manager_dir.clone(), proxy_target.clone());
    if !frontend::is_configured() {
        state.set_mode(AppMode::Setup);
    }

    tracing::info!(
        manager = %manager_dir.display(),
        %proxy_target,
        configured = frontend::is_configured(),
        "Starting Zone desktop server"
    );

    let (listener, bound) = bind("127.0.0.1:0").await?;
    let url = format!("http://{bound}/");
    let router = router(ServeKind::Desktop, state.clone());
    tauri::async_runtime::spawn(async move {
        if let Err(err) = zone_installer::serve::serve(listener, router).await {
            tracing::error!(error = %err, "Zone desktop server exited");
        }
    });

    app.manage(DesktopState {
        server: state.clone(),
    });

    build_menu(&app)?;

    WebviewWindowBuilder::new(&app, "main", WebviewUrl::External(url.parse()?))
        .title("Zone")
        .inner_size(1280.0, 840.0)
        .min_inner_size(900.0, 600.0)
        .resizable(true)
        .build()?;

    Ok(())
}

fn build_menu(app: &AppHandle) -> tauri::Result<()> {
    let change_server = MenuItemBuilder::with_id("change-server", "Change Server…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let zone_menu = SubmenuBuilder::new(app, "Zone")
        .about(None)
        .separator()
        .item(&change_server)
        .separator()
        .quit()
        .build()?;
    let menu = MenuBuilder::new(app).item(&zone_menu).build()?;
    app.set_menu(menu)?;
    Ok(())
}

fn reload_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval("window.location.reload()");
    }
}

fn manager_dir(app: &AppHandle) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        let manager = dir.join("manager");
        if manager.join("index.html").exists() {
            return manager;
        }
    }

    let share = PathBuf::from("/usr/share/zone/manager");
    if share.join("index.html").exists() {
        return share;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for root in [cwd.clone(), cwd.join(".."), cwd.join("../..")] {
        let manager = root.join("manager/frontend/build");
        if manager.join("index.html").exists() {
            return manager;
        }
    }

    frontend::resolve_frontend_dir(zone_installer::FrontendKind::Manager)
}
