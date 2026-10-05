//! Why account mail is unavailable, or a message did not go out.

use crate::services::mail::Variable;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{} is not set", .0.name())]
    Missing(Variable),
    #[error(transparent)]
    Delivery(#[from] abnegate_notify::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_variable_is_named() {
        assert_eq!(
            Error::Missing(Variable::Host).to_string(),
            "SMTP_HOST is not set"
        );
    }
}
