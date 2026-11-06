use abnegate_llm::ProviderError;
use http::StatusCode;

use crate::error::ContextError;

const UNAUTHORIZED: u16 = 401;
const FORBIDDEN: u16 = 403;
const TOO_MANY_REQUESTS: u16 = 429;
const RATE_LIMIT_RETRY_SECONDS: u64 = 60;

/// The [`ContextError`] an embedding provider's failure becomes, worded so
/// [`ContextError::is_context_length`] and
/// [`ContextError::is_fatal_embedding`] classify it as they did before the
/// providers moved into `abnegate-llm`.
pub(super) fn context_error(error: ProviderError) -> ContextError {
    match error {
        ProviderError::Api {
            status, message, ..
        } => refusal(status, &message),
        ProviderError::Limited { .. } => rate_limited(),
        ProviderError::Network { detail, .. } => {
            ContextError::Embedding(format!("Request failed: {detail}"))
        }
        ProviderError::Parse { detail, .. } => {
            ContextError::Embedding(format!("Failed to parse response: {detail}"))
        }
        ProviderError::Agent { message, .. } => {
            ContextError::Embedding(format!("Local inference failed: {message}"))
        }
        ProviderError::Config { detail, .. } => ContextError::InvalidSourceConfig(detail),
        other => ContextError::Embedding(other.to_string()),
    }
}

fn refusal(status: u16, message: &str) -> ContextError {
    let status_line =
        StatusCode::from_u16(status).map_or_else(|_| status.to_string(), |code| code.to_string());
    match status {
        UNAUTHORIZED | FORBIDDEN => {
            ContextError::Auth(format!("Authentication failed ({status_line}): {message}"))
        }
        TOO_MANY_REQUESTS => rate_limited(),
        _ => ContextError::Embedding(format!("API returned error {status_line}: {message}")),
    }
}

fn rate_limited() -> ContextError {
    ContextError::RateLimited {
        retry_after_secs: RATE_LIMIT_RETRY_SECONDS,
    }
}

#[cfg(test)]
mod tests {
    use abnegate_llm::Limit;

    use super::*;

    const CONTEXT_LENGTH_MESSAGE: &str = "the input length exceeds the context length";
    const UNKNOWN_STATUS: u16 = 599;

    fn mapped(error: ProviderError) -> ContextError {
        context_error(error)
    }

    #[test]
    fn a_rejected_key_is_a_fatal_auth_failure() {
        for (status, line) in [(401, "401 Unauthorized"), (403, "403 Forbidden")] {
            let error = mapped(ProviderError::api(status, "invalid api key"));
            assert!(
                matches!(&error, ContextError::Auth(message)
                    if message == &format!("Authentication failed ({line}): invalid api key")),
                "{status}: {error:?}"
            );
            assert!(error.is_fatal_embedding());
            assert!(!error.is_context_length());
        }
    }

    #[test]
    fn a_throttle_is_a_fatal_rate_limit() {
        let error = mapped(ProviderError::api(429, "slow down"));
        assert!(matches!(
            error,
            ContextError::RateLimited {
                retry_after_secs: RATE_LIMIT_RETRY_SECONDS
            }
        ));
        assert!(error.is_fatal_embedding());
    }

    #[test]
    fn a_usage_limit_is_a_fatal_rate_limit() {
        let error = mapped(ProviderError::limited("ollama", Limit::new("usage limit")));
        assert!(matches!(
            error,
            ContextError::RateLimited {
                retry_after_secs: RATE_LIMIT_RETRY_SECONDS
            }
        ));
        assert!(error.is_fatal_embedding());
    }

    #[test]
    fn a_context_length_refusal_keeps_its_wording() {
        let error = mapped(ProviderError::api(500, CONTEXT_LENGTH_MESSAGE));
        assert_eq!(
            error.to_string(),
            format!(
                "Embedding generation failed: API returned error 500 Internal Server Error: {CONTEXT_LENGTH_MESSAGE}"
            )
        );
        assert!(error.is_context_length());
        assert!(!error.is_fatal_embedding());
    }

    #[test]
    fn an_unknown_status_is_rendered_as_its_number() {
        let error = mapped(ProviderError::api(UNKNOWN_STATUS, "odd"));
        assert!(
            matches!(&error, ContextError::Embedding(message)
                if message.starts_with(&format!("API returned error {UNKNOWN_STATUS}"))),
            "{error:?}"
        );
    }

    #[test]
    fn transport_parse_and_inference_failures_are_embedding_failures() {
        for (error, expected) in [
            (
                ProviderError::network("connection refused"),
                "Request failed: connection refused",
            ),
            (
                ProviderError::parse("EOF while parsing"),
                "Failed to parse response: EOF while parsing",
            ),
            (
                ProviderError::agent("fastembed", "session failed"),
                "Local inference failed: session failed",
            ),
        ] {
            let error = mapped(error);
            assert!(
                matches!(&error, ContextError::Embedding(message) if message == expected),
                "{error:?}"
            );
            assert!(!error.is_fatal_embedding());
        }
    }

    #[test]
    fn a_configuration_failure_is_an_invalid_source_config() {
        let error = mapped(ProviderError::config("invalid base URL ftp://host"));
        assert!(
            matches!(&error, ContextError::InvalidSourceConfig(detail) if detail == "invalid base URL ftp://host"),
            "{error:?}"
        );
    }

    #[test]
    fn any_other_failure_is_an_embedding_failure_with_its_own_wording() {
        let error = mapped(ProviderError::unsupported("batch embeddings"));
        assert!(
            matches!(&error, ContextError::Embedding(message) if message == "unsupported operation: batch embeddings"),
            "{error:?}"
        );
    }
}
