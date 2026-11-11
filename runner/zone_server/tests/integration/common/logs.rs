//! What the server logs while a test runs, for a test that reads it back.

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use tracing::subscriber::DefaultGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the capture is readable")).into_owned()
    }
}

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("the capture is writable")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for Captured {
    type Writer = Captured;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

/// Every log line this thread writes under `targets`, an [`EnvFilter`]'s directives, while the
/// guard lives. A test reading it runs on a current-thread runtime, so every task the server
/// spawns logs here. A global subscriber is installed first because, beside a lone scoped one,
/// tracing caches each callsite's interest from whichever thread first reaches it, so a callsite
/// a parallel test reached first would never log here.
pub fn capturing(targets: &str) -> (Captured, DefaultGuard) {
    super::init_tracing();
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(targets))
        .with_writer(captured.clone())
        .with_ansi(false)
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}
