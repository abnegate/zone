//! The server's way out after a SIGTERM.
//!
//! Draining comes first: readiness fails so load balancers stop sending new
//! work, and chat sockets refuse to start turns, while everything already
//! running carries on. Closing comes last: every open WebSocket is told the
//! server is restarting, so its client reconnects to another replica.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio_util::sync::WaitForCancellationFuture;
use tokio_util::task::TaskTracker;

#[derive(Clone, Debug, Default)]
pub struct Drain {
    draining: CancellationToken,
    closing: CancellationToken,
    sockets: TaskTracker,
}

impl Drain {
    pub fn begin(&self) {
        self.draining.cancel();
    }

    pub fn is_draining(&self) -> bool {
        self.draining.is_cancelled()
    }

    /// Resolves once the server starts draining.
    pub fn draining(&self) -> WaitForCancellationFuture<'_> {
        self.draining.cancelled()
    }

    /// Tells every WebSocket to close. A server that closes has stopped draining.
    pub fn close(&self) {
        self.draining.cancel();
        self.closing.cancel();
    }

    pub fn closing(&self) -> WaitForCancellationFuture<'_> {
        self.closing.cancelled()
    }

    /// The WebSockets this process serves, so it can wait for them to close.
    pub fn sockets(&self) -> &TaskTracker {
        &self.sockets
    }

    /// Closes every WebSocket and waits up to `within` for them to finish.
    /// Returns whether they all did.
    pub async fn close_sockets(&self, within: Duration) -> bool {
        self.close();
        self.sockets.close();
        tokio::time::timeout(within, self.sockets.wait())
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_drain_is_serving() {
        let drain = Drain::default();
        assert!(!drain.is_draining());
    }

    #[test]
    fn beginning_drains_every_clone_without_closing_sockets() {
        let drain = Drain::default();
        let shared = drain.clone();

        drain.begin();

        assert!(shared.is_draining());
        assert!(!shared.closing.is_cancelled());
    }

    #[tokio::test]
    async fn closing_sockets_tells_each_one_and_waits_for_it_to_finish() {
        let drain = Drain::default();
        let socket = drain.clone();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = finished.clone();
        drain.sockets().spawn(async move {
            socket.closing().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        assert!(drain.close_sockets(Duration::from_secs(5)).await);

        assert!(drain.is_draining());
        assert!(finished.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_socket_that_never_closes_is_given_up_on() {
        let drain = Drain::default();
        drain.sockets().spawn(std::future::pending::<()>());

        let started = std::time::Instant::now();
        assert!(!drain.close_sockets(Duration::from_millis(100)).await);

        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
