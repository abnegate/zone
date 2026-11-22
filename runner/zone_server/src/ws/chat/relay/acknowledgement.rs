use serde::{Deserialize, Serialize};

use super::outcome::Outcome;

/// The turn owner's answer to a request. `frames` are serialized frames for
/// the asking connection alone; `sequence` is the last frame the owner had
/// published when it answered, which a replay's frames already include.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acknowledgement {
    pub outcome: Outcome,
    pub sequence: u64,
    pub frames: Vec<String>,
}
