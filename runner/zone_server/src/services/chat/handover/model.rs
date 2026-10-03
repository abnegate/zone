use zone_core::llm::AgentKind;

use crate::services::stages::{self, Catalog, Preferences};

/// What a turn's model was chosen from, so a turn that moves to another agent picks again from
/// that agent's catalog as its first round did.
#[derive(Debug, Clone)]
pub struct Model {
    /// The model the chat asks for.
    pub requested: String,
    pub preferences: Preferences,
    /// The message the turn answers.
    pub message: String,
    pub image: bool,
    pub agentic: bool,
}

impl Model {
    /// The model `agent` runs the turn on.
    pub fn on(&self, agent: AgentKind) -> String {
        stages::chat_model(
            &self.requested,
            &self.preferences,
            &Catalog::agent(agent),
            &self.message,
            self.image,
            self.agentic,
        )
    }
}
