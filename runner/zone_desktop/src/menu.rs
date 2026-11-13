mod entry;
mod section;

pub use entry::Entry;
pub use section::Section;

use tauri::AppHandle;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

pub const CHANGE_SERVER: &str = "change-server";
pub const ZOOM_IN: &str = "zoom-in";
pub const ZOOM_OUT: &str = "zoom-out";
pub const ACTUAL_SIZE: &str = "actual-size";

const CHANGE_SERVER_LABEL: &str = "Change Server…";
const CHANGE_SERVER_ACCELERATOR: &str = "CmdOrCtrl+,";
// An accelerator binds a physical key, so these claim the main-row `=`, `-`
// and `0`. The shifted `+`, the numpad and other layouts never reach a menu
// item and are handled in `zoom.js` instead.
const ZOOM_IN_LABEL: &str = "Zoom In";
const ZOOM_IN_ACCELERATOR: &str = "CmdOrCtrl+Equal";
const ZOOM_OUT_LABEL: &str = "Zoom Out";
const ZOOM_OUT_ACCELERATOR: &str = "CmdOrCtrl+Minus";
const ACTUAL_SIZE_LABEL: &str = "Actual Size";
const ACTUAL_SIZE_ACCELERATOR: &str = "CmdOrCtrl+0";
const APP: &str = "Zone";
const EDIT: &str = "Edit";
const VIEW: &str = "View";
const WINDOW: &str = "Window";

pub const LAYOUT: &[Section] = &[
    Section {
        title: APP,
        entries: &[
            Entry::About,
            Entry::Separator,
            Entry::ChangeServer,
            Entry::Separator,
            Entry::Hide,
            Entry::HideOthers,
            Entry::Separator,
            Entry::Quit,
        ],
    },
    Section {
        title: EDIT,
        entries: &[
            Entry::Undo,
            Entry::Redo,
            Entry::Separator,
            Entry::Cut,
            Entry::Copy,
            Entry::Paste,
            Entry::SelectAll,
        ],
    },
    Section {
        title: VIEW,
        entries: &[Entry::ZoomIn, Entry::ZoomOut, Entry::ActualSize],
    },
    Section {
        title: WINDOW,
        entries: &[
            Entry::Minimize,
            Entry::Maximize,
            Entry::Separator,
            Entry::CloseWindow,
        ],
    },
];

type Submenu<'m> = SubmenuBuilder<'m, tauri::Wry, AppHandle>;

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let mut menu = MenuBuilder::new(app);
    for section in LAYOUT {
        let mut submenu = SubmenuBuilder::new(app, section.title);
        for &entry in section.entries {
            submenu = append(app, submenu, entry)?;
        }
        menu = menu.item(&submenu.build()?);
    }
    app.set_menu(menu.build()?)?;
    Ok(())
}

fn append<'m>(
    app: &'m AppHandle,
    submenu: Submenu<'m>,
    entry: Entry,
) -> tauri::Result<Submenu<'m>> {
    Ok(match entry {
        Entry::About => submenu.about(None),
        Entry::ChangeServer => submenu.item(&command(
            app,
            CHANGE_SERVER,
            CHANGE_SERVER_LABEL,
            CHANGE_SERVER_ACCELERATOR,
        )?),
        Entry::ZoomIn => submenu.item(&command(app, ZOOM_IN, ZOOM_IN_LABEL, ZOOM_IN_ACCELERATOR)?),
        Entry::ZoomOut => submenu.item(&command(
            app,
            ZOOM_OUT,
            ZOOM_OUT_LABEL,
            ZOOM_OUT_ACCELERATOR,
        )?),
        Entry::ActualSize => submenu.item(&command(
            app,
            ACTUAL_SIZE,
            ACTUAL_SIZE_LABEL,
            ACTUAL_SIZE_ACCELERATOR,
        )?),
        Entry::Hide => submenu.hide(),
        Entry::HideOthers => submenu.hide_others(),
        Entry::Quit => submenu.quit(),
        Entry::Undo => submenu.undo(),
        Entry::Redo => submenu.redo(),
        Entry::Cut => submenu.cut(),
        Entry::Copy => submenu.copy(),
        Entry::Paste => submenu.paste(),
        Entry::SelectAll => submenu.select_all(),
        Entry::Minimize => submenu.minimize(),
        Entry::Maximize => submenu.maximize(),
        Entry::CloseWindow => submenu.close_window(),
        Entry::Separator => submenu.separator(),
    })
}

/// An item the app answers itself in `on_menu_event`, rather than one of the
/// predefined entries Tauri wires up on its own.
fn command(
    app: &AppHandle,
    id: &str,
    label: &str,
    accelerator: &str,
) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    MenuItemBuilder::with_id(id, label)
        .accelerator(accelerator)
        .build(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(title: &str) -> &'static Section {
        LAYOUT
            .iter()
            .find(|section| section.title == title)
            .unwrap_or_else(|| panic!("menu bar has no {title} menu"))
    }

    fn assert_contains(title: &str, expected: &[Entry]) {
        let entries = section(title).entries;
        for entry in expected {
            assert!(
                entries.contains(entry),
                "{title} menu is missing {entry:?}, so its shortcut never reaches the webview"
            );
        }
    }

    #[test]
    fn edit_menu_routes_clipboard_shortcuts_to_the_webview() {
        assert_contains(
            EDIT,
            &[
                Entry::Undo,
                Entry::Redo,
                Entry::Cut,
                Entry::Copy,
                Entry::Paste,
                Entry::SelectAll,
            ],
        );
    }

    #[test]
    fn app_menu_keeps_change_server_and_quit() {
        assert_eq!(LAYOUT[0].title, APP);
        assert_contains(APP, &[Entry::ChangeServer, Entry::Quit]);
    }

    #[test]
    fn view_menu_carries_the_zoom_entries() {
        assert_contains(VIEW, &[Entry::ZoomIn, Entry::ZoomOut, Entry::ActualSize]);
    }

    #[test]
    fn no_two_items_claim_the_same_accelerator() {
        let accelerators = [
            CHANGE_SERVER_ACCELERATOR,
            ZOOM_IN_ACCELERATOR,
            ZOOM_OUT_ACCELERATOR,
            ACTUAL_SIZE_ACCELERATOR,
        ];
        for (index, accelerator) in accelerators.iter().enumerate() {
            assert!(
                !accelerators[index + 1..].contains(accelerator),
                "{accelerator} is claimed twice, so one of the two never fires"
            );
        }
    }

    #[test]
    fn zoom_ids_are_distinct() {
        let ids = [ZOOM_IN, ZOOM_OUT, ACTUAL_SIZE, CHANGE_SERVER];
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[index + 1..].contains(id), "{id} is used twice");
        }
    }

    #[test]
    fn window_menu_closes_and_minimizes() {
        assert_contains(WINDOW, &[Entry::Minimize, Entry::CloseWindow]);
    }

    #[test]
    fn change_server_appears_once() {
        let count = LAYOUT
            .iter()
            .flat_map(|section| section.entries)
            .filter(|&&entry| entry == Entry::ChangeServer)
            .count();
        assert_eq!(count, 1);
    }
}
