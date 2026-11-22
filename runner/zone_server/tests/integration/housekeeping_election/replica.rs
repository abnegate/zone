use tokio::task::JoinHandle;
use zone_server::workers::housekeeping::{Jitter, Leadership, Registry, Worker};

/// One server's housekeeping: the production worker, led by its own election.
pub struct Replica {
    pub leadership: Leadership,
    worker: JoinHandle<()>,
}

impl Replica {
    pub fn start(registry: Registry, leadership: Leadership) -> Self {
        let worker = Worker::new(registry, leadership.clone())
            .with_jitter(Jitter::none())
            .spawn();
        Self { leadership, worker }
    }

    pub fn leads(&self) -> bool {
        self.leadership.leads()
    }

    /// Stop the worker, then give leadership back, the order a server shuts down in.
    pub async fn stop(self) {
        self.worker.abort();
        let _ = self.worker.await;
        self.leadership.release().await;
    }
}
