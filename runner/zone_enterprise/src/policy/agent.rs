use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|agent| agent.as_str() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        for (agent, value) in [(Agent::Claude, r#""claude""#), (Agent::Codex, r#""codex""#)] {
            assert_eq!(serde_json::to_string(&agent).unwrap(), value);
            assert_eq!(serde_json::to_value(agent).unwrap(), agent.as_str());
        }
    }

    #[test]
    fn refuses_unknown_values() {
        for value in [r#""Claude""#, r#""claude_code""#, r#""gemini""#] {
            assert!(serde_json::from_str::<Agent>(value).is_err(), "{value}");
        }
    }

    #[test]
    fn names_each_agent_by_its_wire_value() {
        assert_eq!(Agent::named("claude"), Some(Agent::Claude));
        assert_eq!(Agent::named("codex"), Some(Agent::Codex));
        assert_eq!(Agent::named("Claude"), None);
        assert_eq!(Agent::named("gemini"), None);
    }

    #[test]
    fn labels_each_agent() {
        assert_eq!(Agent::Claude.label(), "Claude Code");
        assert_eq!(Agent::Codex.label(), "Codex");
    }
}
