//! Webview zoom for the desktop client.
//!
//! The page holds the level: a menu accelerator binds one physical key, so the
//! shifted `+`, the numpad and the layouts that carry `+` somewhere else can
//! only be caught by a key handler inside the webview, and native code cannot
//! read back what that handler worked out. The View menu items therefore call
//! into the same script rather than zooming on their own, which keeps one
//! number in one place.

use tauri::{AppHandle, Manager};

use crate::menu;

/// The script injected before the page loads. Only the host knows whether the
/// shortcuts answer to command or to control.
pub fn script() -> String {
    format!(
        "globalThis.__ZONE_ZOOM_MAC__ = {};\n{}",
        cfg!(target_os = "macos"),
        include_str!("zoom.js")
    )
}

/// What a menu id asks the page to do, if it asks for zoom at all.
fn call_for(id: &str) -> Option<&'static str> {
    match id {
        menu::ZOOM_IN => Some("window.__zoneZoom?.in()"),
        menu::ZOOM_OUT => Some("window.__zoneZoom?.out()"),
        menu::ACTUAL_SIZE => Some("window.__zoneZoom?.reset()"),
        _ => None,
    }
}

/// Handles a menu event if it is one of ours, reporting whether it was.
pub fn handle_menu_event(app: &AppHandle, id: &str) -> bool {
    let Some(call) = call_for(id) else {
        return false;
    };
    if let Some(window) = app.get_webview_window("main")
        && let Err(error) = window.eval(call)
    {
        tracing::error!(%error, id, "Failed to zoom the webview");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_tells_the_page_which_modifier_to_watch() {
        let script = script();
        assert!(script.contains(if cfg!(target_os = "macos") {
            "__ZONE_ZOOM_MAC__ = true"
        } else {
            "__ZONE_ZOOM_MAC__ = false"
        }));
        assert!(script.contains("set_webview_zoom"));
    }

    #[test]
    fn every_zoom_entry_in_the_menu_has_somewhere_to_go() {
        for id in [menu::ZOOM_IN, menu::ZOOM_OUT, menu::ACTUAL_SIZE] {
            assert!(call_for(id).is_some(), "{id} would do nothing");
        }
    }

    #[test]
    fn other_menu_items_are_left_for_their_own_handlers() {
        assert_eq!(call_for(menu::CHANGE_SERVER), None);
        assert_eq!(call_for("quit"), None);
    }
}
