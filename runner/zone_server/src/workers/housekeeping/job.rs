//! One piece of periodic work, and the way it reports going wrong.

use std::error::Error;
use std::fmt::{self, Display};
use std::future::Future;
use std::pin::Pin;

use super::schedule::Schedule;

/// A sweep in flight.
///
/// Owning nothing borrowed from the job is deliberate: the worker hands each
/// sweep to [`tokio::task::JoinSet`], which needs a `'static` future, and that
/// is what buys the isolation — a sweep that panics takes down a task the
/// worker is watching rather than the worker itself.
pub type Sweeping = Pin<Box<dyn Future<Output = Result<(), Failure>> + Send>>;

/// Why one sweep did not finish its work.
///
/// Sweeps fail for unrelated reasons — a database error, an expired GitHub
/// token, a webhook nobody updated — and the worker does nothing with the
/// difference beyond logging it, so the reason is flattened to its message at
/// the boundary rather than propagated as a mixed error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    reason: String,
}

impl Failure {
    pub fn new(reason: impl Display) -> Self {
        Self {
            reason: reason.to_string(),
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl Error for Failure {}

/// Work the housekeeping worker runs on a schedule.
pub trait Job: Send + Sync + 'static {
    /// What this job is called in a log line and in the registry.
    fn name(&self) -> &'static str;

    /// How often it sweeps, and what it owes for a turn it missed.
    fn schedule(&self) -> Schedule;

    /// Do one sweep.
    fn sweep(&self) -> Sweeping;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_carries_the_message_it_was_built_from() {
        let failure = Failure::new(std::io::Error::other("pool timed out"));

        assert_eq!(failure.reason(), "pool timed out");
        assert_eq!(failure.to_string(), "pool timed out");
    }

    #[test]
    fn a_failure_is_an_error_in_its_own_right() {
        fn accepts(_: &dyn Error) {}

        accepts(&Failure::new("no credentials configured"));
    }
}
