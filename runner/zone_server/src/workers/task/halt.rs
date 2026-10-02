use zone_core::llm::{Limit, LlmBackend};

use super::Fault;

/// Why a task's turn stopped short of an answer.
#[derive(Debug)]
pub(super) enum Halt {
    /// The turn failed, in the words the run log records.
    Failed(String),
    /// A usage limit refused the turn. Its message is the words the run log
    /// recorded for the same refusal when it was a failure.
    Limited(Box<Limit>),
}

impl Halt {
    /// The attempt's fault, for a turn that halted on `backend`.
    pub(super) fn fault(self, backend: &LlmBackend) -> Fault {
        match self {
            Self::Failed(message) => Fault::agent(backend, message),
            Self::Limited(limit) => Fault::limited(backend, limit),
        }
    }
}

impl From<String> for Halt {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}
