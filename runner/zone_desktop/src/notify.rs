//! Notifications for a chat that stopped and is waiting for its reader.
//!
//! The page shows them, through the notification plugin's own commands, because
//! only it knows what each chat was doing a moment ago. What a tap on one does
//! is the host's, because only the host can bring the window forward.
//!
//! A tap carries the chat back in the notification's own payload, so a
//! notification that waited an hour still opens the chat it was about rather
//! than whichever one was notified last. The page may be gone by then — on a
//! phone the app is routinely killed while its notification waits — so the chat
//! is also kept here, and handed to the next page that loads.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime, Webview};
use tauri_plugin_notification::NotificationExt;

/// The key the chat id travels under in a notification's payload.
const CHAT: &str = "chat";

/// The action identifier of a tap on the notification itself, as the plugin
/// reports it on every platform.
const TAP: &str = "tap";

/// How long a tap is still what the next page to load is about.
///
/// A tap on a phone can have to wait for the whole app to start, which is why
/// the chat is kept at all. It is not kept indefinitely: a page that loads long
/// afterwards is the reader opening the app themselves, and taking them to a
/// chat they tapped this morning would be taking them away from whatever they
/// opened it for.
const STILL_ASKED_FOR: Duration = Duration::from_secs(60);

/// The chat a tap asked to open, and when it asked.
#[derive(Default)]
pub struct Pending(Mutex<Option<(String, Instant)>>);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Pending {
    fn set(&self, chat: &str, when: Instant) {
        *lock(&self.0) = Some((chat.to_owned(), when));
    }

    /// The chat a tap asked for, if it asked recently enough to still mean it.
    ///
    /// Handed out once either way: a tap that went stale is forgotten rather
    /// than left to surprise a later page.
    fn take_asked_at(&self, now: Instant) -> Option<String> {
        let (chat, when) = lock(&self.0).take()?;
        (now.duration_since(when) <= STILL_ASKED_FOR).then_some(chat)
    }
}

/// Starts routing taps on this app's notifications to the chat they are about.
///
/// Registered before the page can show anything, because the desktop plugin
/// only asks the notification server to report a tap when something is
/// listening for one.
pub fn listen(app: &AppHandle) {
    let handle = app.clone();
    let listening = app.notification().on_action(move |performed| {
        if performed.action_id() != TAP {
            return;
        }
        let chat = performed
            .notification()
            .and_then(|notification| notification.extra().get(CHAT))
            .and_then(|chat| chat.as_str());
        match chat {
            Some(chat) => open(&handle, chat),
            None => tracing::warn!("A notification was tapped without a chat to open"),
        }
    });
    if let Err(error) = listening {
        tracing::error!(%error, "Taps on notifications will not open their chat");
    }
}

/// Brings the window forward and takes it to the chat.
fn open(app: &AppHandle, chat: &str) {
    if !is_chat_id(chat) {
        tracing::warn!("A notification was tapped carrying something that is not a chat id");
        return;
    }
    // Set before the page is asked, so a tap that arrives while the app is
    // still starting is not lost: whichever of the two gets there first opens
    // the chat, and the other finds nothing to do.
    if let Some(pending) = app.try_state::<Pending>() {
        pending.set(chat, Instant::now());
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(desktop)]
    if let Err(error) = window.unminimize() {
        tracing::debug!(%error, "Could not unminimize the window for a tapped notification");
    }
    for (step, outcome) in [("show", window.show()), ("focus", window.set_focus())] {
        if let Err(error) = outcome {
            tracing::warn!(%error, step, "Could not bring the window forward");
        }
    }
    route(window.as_ref(), chat);
}

/// Hands a page that has just loaded the chat a tap asked for, if one did.
///
/// What covers a tap on a notification whose app was not running: the page that
/// starts because of the tap is the one that has to act on it.
pub fn opened_page<R: Runtime>(webview: &Webview<R>) {
    let Some(pending) = webview.app_handle().try_state::<Pending>() else {
        return;
    };
    if let Some(chat) = pending.take_asked_at(Instant::now()) {
        route(webview, &chat);
    }
}

/// Tells a page to show a chat.
fn route<R: Runtime>(page: &Webview<R>, chat: &str) {
    if let Err(error) = page.eval(script(chat)) {
        tracing::error!(%error, "Could not open the chat a notification was about");
    }
}

/// Whether this is the shape of a chat id, which is what makes it safe to put
/// in the script below.
///
/// A payload reaches here from the page and from the platform's notification
/// store, so it is checked rather than trusted: anything but the hex and dashes
/// of a uuid is refused.
pub fn is_chat_id(value: &str) -> bool {
    value.len() == 36
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
}

/// The script that takes the page to a chat.
///
/// The console registers `__zoneOpenChat` and routes without a reload. Without
/// it — a page still starting, or one that has navigated away from the console
/// — the url does the same thing the slower way.
fn script(chat: &str) -> String {
    format!(
        "(() => {{\n  const chat = '{chat}';\n  const open = window.__zoneOpenChat;\n  if (typeof open === 'function') {{ open(chat); return; }}\n  const url = '/chats?id=' + chat;\n  if (window.location.pathname + window.location.search !== url) {{\n    window.location.assign(url);\n  }}\n}})();"
    )
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;

    const CHAT_ID: &str = "0f2b7c3e-9a41-4d8e-8f55-2c6d1b4a7e90";

    #[test]
    fn a_chat_id_is_hex_and_dashes_of_a_uuid_s_length() {
        assert!(is_chat_id(CHAT_ID));
        assert!(!is_chat_id(""));
        assert!(!is_chat_id("0f2b7c3e"), "a short id read as one");
        assert!(
            !is_chat_id("0f2b7c3e-9a41-4d8e-8f55-2c6d1b4a7e9z"),
            "a non-hex character read as one"
        );
    }

    #[test]
    fn a_payload_that_could_escape_the_script_is_refused() {
        for hostile in [
            "'); fetch('http://evil.test'); ('",
            "</script><script>alert(1)</script>",
            "0f2b7c3e-9a41-4d8e-8f55-2c6d1b4a7e90'",
        ] {
            assert!(!is_chat_id(hostile), "{hostile} read as a chat id");
        }
    }

    #[test]
    fn the_script_routes_in_the_page_and_falls_back_to_the_url() {
        let script = script(CHAT_ID);
        assert!(script.contains(CHAT_ID));
        assert!(script.contains("__zoneOpenChat"));
        assert!(script.contains("'/chats?id=' + chat"));
    }

    #[test]
    fn the_chat_a_tap_asked_for_is_handed_out_once() {
        let pending = Pending::default();
        let now = Instant::now();
        assert_eq!(pending.take_asked_at(now), None);

        pending.set(CHAT_ID, now);
        assert_eq!(pending.take_asked_at(now).as_deref(), Some(CHAT_ID));
        assert_eq!(
            pending.take_asked_at(now),
            None,
            "the chat was handed out twice"
        );
    }

    #[test]
    fn a_tap_the_reader_has_moved_on_from_opens_nothing_and_is_forgotten() {
        let pending = Pending::default();
        let tapped = Instant::now();
        pending.set(CHAT_ID, tapped);

        let later = tapped + STILL_ASKED_FOR + Duration::from_secs(1);
        assert_eq!(pending.take_asked_at(later), None);
        assert_eq!(
            pending.take_asked_at(tapped),
            None,
            "a stale tap was left behind for a later page"
        );
    }

    #[test]
    fn a_tap_still_counts_at_the_edge_of_the_window() {
        let pending = Pending::default();
        let tapped = Instant::now();
        pending.set(CHAT_ID, tapped);

        assert_eq!(
            pending.take_asked_at(tapped + STILL_ASKED_FOR).as_deref(),
            Some(CHAT_ID)
        );
    }
}
