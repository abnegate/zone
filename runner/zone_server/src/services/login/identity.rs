use uuid::Uuid;
use zone_core::llm::AgentKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginIdentity {
    pub id: Uuid,
    pub agent: AgentKind,
    pub label: String,
}
