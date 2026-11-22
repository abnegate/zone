use std::sync::Arc;

use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

use super::envelope::Envelope;
use super::shared::Shared;
use super::topic::Topic;

/// One receiver on a topic. The bus stays subscribed while any receiver for
/// the topic lives, and unsubscribes when the last one drops.
pub struct Subscription {
    topic: Topic,
    channel: Arc<str>,
    receiver: broadcast::Receiver<Envelope>,
    shared: Arc<Shared>,
}

impl Subscription {
    pub(super) fn new(
        topic: Topic,
        channel: Arc<str>,
        receiver: broadcast::Receiver<Envelope>,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            topic,
            channel,
            receiver,
            shared,
        }
    }

    pub fn topic(&self) -> Topic {
        self.topic
    }

    /// The next envelope. `RecvError::Lagged` says envelopes were missed
    /// because this receiver fell behind.
    pub async fn recv(&mut self) -> Result<Envelope, RecvError> {
        self.receiver.recv().await
    }

    pub fn try_recv(&mut self) -> Result<Envelope, TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.shared.leave(&self.channel);
    }
}
