//! Shrink old tool traces so a longer reason/act budget still fits in context.
//!
//! History is replayed on every round. Raw `read_file` / `search_code` output
//! from early iterations is rarely needed verbatim once later work has moved
//! on; keeping a one-line stand-in is enough for the model to know the call
//! happened.

use crate::llm::{Message, Role};

/// Leave this many most-recent tool results intact.
pub const KEEP_RECENT_TOOL_RESULTS: usize = 6;

/// Bound even the retained recent results so one giant file cannot dominate.
const RECENT_TOOL_CHARS: usize = 8_000;

/// Compact older tool-role messages in place.
///
/// Assistant tool-call envelopes stay untouched so replay pairing still works.
/// Only `role=tool` bodies are rewritten.
pub fn compact_tool_history(messages: &mut [Message], keep_recent: usize) {
    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::Tool)
        .map(|(index, _)| index)
        .collect();
    let compact_until = tool_indices.len().saturating_sub(keep_recent);
    for (order, index) in tool_indices.into_iter().enumerate() {
        let Some(content) = messages[index].content.as_mut() else {
            continue;
        };
        if order < compact_until {
            *content = summarize_tool_output(content);
        } else if content.chars().count() > RECENT_TOOL_CHARS {
            *content = truncate_chars(content, RECENT_TOOL_CHARS);
        }
    }
}

fn summarize_tool_output(content: &str) -> String {
    let failed = content.starts_with("Error:");
    let lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let first = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let first = truncate_chars(first, 160);
    if failed {
        format!("[compacted tool error] {first}")
    } else {
        format!(
            "[compacted tool result] {first} ({lines} lines, {} chars)",
            content.chars().count()
        )
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}…", &text[..byte_idx]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Message;

    #[test]
    fn older_tool_results_are_summarized_recent_ones_kept() {
        let mut messages = vec![
            Message::system("sys"),
            Message::user("do it"),
            Message::tool_result("a", "first result\nmore"),
            Message::tool_result("b", "second result"),
            Message::tool_result("c", "third result"),
        ];
        compact_tool_history(&mut messages, 2);
        assert!(
            messages[2]
                .content
                .as_deref()
                .unwrap()
                .starts_with("[compacted tool result] first result")
        );
        assert_eq!(messages[3].content.as_deref(), Some("second result"));
        assert_eq!(messages[4].content.as_deref(), Some("third result"));
    }

    #[test]
    fn nothing_to_compact_when_under_the_keep_limit() {
        let mut messages = vec![Message::tool_result("a", "only")];
        compact_tool_history(&mut messages, 6);
        assert_eq!(messages[0].content.as_deref(), Some("only"));
    }

    #[test]
    fn errors_keep_the_failure_line() {
        let mut messages = vec![
            Message::tool_result("a", "Error: boom\nstack"),
            Message::tool_result("b", "ok"),
        ];
        compact_tool_history(&mut messages, 1);
        assert_eq!(
            messages[0].content.as_deref(),
            Some("[compacted tool error] Error: boom")
        );
    }

    #[test]
    fn keep_recent_zero_compacts_every_tool_result() {
        let mut messages = vec![
            Message::tool_result("a", "first"),
            Message::tool_result("b", "second"),
        ];
        compact_tool_history(&mut messages, 0);
        assert!(
            messages[0]
                .content
                .as_deref()
                .unwrap()
                .starts_with("[compacted tool result]")
        );
        assert!(
            messages[1]
                .content
                .as_deref()
                .unwrap()
                .starts_with("[compacted tool result]")
        );
    }

    #[test]
    fn missing_and_blank_tool_bodies_are_left_or_summarized_safely() {
        let mut messages = vec![
            Message {
                role: crate::llm::Role::Tool,
                content: None,
                name: None,
                tool_calls: None,
                tool_call_id: Some("empty".into()),
                images: Vec::new(),
                generated_images: Vec::new(),
            },
            Message::tool_result("blank", "   \n\n"),
            Message::tool_result("keep", "recent"),
        ];
        compact_tool_history(&mut messages, 1);
        assert_eq!(messages[0].content, None);
        assert!(
            messages[1]
                .content
                .as_deref()
                .unwrap()
                .starts_with("[compacted tool result]")
        );
        assert_eq!(messages[2].content.as_deref(), Some("recent"));
    }

    #[test]
    fn assistant_and_user_messages_are_not_rewritten() {
        let mut messages = vec![
            Message::user("please read it"),
            Message::assistant("calling a tool"),
            Message::tool_result("a", "old\noutput"),
            Message::tool_result("b", "kept"),
        ];
        compact_tool_history(&mut messages, 1);
        assert_eq!(messages[0].content.as_deref(), Some("please read it"));
        assert_eq!(messages[1].content.as_deref(), Some("calling a tool"));
        assert!(
            messages[2]
                .content
                .as_deref()
                .unwrap()
                .starts_with("[compacted tool result]")
        );
    }

    #[test]
    fn recent_giant_results_truncate_on_a_char_boundary() {
        let giant = "😀".repeat(RECENT_TOOL_CHARS + 40);
        let mut messages = vec![Message::tool_result("a", giant)];
        compact_tool_history(&mut messages, 6);
        let compacted = messages[0].content.as_deref().unwrap();
        assert!(compacted.ends_with('…'));
        assert_eq!(compacted.chars().count(), RECENT_TOOL_CHARS + 1);
        assert!(compacted.is_char_boundary(compacted.len() - "…".len()));
    }
}
