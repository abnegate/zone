use crate::policy::field::Field;

pub const WIDER: &str = "policy_wider_than_organization";
pub const MODEL_INVALID: &str = "policy_model_invalid";
pub const DOMAIN_INVALID: &str = "policy_domain_invalid";
pub const SERVER_INVALID: &str = "policy_mcp_server_invalid";
pub const CEILING_INVALID: &str = "policy_ceiling_invalid";
pub const BODY_INVALID: &str = "policy_body_invalid";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "This workspace's {} can't allow more than your organization's policy does.",
        .0.label()
    )]
    Wider(Field),
    #[error(
        "\"{0}\" is not a valid model pattern. Use a model name, or a prefix ending in a single *, of up to 128 characters with no spaces."
    )]
    ModelInvalid(String),
    #[error(
        "\"{0}\" is not a valid domain. Use a hostname such as example.com or an IP address, with no scheme, port or path."
    )]
    DomainInvalid(String),
    #[error(
        "\"{0}\" is not a valid MCP server name. Use 1 to 32 letters, digits, hyphens or underscores."
    )]
    ServerInvalid(String),
    #[error("The tool approval ceiling must be write or host, or empty for no limit.")]
    CeilingInvalid,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Wider(_) => WIDER,
            Self::ModelInvalid(_) => MODEL_INVALID,
            Self::DomainInvalid(_) => DOMAIN_INVALID,
            Self::ServerInvalid(_) => SERVER_INVALID,
            Self::CeilingInvalid => CEILING_INVALID,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every() -> [Error; 5] {
        [
            Error::Wider(Field::Models),
            Error::ModelInvalid("claude *".into()),
            Error::DomainInvalid("https://example.com".into()),
            Error::ServerInvalid("git hub".into()),
            Error::CeilingInvalid,
        ]
    }

    #[test]
    fn codes_are_the_wire_contract() {
        let codes: Vec<&str> = every().iter().map(Error::code).collect();
        assert_eq!(
            codes,
            [
                "policy_wider_than_organization",
                "policy_model_invalid",
                "policy_domain_invalid",
                "policy_mcp_server_invalid",
                "policy_ceiling_invalid",
            ]
        );
    }

    #[test]
    fn constants_are_the_wire_contract() {
        assert_eq!(WIDER, "policy_wider_than_organization");
        assert_eq!(MODEL_INVALID, "policy_model_invalid");
        assert_eq!(DOMAIN_INVALID, "policy_domain_invalid");
        assert_eq!(SERVER_INVALID, "policy_mcp_server_invalid");
        assert_eq!(CEILING_INVALID, "policy_ceiling_invalid");
        assert_eq!(BODY_INVALID, "policy_body_invalid");
    }

    #[test]
    fn every_message_is_one_line() {
        for error in every() {
            let message = error.to_string();
            assert!(!message.is_empty() && !message.contains('\n'), "{error:?}");
        }
    }

    #[test]
    fn messages_name_the_entry_or_field() {
        assert!(
            Error::ModelInvalid("claude *".into())
                .to_string()
                .contains("\"claude *\"")
        );
        assert!(
            Error::DomainInvalid("https://example.com".into())
                .to_string()
                .contains("https://example.com")
        );
        assert!(
            Error::ServerInvalid("git hub".into())
                .to_string()
                .contains("git hub")
        );
        assert_eq!(
            Error::Wider(Field::McpServers).to_string(),
            "This workspace's MCP servers can't allow more than your organization's policy does."
        );
    }
}
