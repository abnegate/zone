//! How much each signal contributes to a total, as configuration.
//!
//! The defaults sum to one so a total stays inside the unit interval and two
//! scores from different workspaces remain comparable. Nothing enforces that:
//! a deployment that cares only about blast radius is free to say so.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub severity: f64,
    pub reach: f64,
    pub regression: f64,
    pub blast_radius: f64,
    pub cluster: f64,
}

const DEFAULT_SEVERITY: f64 = 0.35;
const DEFAULT_REACH: f64 = 0.2;
const DEFAULT_REGRESSION: f64 = 0.2;
const DEFAULT_BLAST_RADIUS: f64 = 0.2;
const DEFAULT_CLUSTER: f64 = 0.05;

impl Default for Weights {
    fn default() -> Self {
        Self {
            severity: DEFAULT_SEVERITY,
            reach: DEFAULT_REACH,
            regression: DEFAULT_REGRESSION,
            blast_radius: DEFAULT_BLAST_RADIUS,
            cluster: DEFAULT_CLUSTER,
        }
    }
}

impl Weights {
    pub const NONE: Self = Self {
        severity: 0.0,
        reach: 0.0,
        regression: 0.0,
        blast_radius: 0.0,
        cluster: 0.0,
    };

    pub fn total(&self) -> f64 {
        self.severity + self.reach + self.regression + self.blast_radius + self.cluster
    }
}

#[cfg(test)]
mod tests {
    use super::Weights;

    #[test]
    fn defaults_sum_to_one() {
        assert!(
            (Weights::default().total() - 1.0).abs() < f64::EPSILON * 8.0,
            "default weights sum to {}, expected 1.0",
            Weights::default().total()
        );
    }

    #[test]
    fn none_contributes_nothing() {
        assert_eq!(Weights::NONE.total(), 0.0);
    }
}
