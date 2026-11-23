use std::sync::Weak;

use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use super::packet::Packet;
use super::shared::Shared;

const CAPACITY: usize = 1024;

/// An in-process stand-in for Valkey: every bus attached to one hub hears
/// what the others publish. A server without Valkey attaches only its own bus,
/// so nothing leaves the process and it behaves as a single node always has.
#[derive(Debug, Clone)]
pub struct Loopback {
    hub: broadcast::Sender<Packet>,
}

impl Loopback {
    pub fn new() -> Self {
        Self {
            hub: broadcast::Sender::new(CAPACITY),
        }
    }

    pub(super) fn send(&self, packet: Packet) {
        let _ = self.hub.send(packet);
    }

    /// Whether any bus other than the sender's own is attached.
    pub(super) fn has_peers(&self) -> bool {
        self.hub.receiver_count() > 1
    }

    pub(super) fn attach(&self, shared: Weak<Shared>, stop: CancellationToken) {
        tokio::spawn(listen(self.hub.subscribe(), shared, stop));
    }

    /// Every packet sent on the hub from now on, exactly as it crosses.
    #[cfg(test)]
    pub(super) fn tap(&self) -> broadcast::Receiver<Packet> {
        self.hub.subscribe()
    }
}

impl Default for Loopback {
    fn default() -> Self {
        Self::new()
    }
}

async fn listen(
    mut receiver: broadcast::Receiver<Packet>,
    shared: Weak<Shared>,
    stop: CancellationToken,
) {
    loop {
        let packet = tokio::select! {
            () = stop.cancelled() => return,
            packet = receiver.recv() => packet,
        };
        match packet {
            Ok(packet) => {
                let Some(shared) = shared.upgrade() else {
                    return;
                };
                shared.receive(&packet.channel, packet.bytes);
            }
            Err(RecvError::Lagged(skipped)) => {
                tracing::debug!(
                    skipped,
                    "The loopback bus skipped envelopes it fell behind on"
                );
            }
            Err(RecvError::Closed) => return,
        }
    }
}
