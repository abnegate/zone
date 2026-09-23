//! How much of a Claude account a sign-in asks for.

use serde::{Deserialize, Serialize};

use super::LIFETIME;

const DELIMITER: &str = " ";
const INFERENCE: &[&str] = &["user:inference"];
const FULL: &[&str] = &[
    "org:create_api_key",
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
    "user:mcp_servers",
    "user:file_upload",
    "user:plugins",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    Inference,
    Full,
}

impl Scope {
    pub fn scopes(self) -> &'static [&'static str] {
        match self {
            Self::Inference => INFERENCE,
            Self::Full => FULL,
        }
    }

    pub(super) fn parameter(self) -> String {
        self.scopes().join(DELIMITER)
    }

    pub(super) fn lifetime(self) -> Option<u64> {
        match self {
            Self::Inference => Some(LIFETIME),
            Self::Full => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_access_asks_for_exactly_what_the_cli_asks_for() {
        assert_eq!(Scope::Inference.scopes(), ["user:inference"]);
        assert_eq!(
            Scope::Full.scopes(),
            [
                "org:create_api_key",
                "user:profile",
                "user:inference",
                "user:sessions:claude_code",
                "user:mcp_servers",
                "user:file_upload",
                "user:plugins",
            ]
        );
        assert_eq!(Scope::Inference.parameter(), "user:inference");
        assert_eq!(
            Scope::Full.parameter(),
            "org:create_api_key user:profile user:inference user:sessions:claude_code \
             user:mcp_servers user:file_upload user:plugins"
        );
    }

    #[test]
    fn only_an_inference_sign_in_asks_for_a_lifetime() {
        assert_eq!(Scope::Inference.lifetime(), Some(31_536_000));
        assert_eq!(Scope::Full.lifetime(), None);
    }

    #[test]
    fn a_scope_travels_in_lowercase_and_defaults_to_inference() {
        assert_eq!(
            serde_json::to_string(&Scope::Full).expect("serialise"),
            r#""full""#
        );
        assert_eq!(
            serde_json::from_str::<Scope>(r#""inference""#).expect("deserialise"),
            Scope::Inference
        );
        assert_eq!(Scope::default(), Scope::Inference);
    }
}
