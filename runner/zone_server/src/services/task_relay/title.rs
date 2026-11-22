use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A chat's new title, as another replica sent it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Title {
    pub chat: Uuid,
    pub title: String,
}
