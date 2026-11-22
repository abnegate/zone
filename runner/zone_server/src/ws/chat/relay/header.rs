use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a mirror reads off a relayed frame without parsing the frame itself.
/// `sequence` counts the frames one replica published to the chat, and
/// `terminator` marks the frame that ends a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub chat: Uuid,
    pub sequence: u64,
    pub terminator: bool,
}
