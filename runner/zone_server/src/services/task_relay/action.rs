use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A message stored in a chat outside a turn, as another replica sent it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Action {
    pub chat: Uuid,
    pub message: Value,
}
