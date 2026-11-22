use std::cell::RefCell;
use std::future::Future;
use std::io::{Result, Write};
use std::sync::Once;

use tracing::Level;
use tracing_subscriber::filter::dynamic_filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, fmt};

thread_local! {
    static CURRENT: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

static INSTALL: Once = Once::new();

struct Capture {
    level: Level,
    written: Vec<u8>,
}

struct Writer;

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        CURRENT.with_borrow_mut(|current| {
            if let Some(capture) = current {
                capture.written.extend_from_slice(bytes);
            }
        });
        Ok(bytes.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Captures what `body` logs on this thread. A `#[tokio::test]` runtime runs
/// the server it starts on the same thread, so its request lines are caught.
pub async fn capture(level: Level, body: impl Future<Output = ()>) -> String {
    INSTALL.call_once(install);
    CURRENT.set(Some(Capture {
        level,
        written: Vec::new(),
    }));
    body.await;
    let capture = CURRENT.take().expect("a capture was begun on this thread");
    String::from_utf8(capture.written).unwrap()
}

fn install() {
    let layer = fmt::layer()
        .with_ansi(false)
        .with_writer(|| Writer)
        .with_filter(dynamic_filter_fn(|metadata, _| {
            CURRENT.with_borrow(|current| {
                current
                    .as_ref()
                    .is_some_and(|capture| *metadata.level() <= capture.level)
            })
        }));
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .expect("no other test sets a global subscriber");
    // A callsite registered on another thread mid-install was cached against no subscriber at all.
    tracing::callsite::rebuild_interest_cache();
}
