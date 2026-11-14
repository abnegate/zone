//! Zone desktop and mobile client.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use zone_installer::client::{ClientPlatform, ManagerDirInputs, WEBVIEW_BIND};
use zone_installer::frontend::{self, AppMode};
use zone_installer::serve::AppState;
use zone_installer::{MANAGER_DIR, ServeKind, bind, config_path, resolve_manager_dir, router};

mod manager_bundle;
#[cfg(desktop)]
mod menu;
mod notify;

struct ClientState {
    server: AppState,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zone_desktop=info,zone_installer=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async move { setup_client(handle).await })
        })
        // A notification tapped while the app was not running has no page to
        // take anywhere, so the page that starts because of the tap is handed
        // the chat as it loads.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                notify::opened_page(webview);
            }
        })
        .invoke_handler(tauri::generate_handler![change_server]);

    #[cfg(desktop)]
    let builder = builder.on_menu_event(|app, event| {
        let Some(state) = app.try_state::<ClientState>() else {
            return;
        };
        if event.id().as_ref() == menu::CHANGE_SERVER {
            state.server.set_mode(AppMode::Setup);
            reload_main(app);
        }
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running Zone");
}

#[tauri::command]
fn change_server(app: AppHandle) {
    let Some(state) = app.try_state::<ClientState>() else {
        return;
    };
    state.server.set_mode(AppMode::Setup);
    reload_main(&app);
}

async fn setup_client(app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let manager_dir = manager_dir(&app);
    let config_path = client_config_path(&app);
    let proxy_target = frontend::proxy_target_from(&config_path);
    let state = AppState::desktop(manager_dir.clone(), proxy_target.clone())?
        .with_config_path(config_path.clone());
    if !frontend::is_configured_at(&config_path) {
        state.set_mode(AppMode::Setup);
    }

    let (listener, bound) = bind(WEBVIEW_BIND).await?;
    let url = format!("http://{bound}/");
    tracing::info!(
        manager = %manager_dir.display(),
        config = %config_path.display(),
        %proxy_target,
        effective_proxy_target = %zone_installer::proxy::effective_proxy_target(&proxy_target),
        configured = frontend::is_configured_at(&config_path),
        %bound,
        "Starting Zone client server"
    );
    let router = router(ServeKind::Desktop, state.clone());
    tauri::async_runtime::spawn(async move {
        if let Err(err) = zone_installer::serve::serve(listener, router).await {
            tracing::error!(error = %err, "Zone client server exited");
        }
    });

    app.manage(ClientState {
        server: state.clone(),
    });
    app.manage(notify::Pending::default());
    notify::listen(&app);

    #[cfg(desktop)]
    menu::build(&app)?;

    open_window(&app, &url)?;
    Ok(())
}

fn client_config_path(app: &AppHandle) -> PathBuf {
    config_path(
        ClientPlatform::current(),
        app.path().app_config_dir().ok(),
        frontend::config_file(),
    )
}

fn open_window(app: &AppHandle, url: &str) -> Result<(), Box<dyn std::error::Error>> {
    if app.get_webview_window("main").is_some() {
        return Ok(());
    }
    let builder =
        WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?)).title("Zone");

    #[cfg(desktop)]
    let builder = builder
        .inner_size(1280.0, 840.0)
        .min_inner_size(900.0, 600.0)
        .resizable(true);

    builder.build()?;
    Ok(())
}

fn reload_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval("window.location.reload()");
    }
}

fn manager_dir(app: &AppHandle) -> PathBuf {
    let platform = ClientPlatform::current();
    // A stub build must not read a bundle an earlier build unpacked, or it shadows the checkout.
    let app_data_dir = app
        .path()
        .app_data_dir()
        .ok()
        .filter(|_| manager_bundle::unpacks(platform, manager_bundle::EMBEDDED));
    if let Some(data) = &app_data_dir {
        let dest = data.join(MANAGER_DIR);
        if let Err(error) = manager_bundle::materialize(&dest) {
            tracing::error!(
                %error,
                dest = %dest.display(),
                "Failed to unpack manager UI"
            );
        }
    }
    resolve_manager_dir(ManagerDirInputs {
        platform,
        resource_dir: app.path().resource_dir().ok(),
        app_data_dir,
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        system_share_dir: Some(PathBuf::from(frontend::DEBIAN_MANAGER_DIR)),
    })
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;

    #[test]
    fn host_unit_tests_run_as_desktop() {
        assert_eq!(
            ClientPlatform::current(),
            ClientPlatform::from_os(std::env::consts::OS)
        );
    }
}
