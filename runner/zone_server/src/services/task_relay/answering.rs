use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::question::Answer;

/// Answers for a run parked on a question on whichever replica runs it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Answering {
    pub run: Uuid,
    pub answers: Vec<Answer>,
}
