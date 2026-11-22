use tokio::sync::{broadcast, watch};

use super::envelope::Envelope;
use super::topic::Topic;

/// The local fan-out for one subscribed topic.
#[derive(Debug)]
pub struct Channel {
    pub topic: Topic,
    pub sender: broadcast::Sender<Envelope>,
    pub subscribers: usize,
    pub confirmed: watch::Receiver<bool>,
}
