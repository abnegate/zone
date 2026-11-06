use crate::llm::provider::agent::AgentKind;
use crate::llm::provider::image::Rejection;

/// Why an image on the message being answered cannot reach the agent.
#[derive(Debug, thiserror::Error)]
pub enum Undeliverable {
    #[error("the image attached to this message cannot be given to {agent}: {rejection}")]
    Refused {
        agent: AgentKind,
        rejection: Rejection,
    },
    #[error("the image attached to this message could not be saved for codex: {0}")]
    Unsaved(#[from] std::io::Error),
}
