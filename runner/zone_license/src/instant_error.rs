#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected an RFC 3339 timestamp or a YYYY-MM-DD date")]
pub struct InstantError;
