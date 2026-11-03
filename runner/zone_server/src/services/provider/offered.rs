use abnegate_llm::ReasoningEffort;
use serde::Serialize;

/// A model a provider offers one chat, and the reasoning efforts it takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offered {
    pub name: String,
    pub label: String,
    pub efforts: &'static [ReasoningEffort],
}
