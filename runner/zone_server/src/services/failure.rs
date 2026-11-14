//! What a reader is told when a turn failed.
//!
//! A gateway that reached nothing at all answers with its own traceback: the
//! connection it could not make, the fallbacks it then tried, and how many
//! times it retried. None of that is a reader's problem, and the one fact in
//! it — the service behind the gateway is down — is the hardest part to find.

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
