//! What is measured about a unit of work, independent of where it came from.
//!
//! A queued task and a pull request expose different raw fields but the same
//! five questions: how urgent it was declared, how far it reaches, how likely
//! it is to be a regression, what it touches, and whether it belongs to a group
//! of related work. Only the first three live here; the blast radius is derived
//! from paths and cluster membership is decided by the caller.

use serde::Serialize;

use super::ratio::Ratio;

/// The declared urgency of a change.
///
/// `from_rank` reads the `1..=5` integer the `tasks` and `task_queue` tables
/// store, where a larger number is more urgent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    None,
    Low,
    Medium,
    High,
    Critical,
}

const RANKS: [Severity; 5] = [
    Severity::None,
    Severity::Low,
    Severity::Medium,
    Severity::High,
    Severity::Critical,
];

const LOWEST_RANK: i32 = 1;

impl Severity {
    pub fn from_rank(rank: i32) -> Self {
        let highest = LOWEST_RANK + RANKS.len() as i32 - 1;
        let index = rank.clamp(LOWEST_RANK, highest) - LOWEST_RANK;
        RANKS[index as usize]
    }

    pub fn weight(self) -> Ratio {
        Ratio::of(self as u64, RANKS.len() as u64 - 1)
    }
}

/// How loudly a failure announced itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Unspecified,
    Warning,
    Error,
    Fatal,
}

const WARNING_WEIGHT: f64 = 0.4;
const ERROR_WEIGHT: f64 = 0.7;

impl Level {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "fatal" | "critical" => Self::Fatal,
            "error" => Self::Error,
            "warning" | "warn" => Self::Warning,
            _ => Self::Unspecified,
        }
    }

    pub fn weight(self) -> Ratio {
        match self {
            Self::Unspecified => Ratio::ZERO,
            Self::Warning => Ratio::new(WARNING_WEIGHT),
            Self::Error => Ratio::new(ERROR_WEIGHT),
            Self::Fatal => Ratio::ONE,
        }
    }
}

/// The measurements behind a score.
///
/// `occurrences` counts how often the work has been seen or retried,
/// `dependents` how much other work waits on it, and `escalation` the share of
/// attempts that ended badly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signals {
    pub severity: Severity,
    pub occurrences: u64,
    pub dependents: u64,
    pub escalation: Ratio,
    pub unhandled: bool,
    pub level: Level,
    pub clustered: bool,
}

#[cfg(test)]
mod tests {
    use super::{Level, Ratio, Severity};

    #[test]
    fn rank_maps_onto_the_stored_range() {
        assert_eq!(Severity::from_rank(1), Severity::None);
        assert_eq!(Severity::from_rank(3), Severity::Medium);
        assert_eq!(Severity::from_rank(5), Severity::Critical);
    }

    #[test]
    fn rank_outside_the_stored_range_is_clamped() {
        assert_eq!(Severity::from_rank(-9), Severity::None);
        assert_eq!(Severity::from_rank(9), Severity::Critical);
    }

    #[test]
    fn severity_weights_span_the_unit_interval() {
        assert_eq!(Severity::None.weight(), Ratio::ZERO);
        assert_eq!(Severity::Medium.weight(), Ratio::new(0.5));
        assert_eq!(Severity::Critical.weight(), Ratio::ONE);
    }

    #[test]
    fn level_parses_case_insensitively_with_aliases() {
        assert_eq!(Level::parse("FATAL"), Level::Fatal);
        assert_eq!(Level::parse(" warn "), Level::Warning);
        assert_eq!(Level::parse("info"), Level::Unspecified);
    }

    #[test]
    fn level_weights_increase_with_loudness() {
        assert!(Level::Unspecified.weight() < Level::Warning.weight());
        assert!(Level::Warning.weight() < Level::Error.weight());
        assert!(Level::Error.weight() < Level::Fatal.weight());
    }
}
