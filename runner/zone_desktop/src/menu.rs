mod entry;
mod section;

pub use entry::Entry;
pub use section::Section;

use tauri::AppHandle;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

pub const CHANGE_SERVER: &str = "change-server";

const CHANGE_SERVER_LABEL: &str = "Change Server…";
const CHANGE_SERVER_ACCELERATOR: &str = "CmdOrCtrl+,";
const APP: &str = "Zone";
const EDIT: &str = "Edit";
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
        Entry::ChangeServer => submenu.item(
            &MenuItemBuilder::with_id(CHANGE_SERVER, CHANGE_SERVER_LABEL)
                .accelerator(CHANGE_SERVER_ACCELERATOR)
                .build(app)?,
        ),
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
