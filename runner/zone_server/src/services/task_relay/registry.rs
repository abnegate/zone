use serde_json::Value;
use uuid::Uuid;

use crate::agent::question::Answer;

/// What one process holds that only it can deliver to: the runs parked on a
/// question here, and the sockets here listening for titles and action cards.
pub trait Registry: Send + Sync + 'static {
    /// Resolves the run waiting here on `run`. Returns whether one was.
    fn answer(&self, run: Uuid, answers: Vec<Answer>) -> bool;

    fn title(&self, chat: Uuid, title: String);

    fn action(&self, chat: Uuid, message: Value);
}
