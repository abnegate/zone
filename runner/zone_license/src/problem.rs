use crate::private_key_error::PrivateKeyError;
use crate::untrusted::Untrusted;
use crate::variable::Variable;

#[derive(Debug, thiserror::Error)]
pub enum Problem {
    #[error("{} is not set.", .0.name())]
    Missing(Variable),
    #[error("{} is invalid: {reason}", variable.name())]
    Invalid { variable: Variable, reason: String },
    #[error(transparent)]
    PrivateKey(#[from] PrivateKeyError),
    #[error("{variable}: {0} Use a key this release trusts.", variable = Variable::KeyId.name())]
    Untrusted(#[from] Untrusted),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_variable_is_named() {
        assert_eq!(
            Problem::Missing(Variable::WebhookSecret).to_string(),
            "POLAR_WEBHOOK_SECRET is not set."
        );
    }

    #[test]
    fn an_invalid_variable_is_named_with_the_reason() {
        let problem = Problem::Invalid {
            variable: Variable::SmtpPort,
            reason: "not a port number".into(),
        };
        assert_eq!(
            problem.to_string(),
            "SMTP_PORT is invalid: not a port number"
        );
    }

    #[test]
    fn an_untrusted_key_names_the_key_id_variable() {
        let problem = Problem::from(Untrusted::UnknownKeyId {
            id: "2099-1".into(),
        });
        assert_eq!(
            problem.to_string(),
            "ZONE_LICENSE_KEY_ID: The key id 2099-1 is not in the keyring. Use a key this release trusts."
        );
    }
}
