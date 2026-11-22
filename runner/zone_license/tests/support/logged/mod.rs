mod capture;
mod writer;

use std::future::Future;
use std::sync::Once;

use tracing::Level;
use tracing_subscriber::filter::dynamic_filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, fmt};

use capture::Capture;
use writer::Writer;

static INSTALL: Once = Once::new();

/// Captures what `body` logs on this thread. A `#[tokio::test]` runtime runs
/// the server it starts on the same thread, so its request lines are caught
/// while tests on other threads keep their own.
pub async fn capture(level: Level, body: impl Future<Output = ()>) -> String {
    INSTALL.call_once(install);
    Capture::begin(level);
    body.await;
    Capture::end()
}

fn install() {
    let layer = fmt::layer()
        .with_ansi(false)
        .with_writer(|| Writer)
        .with_filter(dynamic_filter_fn(|metadata, _| Capture::admits(metadata)));
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .expect("no other test sets a global subscriber");
    // A callsite registered on another thread mid-install was cached against no subscriber at all.
    tracing::callsite::rebuild_interest_cache();
}
