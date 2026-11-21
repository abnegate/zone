#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    #[error("The webhook request has no {0} header.")]
    Missing(&'static str),
    #[error("The webhook timestamp is not a whole number of seconds.")]
    Timestamp,
    #[error("The webhook timestamp is outside the accepted window.")]
    Stale,
    #[error("No webhook signature matches the configured secret.")]
    Mismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_names_the_header() {
        assert_eq!(
            SignatureError::Missing("webhook-id").to_string(),
            "The webhook request has no webhook-id header."
        );
    }
}
