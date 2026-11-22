use bytes::Bytes;
use uuid::Uuid;

/// A frame another replica published, kept as the JSON it was sent as.
#[derive(Debug, Clone)]
pub struct Relayed {
    pub origin: Uuid,
    pub sequence: u64,
    pub json: Bytes,
}
