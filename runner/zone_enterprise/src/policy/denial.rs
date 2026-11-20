use crate::policy::agent::Agent;
use crate::policy::provider::Provider;

pub const PROVIDER_DENIED: &str = "policy_provider_denied";
pub const AGENT_DENIED: &str = "policy_agent_denied";
pub const MODEL_DENIED: &str = "policy_model_denied";
pub const SERVER_DENIED: &str = "policy_mcp_server_denied";
pub const DOMAIN_DENIED: &str = "policy_domain_denied";
pub const APPROVAL_REQUIRED: &str = "policy_approval_required";
pub const UNREADABLE: &str = "policy_unreadable";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Denial {
    #[error("Your organization's policy doesn't allow {} in this workspace.", .0.label())]
    Provider(Provider),
    #[error("Your organization's policy doesn't allow {} in this workspace.", .0.label())]
    Agent(Agent),
    #[error(
        "Your organization's policy doesn't allow this model in this workspace. Choose an allowed model."
    )]
    Model,
    #[error("Your organization's policy doesn't allow this MCP server in this workspace.")]
    Server,
    #[error(
        "Your organization's policy doesn't allow requests to this domain from this workspace."
    )]
    Domain,
    #[error(
        "Your organization's policy requires a person to approve this tool call, and nobody can approve it in this run."
    )]
    Approval,
    #[error(
        "Your organization's agent policy couldn't be read, so this can't start. Try again shortly."
    )]
    Unreadable,
}

impl Denial {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Provider(_) => PROVIDER_DENIED,
            Self::Agent(_) => AGENT_DENIED,
            Self::Model => MODEL_DENIED,
            Self::Server => SERVER_DENIED,
            Self::Domain => DOMAIN_DENIED,
            Self::Approval => APPROVAL_REQUIRED,
            Self::Unreadable => UNREADABLE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every() -> [Denial; 7] {
        [
            Denial::Provider(Provider::Bedrock),
            Denial::Agent(Agent::Codex),
            Denial::Model,
            Denial::Server,
            Denial::Domain,
            Denial::Approval,
            Denial::Unreadable,
        ]
    }

    #[test]
    fn codes_are_the_wire_contract() {
        let codes: Vec<&str> = every().iter().map(Denial::code).collect();
        assert_eq!(
            codes,
            [
                "policy_provider_denied",
                "policy_agent_denied",
                "policy_model_denied",
                "policy_mcp_server_denied",
                "policy_domain_denied",
                "policy_approval_required",
                "policy_unreadable",
            ]
        );
    }

    #[test]
    fn constants_are_the_wire_contract() {
        assert_eq!(PROVIDER_DENIED, "policy_provider_denied");
        assert_eq!(AGENT_DENIED, "policy_agent_denied");
        assert_eq!(MODEL_DENIED, "policy_model_denied");
        assert_eq!(SERVER_DENIED, "policy_mcp_server_denied");
        assert_eq!(DOMAIN_DENIED, "policy_domain_denied");
        assert_eq!(APPROVAL_REQUIRED, "policy_approval_required");
        assert_eq!(UNREADABLE, "policy_unreadable");
    }

    #[test]
    fn messages_are_the_contract() {
        let messages: Vec<String> = every().iter().map(Denial::to_string).collect();
        assert_eq!(
            messages,
            [
                "Your organization's policy doesn't allow AWS Bedrock in this workspace.",
                "Your organization's policy doesn't allow Codex in this workspace.",
                "Your organization's policy doesn't allow this model in this workspace. Choose an allowed model.",
                "Your organization's policy doesn't allow this MCP server in this workspace.",
                "Your organization's policy doesn't allow requests to this domain from this workspace.",
                "Your organization's policy requires a person to approve this tool call, and nobody can approve it in this run.",
                "Your organization's agent policy couldn't be read, so this can't start. Try again shortly.",
            ]
        );
    }

    #[test]
    fn names_every_provider_and_agent_by_label() {
        for provider in Provider::ALL {
            assert!(
                Denial::Provider(provider)
                    .to_string()
                    .contains(provider.label())
            );
        }
        for agent in Agent::ALL {
            assert!(Denial::Agent(agent).to_string().contains(agent.label()));
        }
    }
}
