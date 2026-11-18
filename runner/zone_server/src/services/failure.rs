//! What a reader is told when a turn failed, and whether running it again
//! could end any better.
//!
//! A gateway that reached nothing at all answers with its own traceback: the
//! connection it could not make, the fallbacks it then tried, and how many
//! times it retried. None of that is a reader's problem, and the one fact in
//! it — the service behind the gateway is down — is the hardest part to find.
//!
//! One classification serves both the runs a task worker retries and the
//! chat turns that resume themselves, so a failure means the same thing
//! wherever it is read.

use abnegate_agent_cli::CREDITS_REQUIRED;
use abnegate_agent_cli::LONG_CONTEXT_CREDITS_REQUIRED;
use abnegate_agent_cli::PROSE_EXCEEDED;
use abnegate_http::RetryAfter;
use abnegate_http::contains_standalone_status;
use abnegate_llm::Limit;
use reqwest::StatusCode;
use std::time::Duration;

/// Words a gateway uses when the connection was never made. A refused or
/// unanswered connection says the service behind the gateway is down,
/// whatever else the traceback carries.
const UNREACHED: &[&str] = &[
    "all connection attempts failed",
    "cannot connect to host",
    "connect call failed",
    "connection refused",
    "failed to establish a new connection",
];

/// What a service with no name is called.
const UNNAMED: &str = "The model host";

/// The longest service name and host a traceback is believed about, so a
/// malformed one cannot carry a paragraph into the reader's message.
const LONGEST: usize = 120;

/// `message` rewritten when it is only a gateway reporting that it could not
/// reach the model host, and [`None`] when it says anything else. The
/// rewrite replaces the traceback rather than adding to it: a connection
/// that was never made leaves nothing else worth reading.
pub fn unreachable(message: &str) -> Option<String> {
    let lowered = message.to_ascii_lowercase();
    if !UNREACHED.iter().any(|words| lowered.contains(words)) {
        return None;
    }
    let service = named(message, &lowered).unwrap_or_else(|| UNNAMED.to_string());
    Some(match host(message, &lowered) {
        Some(host) => format!("{service} is down: nothing answered at {host}."),
        None => format!("{service} is down: the endpoint could not connect to it."),
    })
}

/// The service a traceback names, as `Ollama` in litellm's
/// `Ollama_chatException`. The suffix after an underscore is the call it
/// failed on rather than the service, so it is dropped.
fn named(message: &str, lowered: &str) -> Option<String> {
    let end = lowered.find("exception")?;
    let start = message[..end]
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || *character == '_')
        .last()
        .map(|(index, _)| index)?;
    let name = message[start..end].split('_').next()?;
    (!name.is_empty() && name.len() <= LONGEST).then(|| name.to_string())
}

/// The host a traceback could not reach, with its port when it names one.
fn host(message: &str, lowered: &str) -> Option<String> {
    const LABEL: &str = "cannot connect to host ";
    let start = lowered.find(LABEL)? + LABEL.len();
    let host = message
        .get(start..)?
        .split_whitespace()
        .next()?
        .trim_end_matches([',', '.', ';', '\'', '"']);
    (!host.is_empty() && host.len() <= LONGEST).then(|| host.to_string())
}

/// How a failed attempt may be recovered.
///
/// A terminal failure is one a byte-identical retry cannot survive: bad
/// credentials, a malformed or oversized request, a missing model, a refusal,
/// or a run that already burned its whole budget. Retrying those spends the
/// budget again and can duplicate the side effects the attempt already had.
///
/// A usage limit that another of the organization's logins can run past is
/// rerouted: the run moves to that login at once, at no cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    Terminal,
    Transient,
    RateLimited { retry_after: Option<Duration> },
    Rerouted,
}

impl Failure {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Transient => "transient",
            Self::RateLimited { .. } => "rate_limited",
            Self::Rerouted => "rerouted",
        }
    }

    pub(crate) fn retry_after(self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => retry_after,
            Self::Terminal | Self::Transient | Self::Rerouted => None,
        }
    }
}

const RATE_LIMIT_MARKERS: &[&str] = &[
    "quota exceeded",
    "rate limit",
    "rate_limit",
    "ratelimit",
    "resource exhausted",
    "retry-after",
    "retry_after",
    "too many requests",
];

/// How abnegate-agent-cli begins a coding agent's failure that a retry of the
/// same prompt meets again: prose past the output limit, and a model or a
/// context the signed-in account cannot spend usage credits on.
pub(crate) const AGENT_TERMINAL_MARKERS: &[&str] = &[
    PROSE_EXCEEDED,
    CREDITS_REQUIRED,
    LONG_CONTEXT_CREDITS_REQUIRED,
];

const TERMINAL_MARKERS: &[&str] = &[
    "access denied",
    "authentication",
    "bad request",
    "budget exhausted",
    "canceled",
    "cancelled",
    "content policy",
    "content_filter",
    "content_policy",
    "context length",
    "context window",
    "invalid api key",
    "invalid model",
    "invalid request",
    "invalid_api_key",
    "invalid_request_error",
    "malformed",
    "maximum context",
    "model not found",
    "model_not_found",
    "no such model",
    "not supported",
    "permission denied",
    "unauthorized",
    "unsupported",
];

const TERMINAL_STATUSES: &[StatusCode] = &[
    StatusCode::BAD_REQUEST,
    StatusCode::UNAUTHORIZED,
    StatusCode::FORBIDDEN,
    StatusCode::NOT_FOUND,
    StatusCode::UNPROCESSABLE_ENTITY,
];

/// Rate limits are matched first: a throttled request is recoverable even when
/// the provider dresses it up in the same vocabulary as a rejected one.
pub(crate) fn classify(message: &str) -> Failure {
    let lowered = message.to_ascii_lowercase();
    if RATE_LIMIT_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
        || contains_standalone_status(&lowered, StatusCode::TOO_MANY_REQUESTS.as_u16())
    {
        return Failure::RateLimited {
            retry_after: hint(&lowered),
        };
    }
    if TERMINAL_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
        || TERMINAL_STATUSES
            .iter()
            .any(|status| contains_standalone_status(&lowered, status.as_u16()))
    {
        return Failure::Terminal;
    }
    Failure::Transient
}

/// The wait `text` asks for. A zero wait is still a rate limit, so it takes
/// the rate-limit curve rather than retrying at once.
fn hint(text: &str) -> Option<Duration> {
    RetryAfter::from_text(text).filter(|delay| !delay.is_zero())
}

/// [`classify`] for a coding agent's own words, where a failure no retry
/// survives is terminal and a subscription that ran out, in any of
/// [`abnegate_llm::provider::LIMIT_WORDINGS`], backs off like any rate
/// limit. Only a coding agent's own words are read for those: an endpoint's
/// failure that happens to use them is judged by [`classify`] alone.
pub(crate) fn classify_agent(words: &str) -> Failure {
    if AGENT_TERMINAL_MARKERS
        .iter()
        .any(|marker| words.contains(marker))
    {
        return Failure::Terminal;
    }
    if Limit::worded(words) {
        return Failure::RateLimited {
            retry_after: hint(words),
        };
    }
    classify(words)
}

/// Whether a turn that failed with `message` is worth going on with as it is.
///
/// A stream that broke on the way from the provider says nothing about the
/// turn: the same request, resumed, usually finishes. A refusal, a bad
/// request and a context that will not fit say everything about it, and
/// meeting them again costs the reader another wait for the same words.
pub(crate) fn resumable(message: &str) -> bool {
    matches!(classify(message), Failure::Transient)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What litellm answers a self-hosted chat with when ollama is not
    /// running: a connection failure, the fallback groups it then looked
    /// through, and its retry count.
    const OLLAMA_DOWN: &str = "Failed to generate response: API error: 500 - {\"error\":{\"message\":\"litellm.APIConnectionError: Ollama_chatException - Cannot connect to host host.docker.internal:11434 ssl:<ssl.SSLContext object at 0xffffadbabb10> [Connect call failed ('192.168.65.254', 11434)]No fallback model group found for original model_group=qwen3:27b. Fallbacks=[{'fast': ['reason', 'qwen3:27b']}] LiteLLM Retried: 2 times, LiteLLM Max Retries: 2\",\"type\":null,\"param\":null,\"code\":\"500\"}}";

    #[test]
    fn a_gateway_that_reached_nothing_reports_the_service_and_where_it_is() {
        let reported = unreachable(OLLAMA_DOWN).expect("a connection that was never made");

        assert_eq!(
            reported,
            "Ollama is down: nothing answered at host.docker.internal:11434."
        );
    }

    #[test]
    fn the_traceback_the_gateway_wrote_does_not_reach_the_reader() {
        let reported = unreachable(OLLAMA_DOWN).expect("a connection that was never made");

        for noise in [
            "litellm",
            "APIConnectionError",
            "ssl.SSLContext",
            "model_group",
            "Fallbacks",
            "Retried",
            "500",
        ] {
            assert!(
                !reported.contains(noise),
                "the gateway's {noise} reached the reader: {reported}"
            );
        }
    }

    #[test]
    fn a_connection_failure_without_a_host_still_names_the_service() {
        let refused = "API error: 500 - litellm.APIConnectionError: OpenAIException - \
             [Errno 111] Connection refused";

        assert_eq!(
            unreachable(refused).as_deref(),
            Some("OpenAI is down: the endpoint could not connect to it.")
        );
    }

    #[test]
    fn a_connection_failure_naming_no_service_says_the_model_host_is_down() {
        let httpx = "API error: 500 - All connection attempts failed";

        assert_eq!(
            unreachable(httpx).as_deref(),
            Some("The model host is down: the endpoint could not connect to it.")
        );
    }

    /// Only a connection that was never made is rewritten. Everything else is
    /// the provider answering, and what it answered is what the reader needs.
    #[test]
    fn a_failure_the_provider_answered_is_left_as_it_is() {
        for answered in [
            "API error (401): Incorrect API key provided",
            "API error (404): The model `gpt-9` does not exist",
            "API error: 429 - slow down",
            "litellm.ContextWindowExceededError: this model's maximum context length is 8192 tokens",
            "the endpoint sent nothing for 600s",
        ] {
            assert_eq!(unreachable(answered), None, "{answered}");
        }
    }
}

#[cfg(test)]
mod resumable_tests {
    use super::*;

    /// What a connection that broke under a running turn says. None of it is about the turn,
    /// so the turn is worth going on with.
    #[test]
    fn a_stream_that_broke_under_a_turn_is_worth_going_on_from() {
        for broken in [
            "Stream error: error decoding response body",
            "Stream error: connection reset by peer",
            "error sending request for url (https://api.anthropic.com/v1/messages)",
            "API error: 502 - Bad Gateway",
            "API error: 529 - Overloaded",
            "the endpoint sent nothing for 600s",
            "Cannot connect to host host.docker.internal:11434",
        ] {
            assert!(resumable(broken), "{broken}");
        }
    }

    /// What the provider says about the request itself. Meeting it again spends the reader's
    /// wait on the same answer.
    #[test]
    fn a_failure_the_request_itself_earned_is_not_resumed() {
        for answered in [
            "API error (401): Incorrect API key provided",
            "invalid_request_error: messages: at least one message is required",
            "litellm.ContextWindowExceededError: this model's maximum context length is 8192 tokens",
            "API error: 429 - rate limit reached",
            "model_not_found: the model `gpt-9` does not exist",
        ] {
            assert!(!resumable(answered), "{answered}");
        }
    }

    /// A rate limit is recoverable, but not by going on at once: it is the one failure a chat
    /// turn hands to its reader rather than sitting on.
    #[test]
    fn a_rate_limit_is_not_resumed_however_recoverable_it_is() {
        assert_eq!(
            classify("API error: 429 - too many requests, retry-after: 30"),
            Failure::RateLimited {
                retry_after: Some(Duration::from_secs(30))
            }
        );
        assert!(!resumable("API error: 429 - too many requests"));
    }
}
