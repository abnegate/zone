use super::{Availability, Provider};

const PROVIDER_UNKNOWN: &str = "provider_unknown";
const PROVIDER_MODEL_INVALID: &str = "provider_model_invalid";
const PROVIDER_MODEL_REQUIRED: &str = "provider_model_required";
const PROVIDER_UNAVAILABLE: &str = "provider_unavailable";

/// Why a provider and model can't be chosen for a chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("Zone has no AI provider by that name.")]
    Unnamed,
    #[error("A chosen model can't be blank.")]
    Blank,
    #[error("A model can only be chosen together with its provider.")]
    Orphaned,
    #[error("{} doesn't offer that model.", .0.label())]
    Unoffered(Provider),
    #[error("{}", .1.reason(*.0).unwrap_or_default())]
    Unavailable(Provider, Availability),
    #[error("{} has no model saved in AI Settings, so choose one for this chat.", .0.label())]
    Unset(Provider),
    #[error("The workspace's AI settings couldn't be read.")]
    Unreadable,
}

impl Refusal {
    /// The code a client tells refusals apart by. Settings that couldn't be
    /// read are no fault of the choice and carry none.
    pub fn code(self) -> Option<&'static str> {
        match self {
            Self::Unnamed => Some(PROVIDER_UNKNOWN),
            Self::Blank | Self::Orphaned | Self::Unoffered(_) => Some(PROVIDER_MODEL_INVALID),
            Self::Unset(_) => Some(PROVIDER_MODEL_REQUIRED),
            Self::Unavailable(..) => Some(PROVIDER_UNAVAILABLE),
            Self::Unreadable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_carries_its_contract_code() {
        for (refusal, code) in [
            (Refusal::Unnamed, Some("provider_unknown")),
            (Refusal::Blank, Some("provider_model_invalid")),
            (Refusal::Orphaned, Some("provider_model_invalid")),
            (
                Refusal::Unoffered(Provider::ClaudeCode),
                Some("provider_model_invalid"),
            ),
            (
                Refusal::Unset(Provider::OpenAI),
                Some("provider_model_required"),
            ),
            (
                Refusal::Unavailable(Provider::Anthropic, Availability::Unconfigured),
                Some("provider_unavailable"),
            ),
            (
                Refusal::Unavailable(Provider::Bedrock, Availability::Unchoosable),
                Some("provider_unavailable"),
            ),
            (Refusal::Unreadable, None),
        ] {
            assert_eq!(refusal.code(), code, "{refusal:?}");
            let message = refusal.to_string();
            assert!(
                message.ends_with('.') && !message.ends_with(".."),
                "{refusal:?} says why in one sentence: {message:?}"
            );
        }
    }

    #[test]
    fn an_unavailable_provider_is_refused_with_its_reason() {
        let refusal = Refusal::Unavailable(Provider::Codex, Availability::SignedOut);
        assert_eq!(
            refusal.to_string(),
            Availability::SignedOut
                .reason(Provider::Codex)
                .expect("a reason")
        );
        assert_eq!(
            Refusal::Unoffered(Provider::ClaudeCode).to_string(),
            "Claude Code doesn't offer that model."
        );
    }
}
