//! What one of an agent's own tools printed.

use abnegate_llm::ToolCall;

#[derive(Debug, Clone)]
pub struct Output {
    pub call: ToolCall,
    pub text: String,
    pub succeeded: bool,
}

impl Output {
    pub fn new(call: ToolCall, text: impl Into<String>, succeeded: bool) -> Self {
        Self {
            call,
            text: text.into(),
            succeeded,
        }
    }
}
