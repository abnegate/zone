use abnegate_llm::Usage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub model: String,
    /// `None` when the provider reported nothing, which is not the same as zero.
    pub reported: Option<Usage>,
    pub rounds: usize,
    pub tool_calls: usize,
}

impl Share {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            reported: None,
            rounds: 0,
            tool_calls: 0,
        }
    }
}
