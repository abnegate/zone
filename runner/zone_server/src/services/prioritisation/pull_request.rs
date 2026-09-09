//! A blast-radius signal for a pull request.
//!
//! This is information, never a verdict. A change that reaches the
//! authentication code is not thereby unready, and nothing here produces a
//! `Blocker`: readiness is about whether the evidence for a pull request adds
//! up, and how far the change reaches is a separate question that a reviewer
//! reads alongside it. A wide blast radius asks for a closer look; it does not
//! withhold a merge.

use serde::Serialize;
use std::fmt;

use super::blast_radius::{BlastRadius, classify, drivers};
use super::change::{Change, Origin};
use super::patterns::PathPatterns;
use super::prioritiser::Prioritiser;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RiskSignal {
    pub blast_radius: BlastRadius,
    pub touched: usize,
    pub drivers: Vec<String>,
}

impl RiskSignal {
    /// True when nothing was observed, so the tier is the conservative default
    /// rather than a reading of the diff.
    pub fn presumed(&self) -> bool {
        self.touched == 0
    }
}

impl fmt::Display for RiskSignal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.presumed() {
            return write!(
                formatter,
                "Blast radius {} (assumed: no files were observed)",
                self.blast_radius.as_str()
            );
        }
        write!(
            formatter,
            "Blast radius {} across {} file{}",
            self.blast_radius.as_str(),
            self.touched,
            if self.touched == 1 { "" } else { "s" }
        )?;
        if self.drivers.is_empty() {
            return Ok(());
        }
        write!(formatter, " ({})", self.drivers.join(", "))
    }
}

pub fn risk(prioritiser: &Prioritiser, paths: &[String]) -> RiskSignal {
    risk_with(prioritiser.patterns(), paths)
}

pub fn risk_with(patterns: &PathPatterns, paths: &[String]) -> RiskSignal {
    let change = Change {
        paths: paths.to_vec(),
        ..Change::new(String::new(), Origin::PullRequest)
    };
    let blast_radius = classify(&change, patterns);
    RiskSignal {
        blast_radius,
        touched: change.surface().count(),
        drivers: drivers(&change, patterns, blast_radius)
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{RiskSignal, risk, risk_with};
    use crate::services::prioritisation::blast_radius::BlastRadius;
    use crate::services::prioritisation::configuration::Configuration;
    use crate::services::prioritisation::patterns::PathPatterns;
    use crate::services::prioritisation::prioritiser::Prioritiser;

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn a_wide_change_reports_its_tier_and_the_paths_behind_it() {
        let signal = risk_with(
            &PathPatterns::default(),
            &paths(&["src/auth/token.rs", "README.md", "src/api/routes.rs"]),
        );

        assert_eq!(signal.blast_radius, BlastRadius::Critical);
        assert_eq!(signal.touched, 3);
        assert_eq!(signal.drivers, vec!["src/auth/token.rs"]);
        assert!(!signal.presumed());
    }

    #[test]
    fn an_unobserved_diff_is_marked_as_assumed() {
        let signal = risk_with(&PathPatterns::default(), &[]);

        assert_eq!(
            signal.blast_radius,
            BlastRadius::Core,
            "no observation must not read as a small change"
        );
        assert!(signal.presumed());
        assert_eq!(
            signal.to_string(),
            "Blast radius core (assumed: no files were observed)"
        );
    }

    #[test]
    fn the_summary_reads_as_information() {
        let signal = risk_with(
            &PathPatterns::default(),
            &paths(&["deploy/helm/values.yaml"]),
        );
        assert_eq!(
            signal.to_string(),
            "Blast radius infrastructure across 1 file (deploy/helm/values.yaml)"
        );
    }

    #[test]
    fn the_prioritiser_carries_the_configured_vocabulary() {
        let configuration = Configuration {
            patterns: PathPatterns {
                critical: vec!["ledger".into()],
                ..PathPatterns::empty()
            },
            ..Configuration::default()
        };
        let prioritiser = Prioritiser::new(configuration);

        assert_eq!(
            risk(&prioritiser, &paths(&["app/ledger/post.kt"])).blast_radius,
            BlastRadius::Critical
        );
        assert_eq!(
            risk(&prioritiser, &paths(&["src/auth/login.rs"])).blast_radius,
            BlastRadius::Peripheral
        );
    }

    #[test]
    fn the_signal_serialises_for_a_readiness_report() {
        let signal = RiskSignal {
            blast_radius: BlastRadius::Test,
            touched: 2,
            drivers: vec!["tests/queue.rs".into()],
        };
        let encoded = serde_json::to_value(&signal).expect("risk signal serialises");
        assert_eq!(encoded["blast_radius"], "test");
        assert_eq!(encoded["touched"], 2);
    }
}
