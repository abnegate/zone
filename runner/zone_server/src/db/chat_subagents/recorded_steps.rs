use zone_core::llm::SubagentStep;

/// The steps one write changed, in their order, and how many of them are new.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordedSteps {
    pub changed: Vec<SubagentStep>,
    pub added: i64,
}
