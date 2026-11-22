use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    /// A Send whose message id this replica already took, resent after its
    /// acknowledgement went missing.
    Duplicate,
    /// The user may no longer write to the chat.
    Refused,
}
