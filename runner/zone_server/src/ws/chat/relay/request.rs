use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::command::Command;

/// What a replica asks of the one running a chat's turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// The frames a connection joining now would be shown.
    Replay,
    /// Apply `command` as `user`, signed in under `session`, unless it is
    /// past `deadline`: by then the asking replica has stopped waiting, and
    /// may have told its socket the chat is busy.
    Control {
        user: Uuid,
        session: Uuid,
        deadline: DateTime<Utc>,
        command: Command,
    },
}
