//! Compatibility entry point for callers without a verified context budget.
//!
//! Canonical evidence must never be rewritten. Budget-aware callers use
//! `context::prepare`, which produces a separate validated projection.

use crate::llm::Message;

/// Retained for callers migrating from the former count-based API.
pub const KEEP_RECENT_TOOL_RESULTS: usize = 6;

/// Without consumption flags or capacity information no safe compaction can
/// occur. In particular, a large or seventh fresh result is still evidence.
pub fn compact_tool_history(_messages: &mut [Message], _keep_recent: usize) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_fresh_results_are_never_truncated() {
        let body = format!("{}\nTAIL_EVIDENCE", "x".repeat(9_000));
        let mut messages: Vec<_> = (0..7)
            .map(|index| Message::tool_result(index.to_string(), body.clone()))
            .collect();
        compact_tool_history(&mut messages, 6);
        assert!(
            messages
                .iter()
                .all(|message| message.content.as_ref() == Some(&body))
        );
    }

    #[test]
    fn regression_repeated_compaction_preserves_canonical_error() {
        let original = "Error: command failed\ncritical stack and correction";
        let mut messages = vec![Message::tool_result("old", original)];
        for _ in 0..12 {
            compact_tool_history(&mut messages, 0);
        }
        assert_eq!(messages[0].content.as_deref(), Some(original));
    }
}
