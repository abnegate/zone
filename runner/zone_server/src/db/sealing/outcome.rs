use std::fmt;

use super::class::Class;
use super::policy::Policy;

/// What sealing did with one stored value. A dry run counts the writes it would make.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Outcome {
    /// Plaintext sealed into an envelope.
    Sealed,
    /// A legacy body re-sealed into an envelope.
    Resealed,
    /// Left as it is and reported, because sealing it could change what a reader does.
    Reported,
    /// Left as it is, because there is nothing to seal.
    Skipped,
    /// Left as it is, because the envelope did not reopen as the credential.
    Mismatched,
    /// Left as it is, because the row changed after it was read. The next run picks it up.
    Raced,
}

impl Outcome {
    /// Every outcome, in report order.
    pub const ALL: &[Self] = &[
        Self::Sealed,
        Self::Resealed,
        Self::Reported,
        Self::Skipped,
        Self::Mismatched,
        Self::Raced,
    ];

    /// What sealing sets out to do with a value of `class` in a column under `policy`.
    pub fn planned(policy: Policy, class: Class) -> Self {
        match class {
            Class::Empty | Class::Envelope => Self::Skipped,
            _ if !policy.writes() => Self::Reported,
            Class::Legacy => Self::Resealed,
            Class::Plaintext if policy.seals_plaintext() => Self::Sealed,
            Class::Plaintext | Class::Ambiguous | Class::Unopenable => Self::Reported,
        }
    }

    /// Whether the outcome is a write.
    pub fn writes(self) -> bool {
        matches!(self, Self::Sealed | Self::Resealed)
    }

    /// The outcome as the report prints it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sealed => "sealed",
            Self::Resealed => "resealed",
            Self::Reported => "reported",
            Self::Skipped => "skipped",
            Self::Mismatched => "mismatched",
            Self::Raced => "raced",
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Class;
    use super::Outcome;
    use super::Policy;

    #[test]
    fn legacy_is_resealed_wherever_sealing_writes() {
        for policy in [Policy::Strict, Policy::Mixed, Policy::Lenient] {
            assert_eq!(Outcome::planned(policy, Class::Legacy), Outcome::Resealed);
        }
        assert_eq!(
            Outcome::planned(Policy::Counted, Class::Legacy),
            Outcome::Reported
        );
    }

    #[test]
    fn plaintext_is_sealed_only_where_every_reader_is_lenient() {
        assert_eq!(
            Outcome::planned(Policy::Lenient, Class::Plaintext),
            Outcome::Sealed
        );
        for policy in [Policy::Strict, Policy::Mixed, Policy::Counted] {
            assert_eq!(
                Outcome::planned(policy, Class::Plaintext),
                Outcome::Reported
            );
        }
    }

    #[test]
    fn nothing_doubtful_is_written_and_nothing_sealed_is_touched() {
        for policy in [
            Policy::Strict,
            Policy::Mixed,
            Policy::Lenient,
            Policy::Counted,
        ] {
            assert_eq!(
                Outcome::planned(policy, Class::Ambiguous),
                Outcome::Reported
            );
            assert_eq!(
                Outcome::planned(policy, Class::Unopenable),
                Outcome::Reported
            );
            assert_eq!(Outcome::planned(policy, Class::Envelope), Outcome::Skipped);
            assert_eq!(Outcome::planned(policy, Class::Empty), Outcome::Skipped);
        }
    }
}
