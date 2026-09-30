//! The request shape each kind of OpenAI-compatible endpoint accepts.
//!
//! LiteLLM absorbs every provider's quirks, so a request sent through it can
//! carry whatever zone asks for. A provider's own API refuses what it does
//! not support with a 400, so a request sent there directly is shaped first.

/// Tokens that end an assistant turn across common chat templates.
pub const TEMPLATE_STOPS: &[&str] = &[
    "<|im_end|>",
    "<|im_start|>",
    "<|eot_id|>",
    "<|end_of_turn|>",
    "<|endoftext|>",
    "<|end_of_text|>",
];

const OPENAI_STOP_LIMIT: usize = 4;
const OPENAI_REASONING_PREFIXES: [&str; 4] = ["o1", "o3", "o4", "gpt-5"];
const ANTHROPIC_TEMPERATURE_RANGE: (f32, f32) = (0.0, 1.0);

/// Which API sits behind [`super::LlmConfig::base_url`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dialect {
    /// LiteLLM, Ollama, vLLM or any other server that takes the request as is.
    #[default]
    Compatible,
    /// OpenAI's own API.
    OpenAI,
    /// Anthropic's OpenAI SDK compatibility layer.
    Anthropic,
}

/// The field a request's output budget travels in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    MaxTokens,
    MaxCompletionTokens,
}

impl Dialect {
    /// Whether `model` is an OpenAI reasoning model, which refuses `stop`, a
    /// non-default `temperature` and `max_tokens`.
    pub fn reasons(self, model: &str) -> bool {
        if self != Self::OpenAI {
            return false;
        }
        let name = model
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        OPENAI_REASONING_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
    }

    /// The stops `model` accepts out of `stops`. OpenAI takes at most four,
    /// so the template tokens its own models never emit go first; the
    /// caller's output filter still halts on them.
    pub fn stops(self, model: &str, stops: &[String]) -> Vec<String> {
        match self {
            Self::Compatible => stops.to_vec(),
            Self::OpenAI if self.reasons(model) => Vec::new(),
            Self::OpenAI => stops
                .iter()
                .filter(|stop| !stop.is_empty() && !TEMPLATE_STOPS.contains(&stop.as_str()))
                .take(OPENAI_STOP_LIMIT)
                .cloned()
                .collect(),
            Self::Anthropic => stops
                .iter()
                .filter(|stop| !stop.trim().is_empty())
                .cloned()
                .collect(),
        }
    }

    /// The temperature `model` accepts, or `None` when it takes only its own.
    pub fn temperature(self, model: &str, temperature: f32) -> Option<f32> {
        match self {
            Self::Compatible => Some(temperature),
            Self::OpenAI if self.reasons(model) => None,
            Self::OpenAI => Some(temperature),
            Self::Anthropic => {
                let (lowest, highest) = ANTHROPIC_TEMPERATURE_RANGE;
                Some(temperature.clamp(lowest, highest))
            }
        }
    }

    pub fn budget(self, model: &str) -> Budget {
        if self.reasons(model) {
            Budget::MaxCompletionTokens
        } else {
            Budget::MaxTokens
        }
    }

    /// Whether `reasoning_effort` may be sent for `model`. OpenAI refuses it
    /// on a model that does not reason, and Anthropic ignores it.
    pub fn effort(self, model: &str) -> bool {
        match self {
            Self::Compatible => true,
            Self::OpenAI => self.reasons(model),
            Self::Anthropic => false,
        }
    }

    /// Whether fields no provider API defines, such as Ollama's `num_ctx`,
    /// may be sent.
    pub fn extended(self) -> bool {
        self == Self::Compatible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn openai_reasoning_models_are_recognised_by_prefix() {
        for model in [
            "o1",
            "o1-mini",
            "o3-mini",
            "o4-mini",
            "gpt-5",
            "gpt-5-mini",
            "openai/o3",
            "O3",
        ] {
            assert!(Dialect::OpenAI.reasons(model), "{model}");
        }
        for model in ["gpt-4o", "gpt-4.1-mini", "chatgpt-4o-latest"] {
            assert!(!Dialect::OpenAI.reasons(model), "{model}");
        }
        assert!(!Dialect::Compatible.reasons("o3"));
        assert!(!Dialect::Anthropic.reasons("o3"));
    }

    #[test]
    fn openai_takes_at_most_four_stops_and_none_of_the_template_tokens() {
        let mut stops = strings(TEMPLATE_STOPS);
        stops.extend(strings(&["User:", "", "Human:", "###", "END", "STOP"]));

        let shaped = Dialect::OpenAI.stops("gpt-4o", &stops);

        assert_eq!(shaped, strings(&["User:", "Human:", "###", "END"]));
    }

    #[test]
    fn anthropic_keeps_every_stop_but_whitespace() {
        let stops = strings(&["<|im_end|>", " ", "\n", "User:"]);

        assert_eq!(
            Dialect::Anthropic.stops("claude-sonnet-4-5", &stops),
            strings(&["<|im_end|>", "User:"])
        );
    }

    #[test]
    fn anthropic_temperature_is_held_between_zero_and_one() {
        assert_eq!(Dialect::Anthropic.temperature("claude", 1.4), Some(1.0));
        assert_eq!(Dialect::Anthropic.temperature("claude", -0.5), Some(0.0));
        assert_eq!(Dialect::Anthropic.temperature("claude", 0.3), Some(0.3));
    }

    #[test]
    fn a_compatible_endpoint_is_sent_the_request_unchanged() {
        let stops = strings(TEMPLATE_STOPS);

        assert_eq!(Dialect::Compatible.stops("o3", &stops), stops);
        assert_eq!(Dialect::Compatible.temperature("o3", 1.7), Some(1.7));
        assert_eq!(Dialect::Compatible.budget("o3"), Budget::MaxTokens);
        assert!(Dialect::Compatible.effort("anything"));
        assert!(Dialect::Compatible.extended());
    }
}
