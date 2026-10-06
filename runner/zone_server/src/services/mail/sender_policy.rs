//! What a relay does without `SMTP_FROM`.

/// Whether a relay may fall back to a default sender address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenderPolicy {
    /// Send from [`DEFAULT_SENDER`](super::config::DEFAULT_SENDER) when
    /// `SMTP_FROM` is unset or blank.
    Default,
    /// Refuse the relay unless `SMTP_FROM` names an address.
    Required,
}
