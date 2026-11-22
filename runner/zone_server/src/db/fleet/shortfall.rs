use zone_enterprise::fleet::{Labels, Version};

use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::mode::Mode;
use crate::db::fleet::state::State;

/// Why a runner may not claim a piece of work, in precedence order: the first that applies is
/// the one reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shortfall {
    Unregistered,
    Cordoned,
    Revoked,
    Outdated { version: Version, minimum: Version },
    LabelsMissing { missing: Labels },
}

impl Shortfall {
    /// Judges a registered runner the way `eligibility::UNATTENDED` and
    /// `eligibility::INTERACTIVE` do. `missing` is the required labels the runner lacks.
    pub fn of(
        mode: Mode,
        state: State,
        version: &Version,
        minimum: Option<&Minimum>,
        missing: Labels,
    ) -> Option<Self> {
        match (state, mode) {
            (State::Cordoned, Mode::Unattended) => return Some(Self::Cordoned),
            (State::Revoked, _) => return Some(Self::Revoked),
            (State::Active | State::Cordoned, _) => {}
        }
        if let Some(minimum) = minimum.filter(|minimum| !minimum.admits(version)) {
            return Some(Self::Outdated {
                version: version.clone(),
                minimum: minimum.version().clone(),
            });
        }
        match mode {
            Mode::Unattended if !missing.is_empty() => Some(Self::LabelsMissing { missing }),
            Mode::Unattended | Mode::Interactive => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn labels(text: &str) -> Labels {
        Labels::parse_list(text).unwrap()
    }

    #[test]
    fn a_current_active_runner_with_its_labels_falls_short_of_nothing() {
        let minimum = Minimum::parse("1.4.0").unwrap();
        for mode in [Mode::Unattended, Mode::Interactive] {
            assert_eq!(
                Shortfall::of(
                    mode,
                    State::Active,
                    &version("1.4.0"),
                    Some(&minimum),
                    Labels::default()
                ),
                None
            );
        }
    }

    #[test]
    fn state_comes_before_version_and_version_before_labels() {
        let minimum = Minimum::parse("2.0.0").unwrap();
        let outdated = version("1.0.0");
        let judge = |state| {
            Shortfall::of(
                Mode::Unattended,
                state,
                &outdated,
                Some(&minimum),
                labels("gpu"),
            )
        };
        assert_eq!(judge(State::Cordoned), Some(Shortfall::Cordoned));
        assert_eq!(judge(State::Revoked), Some(Shortfall::Revoked));
        assert_eq!(
            judge(State::Active),
            Some(Shortfall::Outdated {
                version: outdated.clone(),
                minimum: version("2.0.0"),
            })
        );
        assert_eq!(
            Shortfall::of(
                Mode::Unattended,
                State::Active,
                &version("2.0.0"),
                Some(&minimum),
                labels("gpu"),
            ),
            Some(Shortfall::LabelsMissing {
                missing: labels("gpu")
            })
        );
    }

    #[test]
    fn interactive_work_runs_on_a_cordoned_runner_and_ignores_labels() {
        assert_eq!(
            Shortfall::of(
                Mode::Interactive,
                State::Cordoned,
                &version("1.0.0"),
                None,
                labels("gpu")
            ),
            None
        );
        assert_eq!(
            Shortfall::of(
                Mode::Interactive,
                State::Revoked,
                &version("1.0.0"),
                None,
                Labels::default()
            ),
            Some(Shortfall::Revoked)
        );
        assert_eq!(
            Shortfall::of(
                Mode::Interactive,
                State::Cordoned,
                &version("1.0.0-rc.1"),
                Some(&Minimum::parse("1.0.0").unwrap()),
                Labels::default()
            ),
            Some(Shortfall::Outdated {
                version: version("1.0.0-rc.1"),
                minimum: version("1.0.0"),
            })
        );
    }
}
