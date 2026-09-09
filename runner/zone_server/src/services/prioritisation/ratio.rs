//! A value confined to the closed unit interval.
//!
//! Every score component is a proportion, and a single NaN anywhere in the
//! arithmetic would poison a total and make the queue ordering
//! non-deterministic. Constructing through this type is the only way in, so the
//! invariant holds by construction rather than by convention.

use serde::Serialize;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Ratio(f64);

impl Ratio {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);

    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            return Self::ZERO;
        }
        Self(value.clamp(Self::ZERO.0, Self::ONE.0))
    }

    pub fn of(numerator: u64, denominator: u64) -> Self {
        if denominator == 0 {
            return Self::ZERO;
        }
        Self::new(numerator as f64 / denominator as f64)
    }

    pub fn value(self) -> f64 {
        self.0
    }
}

impl Eq for Ratio {}

impl Ord for Ratio {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::Ratio;

    #[test]
    fn clamps_outside_the_unit_interval() {
        assert_eq!(Ratio::new(4.2), Ratio::ONE);
        assert_eq!(Ratio::new(-4.2), Ratio::ZERO);
    }

    #[test]
    fn collapses_not_a_number_to_zero() {
        assert_eq!(
            Ratio::new(f64::NAN),
            Ratio::ZERO,
            "a NaN input must never reach a score total"
        );
    }

    #[test]
    fn division_by_zero_yields_zero() {
        assert_eq!(Ratio::of(7, 0), Ratio::ZERO);
    }

    #[test]
    fn divides_into_a_proportion() {
        assert_eq!(Ratio::of(1, 4), Ratio::new(0.25));
    }

    #[test]
    fn orders_by_value() {
        assert!(Ratio::new(0.25) < Ratio::new(0.75));
    }
}
