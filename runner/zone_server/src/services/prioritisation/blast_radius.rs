//! How far a change reaches, judged from the paths and symbols it names.
//!
//! The tiers are declared in impact order so the derived `Ord` sorts them the
//! way a reader expects. Classification does not use that order: a change that
//! touches both a migration and a README is infrastructure, but a change that
//! touches both a README and a service module is documentation, so the least
//! impactful tiers are consulted before the middle ones. `PRECEDENCE` is that
//! rule, written down once.

use serde::Serialize;

use super::change::Change;
use super::patterns::PathPatterns;
use super::ratio::Ratio;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlastRadius {
    Cosmetic,
    Test,
    Peripheral,
    #[default]
    Core,
    Infrastructure,
    Critical,
}

const PRECEDENCE: [BlastRadius; 5] = [
    BlastRadius::Critical,
    BlastRadius::Infrastructure,
    BlastRadius::Cosmetic,
    BlastRadius::Test,
    BlastRadius::Core,
];

const UNMATCHED: BlastRadius = BlastRadius::Peripheral;

const INFRASTRUCTURE_WEIGHT: f64 = 0.8;
const CORE_WEIGHT: f64 = 0.6;
const PERIPHERAL_WEIGHT: f64 = 0.4;
const TEST_WEIGHT: f64 = 0.2;
const COSMETIC_WEIGHT: f64 = 0.1;

impl BlastRadius {
    pub fn weight(self) -> Ratio {
        match self {
            Self::Critical => Ratio::ONE,
            Self::Infrastructure => Ratio::new(INFRASTRUCTURE_WEIGHT),
            Self::Core => Ratio::new(CORE_WEIGHT),
            Self::Peripheral => Ratio::new(PERIPHERAL_WEIGHT),
            Self::Test => Ratio::new(TEST_WEIGHT),
            Self::Cosmetic => Ratio::new(COSMETIC_WEIGHT),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Infrastructure => "infrastructure",
            Self::Core => "core",
            Self::Peripheral => "peripheral",
            Self::Test => "test",
            Self::Cosmetic => "cosmetic",
        }
    }
}

/// Classify one path or symbol on its own.
pub fn classify_signal(signal: &str, patterns: &PathPatterns) -> BlastRadius {
    PRECEDENCE
        .into_iter()
        .find(|radius| patterns.matches(*radius, signal))
        .unwrap_or(UNMATCHED)
}

/// Classify a change from everything it names.
///
/// A change that names nothing is `Core`, not `Peripheral`: absent metadata is
/// not evidence of a small change, so the default is the conservative one.
pub fn classify(change: &Change, patterns: &PathPatterns) -> BlastRadius {
    let surface: Vec<&str> = change.surface().collect();
    if surface.is_empty() {
        return BlastRadius::default();
    }
    PRECEDENCE
        .into_iter()
        .find(|radius| {
            surface
                .iter()
                .any(|signal| patterns.matches(*radius, signal))
        })
        .unwrap_or(UNMATCHED)
}

/// The entries of `surface` that put the change in `radius`.
pub fn drivers<'a>(
    change: &'a Change,
    patterns: &PathPatterns,
    radius: BlastRadius,
) -> Vec<&'a str> {
    change
        .surface()
        .filter(|signal| classify_signal(signal, patterns) == radius)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{BlastRadius, classify, classify_signal, drivers};
    use crate::services::prioritisation::change::{Change, Origin};
    use crate::services::prioritisation::patterns::PathPatterns;

    fn touching(paths: &[&str]) -> Change {
        Change {
            paths: paths.iter().map(|path| (*path).to_string()).collect(),
            ..Change::new("change-1", Origin::PullRequest)
        }
    }

    fn patterns() -> PathPatterns {
        PathPatterns::default()
    }

    #[test]
    fn critical_tier_from_an_authentication_path() {
        assert_eq!(
            classify(&touching(&["src/auth/login.rs"]), &patterns()),
            BlastRadius::Critical
        );
    }

    #[test]
    fn infrastructure_tier_from_a_deployment_path() {
        assert_eq!(
            classify(&touching(&["deploy/docker-compose.yml"]), &patterns()),
            BlastRadius::Infrastructure
        );
    }

    #[test]
    fn cosmetic_tier_from_documentation() {
        assert_eq!(
            classify(&touching(&["README.md"]), &patterns()),
            BlastRadius::Cosmetic
        );
    }

    #[test]
    fn test_tier_from_a_test_path() {
        assert_eq!(
            classify(&touching(&["zone_server/tests/queue.rs"]), &patterns()),
            BlastRadius::Test
        );
    }

    #[test]
    fn core_tier_from_a_service_path() {
        assert_eq!(
            classify(&touching(&["src/api/routes.rs"]), &patterns()),
            BlastRadius::Core
        );
    }

    #[test]
    fn peripheral_tier_when_nothing_matches() {
        assert_eq!(
            classify(&touching(&["src/formatting/pretty.rs"]), &patterns()),
            BlastRadius::Peripheral
        );
    }

    #[test]
    fn no_metadata_defaults_to_core() {
        let change = Change::new("change-1", Origin::Task);
        assert_eq!(
            classify(&change, &patterns()),
            BlastRadius::Core,
            "a change that names nothing must take the conservative default"
        );
    }

    #[test]
    fn symbols_classify_when_no_path_is_known() {
        let change = Change {
            symbols: vec!["billing_service.charge".into()],
            ..Change::new("change-1", Origin::Task)
        };
        assert_eq!(classify(&change, &patterns()), BlastRadius::Critical);
    }

    #[test]
    fn critical_wins_over_a_test_path() {
        assert_eq!(
            classify(&touching(&["tests/auth/session.rs"]), &patterns()),
            BlastRadius::Critical
        );
    }

    #[test]
    fn documentation_wins_over_a_core_path() {
        assert_eq!(
            classify(&touching(&["src/api/README.md"]), &patterns()),
            BlastRadius::Cosmetic,
            "a doc file inside a core directory is still a doc change"
        );
    }

    #[test]
    fn segment_matching_rejects_a_substring_hit() {
        assert_ne!(
            classify(&touching(&["src/scoreboard/render.rs"]), &patterns()),
            BlastRadius::Core,
            "\"scoreboard\" must not match the core pattern \"core\""
        );
        assert_ne!(
            classify(&touching(&["src/social/feed.rs"]), &patterns()),
            BlastRadius::Infrastructure,
            "\"social\" must not match the infrastructure pattern \"ci\""
        );
    }

    #[test]
    fn weights_descend_with_impact() {
        assert_eq!(BlastRadius::Critical.weight().value(), 1.0);
        assert!(BlastRadius::Critical.weight() > BlastRadius::Infrastructure.weight());
        assert!(BlastRadius::Infrastructure.weight() > BlastRadius::Core.weight());
        assert!(BlastRadius::Core.weight() > BlastRadius::Peripheral.weight());
        assert!(BlastRadius::Peripheral.weight() > BlastRadius::Test.weight());
        assert!(BlastRadius::Test.weight() > BlastRadius::Cosmetic.weight());
    }

    #[test]
    fn tiers_order_by_impact() {
        let mut tiers = vec![
            BlastRadius::Core,
            BlastRadius::Cosmetic,
            BlastRadius::Critical,
            BlastRadius::Test,
        ];
        tiers.sort();
        assert_eq!(
            tiers,
            vec![
                BlastRadius::Cosmetic,
                BlastRadius::Test,
                BlastRadius::Core,
                BlastRadius::Critical,
            ]
        );
    }

    #[test]
    fn classify_signal_matches_the_whole_change_for_one_path() {
        assert_eq!(
            classify_signal("infra/terraform/main.tf", &patterns()),
            BlastRadius::Infrastructure
        );
    }

    #[test]
    fn drivers_name_the_paths_behind_the_tier() {
        let change = touching(&["src/auth/token.rs", "README.md", "src/api/routes.rs"]);
        assert_eq!(
            drivers(&change, &patterns(), BlastRadius::Critical),
            vec!["src/auth/token.rs"]
        );
    }
}
