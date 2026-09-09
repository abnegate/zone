//! A job built from a schedule and the work to run on it.

use std::sync::Arc;

use super::job::{Failure, Job, Sweeping};
use super::schedule::Schedule;

/// The [`Job`] every registered sweep is built from.
///
/// The work is a closure rather than a trait implementation per worker, so
/// moving an existing loop onto the registry is a matter of naming its cadence
/// and pointing at the cycle function it already had. Anything a sweep needs to
/// remember between turns is captured by the closure; the worker never runs two
/// sweeps of the same job at once, so nothing captured has to guard against
/// itself.
#[derive(Clone)]
pub struct Sweep {
    name: &'static str,
    schedule: Schedule,
    work: Arc<dyn Fn() -> Sweeping + Send + Sync>,
}

impl Sweep {
    pub fn new<Work, Sweeps>(name: &'static str, schedule: Schedule, work: Work) -> Self
    where
        Work: Fn() -> Sweeps + Send + Sync + 'static,
        Sweeps: Future<Output = Result<(), Failure>> + Send + 'static,
    {
        Self {
            name,
            schedule,
            work: Arc::new(move || Box::pin(work())),
        }
    }
}

impl Job for Sweep {
    fn name(&self) -> &'static str {
        self.name
    }

    fn schedule(&self) -> Schedule {
        self.schedule
    }

    fn sweep(&self) -> Sweeping {
        (self.work)()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::catchup::Catchup;
    use super::super::warmup::Warmup;
    use super::*;

    fn schedule() -> Schedule {
        Schedule::new(Duration::from_secs(60), Warmup::Period, Catchup::Skip)
    }

    #[tokio::test]
    async fn a_sweep_reports_the_name_and_schedule_it_was_registered_with() {
        let sweep = Sweep::new("resync", schedule(), || async { Ok(()) });

        assert_eq!(sweep.name(), "resync");
        assert_eq!(sweep.schedule(), schedule());
    }

    #[tokio::test]
    async fn each_turn_runs_the_work_again() {
        let turns = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&turns);
        let sweep = Sweep::new("counting", schedule(), move || {
            let counted = Arc::clone(&counted);
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });

        for _ in 0..3 {
            sweep.sweep().await.expect("the work succeeds");
        }

        assert_eq!(turns.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_failing_sweep_hands_back_the_reason() {
        let sweep = Sweep::new("failing", schedule(), || async {
            Err(Failure::new("connection refused"))
        });

        assert_eq!(sweep.sweep().await, Err(Failure::new("connection refused")));
    }
}
