//! Why a call put to the reader did not run.

use serde::{Deserialize, Serialize};

/// What a failed call's message starts with, as the agent's tool result
/// renders it. The console reads a refusal back from its words with or
/// without it.
pub const ERROR_PREFIX: &str = "Error: ";

/// What a call the reader did not allow is told, wherever it was asked. The
/// console reads a denial back from these words once the approval is gone.
pub const DENIED: &str = "The user denied this tool call.";

/// What a call is told when nobody answered its approval in time. Not the
/// reader's answer, so it reads apart from [`DENIED`].
pub const APPROVAL_TIMED_OUT: &str =
    "Nobody answered the approval request in time, so this tool call did not run.";

/// What a call is told when its turn stopped while it waited for approval.
pub const APPROVAL_CANCELLED: &str =
    "The turn stopped before this tool call was approved, so it did not run.";

/// Stored with the call and sent with its result, so the console labels a refused call from
/// what refused it rather than from the words its outcome happens to start with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Denied,
    TimedOut,
    Cancelled,
}

impl Refusal {
    const ALL: [Self; 3] = [Self::Denied, Self::TimedOut, Self::Cancelled];

    /// What the call is told in place of running.
    pub const fn message(self) -> &'static str {
        match self {
            Self::Denied => DENIED,
            Self::TimedOut => APPROVAL_TIMED_OUT,
            Self::Cancelled => APPROVAL_CANCELLED,
        }
    }

    /// The refusal a call's outcome line states, as a permission card states
    /// it or as a failed call's error.
    pub fn worded(detail: &str) -> Option<Self> {
        let bare = detail.strip_prefix(ERROR_PREFIX).unwrap_or(detail);
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.message() == bare)
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent::tool::ToolResult;

    use super::*;

    #[test]
    fn a_refusal_is_sent_under_the_names_the_console_reads() {
        for (refusal, name) in [
            (Refusal::Denied, "denied"),
            (Refusal::TimedOut, "timed_out"),
            (Refusal::Cancelled, "cancelled"),
        ] {
            assert_eq!(
                serde_json::to_value(refusal).expect("a refusal serializes"),
                serde_json::json!(name)
            );
        }
    }

    #[test]
    fn a_failed_call_is_prefixed_as_the_console_reads() {
        assert_eq!(
            ToolResult::error("x").to_message(),
            format!("{ERROR_PREFIX}x")
        );
    }

    #[test]
    fn a_refusal_reads_back_from_its_words() {
        for refusal in Refusal::ALL {
            assert_eq!(Refusal::worded(refusal.message()), Some(refusal));
            assert_eq!(
                Refusal::worded(&format!("{ERROR_PREFIX}{}", refusal.message())),
                Some(refusal)
            );
        }
        assert_eq!(Refusal::worded("Error: something else"), None);
    }
}
