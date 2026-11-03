//! How hard a thinking-capable model should think for this request.

use abnegate_llm::Effort;
use abnegate_llm::reasoning::classify;
use serde::{Deserialize, Serialize};

/// User-facing effort. Auto inspects the request; Off skips thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    #[default]
    Auto,
    Off,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
    Ultra,
}

impl ReasoningEffort {
    pub const ALL: [Self; 8] = [
        Self::Auto,
        Self::Off,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
        Self::Ultra,
    ];

    /// The levels a completions API is offered. Anything above `High` reaches
    /// it as `High`, the most it takes.
    pub const HTTP: [Self; 4] = [Self::Off, Self::Low, Self::Medium, Self::High];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|effort| effort.as_str() == value)
    }

    /// The effort a completions API is sent, if thinking should run.
    pub fn resolve(self, prompt: &str) -> Option<Effort> {
        match self {
            Self::Off => None,
            Self::Low => Some(Effort::Low),
            Self::Medium => Some(Effort::Medium),
            Self::High | Self::XHigh | Self::Max | Self::Ultra => Some(Effort::High),
            Self::Auto => Some(classify(prompt)),
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_llm::Effort;

    use super::ReasoningEffort;

    #[test]
    fn auto_picks_effort_from_the_request() {
        assert_eq!(ReasoningEffort::Auto.resolve("Hello."), Some(Effort::Low));
        assert_eq!(
            ReasoningEffort::Auto.resolve("What is the capital of France?"),
            Some(Effort::Medium)
        );
        assert_eq!(
            ReasoningEffort::Auto.resolve("Debug the deadlock in the worker pool step by step."),
            Some(Effort::High)
        );
        assert_eq!(ReasoningEffort::Off.resolve("Debug this."), None);
        assert_eq!(ReasoningEffort::High.resolve("Hi"), Some(Effort::High));
    }

    #[test]
    fn parse_round_trips_known_values() {
        for value in [
            "auto", "off", "low", "medium", "high", "xhigh", "max", "ultra",
        ] {
            let effort = ReasoningEffort::parse(value).unwrap();
            assert_eq!(effort.as_str(), value);
        }
        for value in ["extreme", "x_high", "XHIGH", " max", ""] {
            assert_eq!(ReasoningEffort::parse(value), None, "{value:?}");
        }
    }

    #[test]
    fn serde_spells_every_level_as_parse_reads_it() {
        for effort in ReasoningEffort::ALL {
            let spelled = serde_json::to_value(effort).unwrap();
            assert_eq!(spelled, serde_json::json!(effort.as_str()));
            assert_eq!(
                serde_json::from_value::<ReasoningEffort>(spelled).unwrap(),
                effort
            );
        }
    }

    #[test]
    fn levels_above_high_reach_a_completions_api_as_high() {
        for effort in [
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
            ReasoningEffort::Ultra,
        ] {
            assert_eq!(effort.resolve("Hi"), Some(Effort::High), "{effort:?}");
        }
    }

    #[test]
    fn a_completions_api_is_offered_off_through_high() {
        assert_eq!(
            ReasoningEffort::HTTP,
            [
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ]
        );
    }
}
