use abnegate_agent_cli::AgentKind;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginIdentity {
    pub id: Uuid,
    pub agent: AgentKind,
    pub label: String,
}
