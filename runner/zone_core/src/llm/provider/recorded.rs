//! Reading back what a stand-in agent was sent, for tests that record it.

use serde_json::Value;

use super::transcript::SEPARATOR;

/// The prompt text an agent read from `stdin`: a claude stream-json message's
/// text blocks, or codex's stdin as it is.
pub fn prompt(stdin: &str) -> String {
    let Ok(message) = serde_json::from_str::<Value>(stdin.trim_end()) else {
        return stdin.to_string();
    };
    message["message"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join(SEPARATOR)
        })
        .unwrap_or_else(|| stdin.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claude_message_reads_back_as_its_text() {
        let stdin = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"User:\\nping\"}]}}\n";

        assert_eq!(prompt(stdin), "User:\nping");
    }

    #[test]
    fn codex_stdin_reads_back_as_it_was_written() {
        assert_eq!(prompt("User:\nping"), "User:\nping");
    }
}
