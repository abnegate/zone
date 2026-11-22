use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The owner's reply once the answers resolved its run. Only the owner
/// replies, so a replica with nothing waiting cannot win the race with a no.
#[derive(Debug, Serialize, Deserialize)]
pub struct Answered {
    pub run: Uuid,
}
