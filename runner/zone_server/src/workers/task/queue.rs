use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

static QUEUED: AtomicUsize = AtomicUsize::new(0);

/// How many claimed executions of this process wait for an execution slot.
pub(super) fn queued() -> usize {
    QUEUED.load(Ordering::Relaxed)
}

/// Counts the execution among the queued ones for as long as `waiting` lasts, however it ends.
pub(super) async fn waiting<T>(waiting: impl Future<Output = T>) -> T {
    let _queued = Queued::join();
    waiting.await
}

struct Queued;

impl Queued {
    fn join() -> Self {
        QUEUED.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for Queued {
    fn drop(&mut self) {
        QUEUED.fetch_sub(1, Ordering::Relaxed);
    }
}
