//! What one sweep did.

use super::job::Failure;

/// How a sweep ended.
///
/// A panic is an outcome rather than an error because the worker treats it as
/// one: the sweep runs as its own task, so unwinding costs that turn and
/// nothing else, and the next turn is scheduled exactly as it would have been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed(Failure),
    Panicked,
}

impl Outcome {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn failure(&self) -> Option<&Failure> {
        match self {
            Self::Failed(failure) => Some(failure),
            Self::Completed | Self::Panicked => None,
        }
    }
}

impl From<Result<(), Failure>> for Outcome {
    fn from(result: Result<(), Failure>) -> Self {
        match result {
            Ok(()) => Self::Completed,
            Err(failure) => Self::Failed(failure),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_sweep_reads_as_completed() {
        let outcome = Outcome::from(Ok(()));

        assert!(outcome.is_completed());
        assert_eq!(outcome.failure(), None);
    }

    #[test]
    fn a_failed_sweep_keeps_the_reason_it_gave() {
        let outcome = Outcome::from(Err(Failure::new("statement timeout")));

        assert!(!outcome.is_completed());
        assert_eq!(
            outcome.failure().map(Failure::reason),
            Some("statement timeout")
        );
    }

    #[test]
    fn a_panic_is_neither_a_success_nor_a_reason() {
        let outcome = Outcome::Panicked;

        assert!(!outcome.is_completed());
        assert_eq!(outcome.failure(), None);
    }
}
