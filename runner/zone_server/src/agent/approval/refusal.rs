//! Why a call put to the reader did not run.

use serde::{Deserialize, Serialize};
use zone_core::tools::{APPROVAL_CANCELLED, APPROVAL_TIMED_OUT, DENIED};

/// Stored with the call and sent with its result, so the console labels a refused call from
/// what refused it rather than from the words its outcome happens to start with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The reader said no.
    Denied,
    /// Nobody answered in time.
    TimedOut,
    /// The turn stopped first.
    Cancelled,
}

impl Refusal {
    /// What the call is told in place of running.
    pub const fn told(self) -> &'static str {
        match self {
            Self::Denied => DENIED,
            Self::TimedOut => APPROVAL_TIMED_OUT,
            Self::Cancelled => APPROVAL_CANCELLED,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
