use std::sync::Arc;

use super::ServerMessage;
use super::relay::Relayed;

/// One entry on a chat's local fan-out.
#[derive(Debug, Clone)]
pub(super) enum Frame {
    /// Published on this replica.
    Message(Arc<ServerMessage>),
    /// Published on another replica, sent to sockets exactly as it arrived.
    Relayed(Relayed),
    /// Relayed frames went missing; each connection asks for a fresh replay.
    Gap,
}

impl Frame {
    pub fn message(message: ServerMessage) -> Self {
        Self::Message(Arc::new(message))
    }
}

#[cfg(test)]
impl Frame {
    pub fn published(self) -> ServerMessage {
        match self {
            Self::Message(message) => Arc::unwrap_or_clone(message),
            other => panic!("expected a frame published here, got {other:?}"),
        }
    }
}
