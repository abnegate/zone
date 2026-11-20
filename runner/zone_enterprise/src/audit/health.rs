use std::fmt;

use serde::{Deserialize, Serialize};

use super::standing::Pause;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Healthy,
    Failing,
    Paused,
    Disabled,
}

impl Health {
    pub const ALL: [Self; 4] = [Self::Healthy, Self::Failing, Self::Paused, Self::Disabled];

    pub fn of(enabled: bool, pause: Option<Pause>, attempts: u32) -> Self {
        if !enabled {
            return Self::Disabled;
        }
        if pause.is_some() {
            return Self::Paused;
        }
        if attempts > 0 {
            return Self::Failing;
        }
        Self::Healthy
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Failing => "failing",
            Self::Paused => "paused",
            Self::Disabled => "disabled",
        }
    }
}

impl fmt::Display for Health {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_wins_over_everything() {
        for pause in [None, Some(Pause::Lapsed)] {
            for attempts in [0, 3] {
                assert_eq!(Health::of(false, pause, attempts), Health::Disabled);
            }
        }
    }

    #[test]
    fn a_pause_wins_over_failing() {
        for pause in Pause::ALL {
            assert_eq!(Health::of(true, Some(pause), 0), Health::Paused);
            assert_eq!(Health::of(true, Some(pause), 4), Health::Paused);
        }
    }

    #[test]
    fn failing_while_attempts_are_outstanding() {
        assert_eq!(Health::of(true, None, 1), Health::Failing);
        assert_eq!(Health::of(true, None, u32::MAX), Health::Failing);
    }

    #[test]
    fn healthy_when_enabled_unpaused_and_without_attempts() {
        assert_eq!(Health::of(true, None, 0), Health::Healthy);
    }

    #[test]
    fn health_is_the_wire_contract() {
        let values: Vec<&str> = Health::ALL.iter().map(|health| health.as_str()).collect();
        assert_eq!(values, ["healthy", "failing", "paused", "disabled"]);
        for health in Health::ALL {
            assert_eq!(serde_json::to_value(health).unwrap(), health.as_str());
            assert_eq!(health.to_string(), health.as_str());
        }
    }
}
