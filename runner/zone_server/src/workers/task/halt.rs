use zone_core::llm::Limit;

/// Why a task's turn stopped short of an answer.
#[derive(Debug)]
pub(super) enum Halt {
    /// The turn failed, in the words the run log records.
    Failed(String),
    /// A usage limit refused the turn. Its message is the words the run log
    /// recorded for the same refusal when it was a failure.
    Limited(Box<Limit>),
}

impl From<String> for Halt {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}
