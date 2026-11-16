//! A turn the agent policy refuses: the error that carries the refusal out of the turn's
//! preparation, the frame that tells the chat, and the audit row that records it.

use std::error::Error;

use uuid::Uuid;
use zone_enterprise::policy::Denial;

use super::ServerMessage;
use crate::db::audit::actions::AGENT_POLICY_TURN_DENIED;
use crate::services::backend;
use crate::services::policy::{self, Denied, Subject};
use crate::services::route::Unusable;
use crate::state::AppState;

/// The policy refused `model`, the model the turn would have run on.
#[derive(Debug, thiserror::Error)]
#[error("{denial}")]
pub(super) struct Refused {
    pub(super) denial: Denial,
    pub(super) model: String,
}

/// `actor`'s turn in `chat`, one of `workspace`'s, whose refusals are audited against the chat.
#[derive(Debug, Clone, Copy)]
pub(super) struct Turn {
    pub(super) chat: Uuid,
    pub(super) workspace: Uuid,
    pub(super) actor: Uuid,
}

impl Turn {
    /// Audits `denial`, naming `detail`, the model it refused, when it refused one.
    pub(super) async fn record(self, state: &AppState, denial: Denial, detail: Option<&str>) {
        let denied = Denied {
            workspace: self.workspace,
            organization: None,
            actor: Some(self.actor),
            subject: Subject::Chat(self.chat),
            denial,
            detail,
        };
        policy::record(state, AGENT_POLICY_TURN_DENIED, &denied).await;
    }

    /// The frame the turn `error` stopped ends with: a [`ServerMessage::Denied`], once audited,
    /// when the policy refused it, and a [`ServerMessage::Error`] otherwise.
    pub(super) async fn ended(
        self,
        state: &AppState,
        error: &(dyn Error + Send + Sync + 'static),
    ) -> ServerMessage {
        match refusal(error) {
            Some((denial, detail)) => {
                self.record(state, denial, detail).await;
                frame(denial)
            }
            None => ServerMessage::Error {
                message: error.to_string(),
            },
        }
    }
}

pub(super) fn frame(denial: Denial) -> ServerMessage {
    ServerMessage::Denied {
        message: denial.to_string(),
        code: denial.code().to_string(),
    }
}

/// The policy denial `error` is, with the model it refused when it refused one: directly, as the
/// reason the turn's route is unusable, or as why its backend could not be resolved.
fn refusal<'a>(
    error: &'a (dyn Error + Send + Sync + 'static),
) -> Option<(Denial, Option<&'a str>)> {
    if let Some(refused) = error.downcast_ref::<Refused>() {
        return Some((refused.denial, Some(&refused.model)));
    }
    error
        .downcast_ref::<Denial>()
        .copied()
        .or_else(|| error.downcast_ref::<Unusable>().and_then(Unusable::denial))
        .or_else(|| {
            error
                .downcast_ref::<backend::Error>()
                .and_then(backend::Error::denial)
        })
        .map(|denial| (denial, None))
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::AgentKind;
    use zone_enterprise::policy::Agent;

    use super::*;
    use crate::services::route::Reason;

    #[test]
    fn a_refusal_is_read_from_every_error_a_turn_is_refused_with() {
        let agent = Denial::Agent(Agent::Codex);
        let refused = Refused {
            denial: Denial::Model,
            model: "gpt-4o".into(),
        };
        let unusable = Unusable::from(Reason::Denied(Denial::Unreadable));

        assert_eq!(refusal(&refused), Some((Denial::Model, Some("gpt-4o"))));
        assert_eq!(refusal(&agent), Some((agent, None)));
        assert_eq!(refusal(&unusable), Some((Denial::Unreadable, None)));
        assert_eq!(refusal(&backend::Error::Denied(agent)), Some((agent, None)));
        assert_eq!(
            refusal(&backend::Error::Unusable(unusable)),
            Some((Denial::Unreadable, None))
        );
        assert_eq!(refusal(&Unusable::from(Reason::Unreadable)), None);
        assert_eq!(
            refusal(&backend::Error::SignedOut {
                agent: AgentKind::Claude
            }),
            None
        );
    }

    #[test]
    fn a_refusal_is_framed_with_its_message_and_code() {
        let denial = Denial::Model;
        let frame = serde_json::to_value(frame(denial)).expect("the frame serialises");

        assert_eq!(
            frame,
            serde_json::json!({
                "type": "denied",
                "message": denial.to_string(),
                "code": "policy_model_denied",
            })
        );
    }
}
