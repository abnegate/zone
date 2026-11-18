use abnegate_secret::sanitize_owned;
use thiserror::Error;

use crate::mail::SEND_TIMEOUT;
use crate::mail::failure::Failure;

/// Addresses are personal data and these messages head for log lines, so no
/// variant quotes an address or keeps the text of a relay's reply.
#[derive(Debug, Error)]
pub enum MailError {
    #[error("the recipient address could not be parsed")]
    Recipient,
    #[error("the sender address could not be parsed")]
    Sender,
    #[error("the relay at {host} could not be configured: {message}")]
    Configuration { host: String, message: String },
    #[error("the message could not be composed: {0}")]
    Compose(String),
    #[error(
        "the relay at {host} did not take the message: {failure}, {}, {}",
        status.map_or_else(|| "no status".to_owned(), |code| format!("status {code}")),
        if *permanent { "permanent" } else { "transient" }
    )]
    Delivery {
        host: String,
        failure: Failure,
        status: Option<u16>,
        permanent: bool,
    },
    #[error("the relay at {host} did not take the message within {}s", SEND_TIMEOUT.as_secs())]
    Timeout { host: String },
}

impl MailError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Delivery { permanent, .. } => !permanent,
            Self::Timeout { .. } => true,
            Self::Recipient | Self::Sender | Self::Configuration { .. } | Self::Compose(_) => false,
        }
    }

    pub(crate) fn configuration(host: &str, error: &lettre::transport::smtp::Error) -> Self {
        Self::Configuration {
            host: host.to_string(),
            message: sanitize_owned(error.to_string()),
        }
    }

    pub(crate) fn delivery(host: &str, error: &lettre::transport::smtp::Error) -> Self {
        Self::Delivery {
            host: host.to_string(),
            failure: Failure::of(error),
            status: error.status().map(u16::from),
            permanent: error.is_permanent() || error.is_client(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delivery(permanent: bool) -> MailError {
        MailError::Delivery {
            host: "smtp.example.test".to_string(),
            failure: Failure::Rejected,
            status: Some(if permanent { 550 } else { 451 }),
            permanent,
        }
    }

    #[test]
    fn a_timeout_is_retried() {
        let error = MailError::Timeout {
            host: "smtp.example.test".to_string(),
        };
        assert!(error.is_retryable());
        assert_eq!(
            error.to_string(),
            "the relay at smtp.example.test did not take the message within 7s"
        );
    }

    #[test]
    fn a_delivery_failure_is_retried_only_when_it_is_transient() {
        assert!(delivery(false).is_retryable());
        assert!(!delivery(true).is_retryable());
    }

    #[test]
    fn a_delivery_failure_names_its_kind_and_status() {
        assert_eq!(
            delivery(true).to_string(),
            "the relay at smtp.example.test did not take the message: rejected, status 550, permanent"
        );
        let unreachable = MailError::Delivery {
            host: "smtp.example.test".to_string(),
            failure: Failure::Connection,
            status: None,
            permanent: false,
        };
        assert_eq!(
            unreachable.to_string(),
            "the relay at smtp.example.test did not take the message: connection, no status, transient"
        );
    }

    #[test]
    fn a_bad_address_or_message_is_never_retried() {
        for error in [
            MailError::Recipient,
            MailError::Sender,
            MailError::Compose("missing from".to_string()),
            MailError::Configuration {
                host: "smtp.example.test".to_string(),
                message: "invalid domain".to_string(),
            },
        ] {
            assert!(!error.is_retryable(), "{error}");
        }
    }
}
