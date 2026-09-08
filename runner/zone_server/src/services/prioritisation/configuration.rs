//! Everything a workspace can tune, in one typed object.

use super::patterns::PathPatterns;
use super::suppression::Rule;
use super::weights::Weights;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Configuration {
    pub patterns: PathPatterns,
    pub weights: Weights,
    pub suppression: Vec<Rule>,
}

#[cfg(test)]
mod tests {
    use super::Configuration;
    use crate::services::prioritisation::patterns::PathPatterns;
    use crate::services::prioritisation::weights::Weights;

    #[test]
    fn defaults_compose_the_default_of_each_part() {
        let configuration = Configuration::default();
        assert_eq!(configuration.patterns, PathPatterns::default());
        assert_eq!(configuration.weights, Weights::default());
        assert!(configuration.suppression.is_empty());
    }
}
