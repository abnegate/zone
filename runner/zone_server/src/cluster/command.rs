use std::sync::Arc;

use tokio::sync::watch;

/// A change to the channels the pub/sub connection listens on.
#[derive(Debug)]
pub enum Command {
    Subscribe {
        channel: Arc<str>,
        confirmed: watch::Sender<bool>,
    },
    Unsubscribe {
        channel: Arc<str>,
    },
}
