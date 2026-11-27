use std::sync::Arc;

use bytes::Bytes;

/// A sealed envelope on its way to or from one channel.
#[derive(Debug, Clone)]
pub struct Packet {
    pub channel: Arc<str>,
    pub bytes: Bytes,
}
