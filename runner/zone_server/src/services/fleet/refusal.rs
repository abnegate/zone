use zone_enterprise::fleet::Refusal;

use crate::db::fleet::shortfall::Shortfall;

/// The refusal a runner gives for work it falls short of.
pub fn of(shortfall: Shortfall) -> Refusal {
    match shortfall {
        Shortfall::Unregistered => Refusal::Unregistered,
        Shortfall::Cordoned => Refusal::Cordoned,
        Shortfall::Revoked => Refusal::Revoked,
        Shortfall::Outdated { version, minimum } => Refusal::Outdated { version, minimum },
        Shortfall::LabelsMissing { missing } => Refusal::LabelsMissing { missing },
    }
}

#[cfg(test)]
mod tests {
    use zone_enterprise::fleet::{
        LABELS_MISSING, Labels, RUNNER_CORDONED, RUNNER_OUTDATED, RUNNER_REVOKED,
        RUNNER_UNREGISTERED, Version,
    };

    use super::*;

    fn every() -> [Shortfall; 5] {
        [
            Shortfall::Unregistered,
            Shortfall::Cordoned,
            Shortfall::Revoked,
            Shortfall::Outdated {
                version: Version::parse("1.3.0").unwrap(),
                minimum: Version::parse("1.4.0").unwrap(),
            },
            Shortfall::LabelsMissing {
                missing: Labels::parse_list("gpu").unwrap(),
            },
        ]
    }

    fn code(shortfall: &Shortfall) -> &'static str {
        match shortfall {
            Shortfall::Unregistered => RUNNER_UNREGISTERED,
            Shortfall::Cordoned => RUNNER_CORDONED,
            Shortfall::Revoked => RUNNER_REVOKED,
            Shortfall::Outdated { .. } => RUNNER_OUTDATED,
            Shortfall::LabelsMissing { .. } => LABELS_MISSING,
        }
    }

    #[test]
    fn every_shortfall_maps_to_the_refusal_of_its_code() {
        for shortfall in every() {
            assert_eq!(
                of(shortfall.clone()).code(),
                code(&shortfall),
                "{shortfall:?}"
            );
        }
    }

    #[test]
    fn a_refusal_keeps_what_the_runner_lacks() {
        let [.., outdated, missing] = every();
        assert_eq!(
            of(outdated),
            Refusal::Outdated {
                version: Version::parse("1.3.0").unwrap(),
                minimum: Version::parse("1.4.0").unwrap(),
            }
        );
        assert_eq!(
            of(missing),
            Refusal::LabelsMissing {
                missing: Labels::parse_list("gpu").unwrap(),
            }
        );
    }
}
