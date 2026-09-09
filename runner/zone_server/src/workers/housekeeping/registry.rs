//! Every periodic job the server runs, in one place.

use std::sync::Arc;

use super::job::Job;
use super::schedule::Schedule;

/// The ordered set of jobs the housekeeping worker drives.
///
/// Registration order is kept, and it is the order jobs are dispatched in when
/// several fall due on the same wake-up. That makes the registry readable as a
/// list — the cadence of the whole server in one screen — instead of a cadence
/// per module, discovered by grepping for `Duration::from_secs`.
#[derive(Clone, Default)]
pub struct Registry {
    jobs: Vec<Arc<dyn Job>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, job: impl Job) -> Self {
        self.register(Arc::new(job));
        self
    }

    pub fn register(&mut self, job: Arc<dyn Job>) {
        self.jobs.push(job);
    }

    pub fn jobs(&self) -> &[Arc<dyn Job>] {
        &self.jobs
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// The registered jobs, in the order they will be dispatched.
    pub fn names(&self) -> Vec<&'static str> {
        self.jobs.iter().map(|job| job.name()).collect()
    }

    /// The cadence one job was registered with.
    pub fn schedule(&self, name: &str) -> Option<Schedule> {
        self.jobs
            .iter()
            .find(|job| job.name() == name)
            .map(|job| job.schedule())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use super::super::catchup::Catchup;
    use super::super::sweep::Sweep;
    use super::super::warmup::Warmup;
    use super::*;

    fn sweep(name: &'static str, seconds: u64, catchup: Catchup) -> Sweep {
        Sweep::new(
            name,
            Schedule::new(Duration::from_secs(seconds), Warmup::Period, catchup),
            || async { Ok(()) },
        )
    }

    fn registry() -> Registry {
        Registry::new()
            .with(sweep("refresh", 300, Catchup::Skip))
            .with(sweep("resync", 300, Catchup::Delay))
            .with(sweep("learning", 21_600, Catchup::Skip))
    }

    #[test]
    fn an_empty_registry_has_nothing_to_drive() {
        let registry = Registry::new();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.names().is_empty());
    }

    #[test]
    fn jobs_keep_the_order_they_were_registered_in() {
        assert_eq!(registry().names(), ["refresh", "resync", "learning"]);
    }

    #[test]
    fn a_registered_job_reports_the_cadence_it_was_given() {
        let registry = registry();

        let learning = registry
            .schedule("learning")
            .expect("learning is registered");
        assert_eq!(learning.period(), Duration::from_secs(21_600));
        assert_eq!(learning.catchup(), Catchup::Skip);

        let resync = registry.schedule("resync").expect("resync is registered");
        assert_eq!(resync.catchup(), Catchup::Delay);
    }

    #[test]
    fn a_job_that_was_never_registered_has_no_cadence() {
        assert_eq!(registry().schedule("reminders"), None);
    }

    #[test]
    fn every_name_is_distinct_so_a_log_line_says_which_job_it_came_from() {
        let names = registry().names();
        let distinct: BTreeSet<_> = names.iter().collect();

        assert_eq!(distinct.len(), names.len());
    }
}
