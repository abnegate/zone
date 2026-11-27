//! The one loop that drives every periodic sweep.

use std::collections::HashMap;
use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::{AbortHandle, Id, JoinHandle, JoinSet};
use tokio::time::{Instant, sleep_until};

use super::jitter::Jitter;
use super::job::Failure;
use super::leader::Leadership;
use super::outcome::Outcome;
use super::periodic::periodic;
use super::registry::Registry;
use super::report::Report;
use super::scope::Scope;
use super::slot::Slot;
use crate::state::AppState;

/// Start the housekeeping worker for this server. A server that starts
/// draining gives leadership up at once, so another replica takes the cluster
/// sweeps over while this one finishes what it serves.
pub fn spawn(state: AppState, leadership: Leadership) -> JoinHandle<()> {
    let drain = state.drain().clone();
    let resigning = leadership.clone();
    tokio::spawn(async move {
        drain.draining().await;
        resigning.release().await;
    });
    Worker::new(periodic(state), leadership).spawn()
}

/// Which job a running sweep belongs to, when it started, and how to stop it.
#[derive(Debug)]
struct Dispatch {
    index: usize,
    scope: Scope,
    started: Instant,
    handle: AbortHandle,
    aborted: bool,
}

impl Dispatch {
    fn needs_leadership(&self) -> bool {
        self.scope == Scope::Cluster && !self.aborted
    }
}

/// Runs a [`Registry`] of periodic jobs on one timer.
///
/// Every sweep is its own task in a [`JoinSet`], and the set is what makes the
/// worker unkillable by the work it runs: a sweep that fails is a `Result` the
/// loop logs, a sweep that panics is a [`tokio::task::JoinError`] the loop maps
/// back to its job through the task id, and neither reaches the loop's own
/// stack. This is the same arrangement [`abnegate_notify::Fanout`] uses to keep one
/// broken webhook from costing a notification its other channels.
///
/// A [`Scope::Cluster`] job is dispatched only while `leadership` leads, checked at the moment
/// of dispatch, and is aborted as soon as it stops leading. A turn that falls due while a new
/// leader sits out its wait is held until the wait ends, so a single server still runs its
/// start-up sweeps. A follower's cluster turns are reported as [`Outcome::Skipped`] and its
/// slots keep their cadence.
pub struct Worker {
    registry: Registry,
    leadership: Leadership,
    jitter: Jitter,
    reports: Option<UnboundedSender<Report>>,
}

impl Worker {
    pub fn new(registry: Registry, leadership: Leadership) -> Self {
        Self {
            registry,
            leadership,
            jitter: Jitter::default(),
            reports: None,
        }
    }

    #[must_use]
    pub fn with_jitter(mut self, jitter: Jitter) -> Self {
        self.jitter = jitter;
        self
    }

    /// Also publish every outcome to `reports`, for a caller that needs to
    /// observe sweeps rather than read about them in the log.
    #[must_use]
    pub fn reporting_to(mut self, reports: UnboundedSender<Report>) -> Self {
        self.reports = Some(reports);
        self
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(self) {
        if self.registry.is_empty() {
            tracing::info!("No periodic housekeeping jobs are registered");
            return;
        }

        let start = Instant::now();
        let mut slots: Vec<Slot> = self
            .registry
            .jobs()
            .iter()
            .map(|job| {
                let schedule = job.schedule();
                Slot::first(&schedule, start, self.jitter.draw(schedule.period()))
            })
            .collect();

        tracing::info!(
            jobs = ?self.registry.names(),
            "Housekeeping worker started"
        );

        let mut sweeps: JoinSet<Result<(), Failure>> = JoinSet::new();
        let mut owners: HashMap<Id, Dispatch> = HashMap::with_capacity(self.registry.len());

        loop {
            let wake = slots.iter().filter_map(Slot::due).min();
            let leading = owners.values().any(Dispatch::needs_leadership);

            tokio::select! {
                () = waiting(wake) => self.dispatch(&mut slots, &mut sweeps, &mut owners),
                () = self.leadership.lost(), if leading => self.abort(&mut owners),
                Some(joined) = sweeps.join_next_with_id(), if !sweeps.is_empty() => {
                    let (id, outcome) = match joined {
                        Ok((id, result)) => (id, Outcome::from(result)),
                        Err(error) if error.is_panic() => (error.id(), Outcome::Panicked),
                        Err(error) => (error.id(), Outcome::Aborted),
                    };
                    self.settle(&mut slots, &mut owners, id, outcome);
                }
            }
        }
    }

    fn dispatch(
        &self,
        slots: &mut [Slot],
        sweeps: &mut JoinSet<Result<(), Failure>>,
        owners: &mut HashMap<Id, Dispatch>,
    ) {
        let now = Instant::now();

        for (index, slot) in slots.iter_mut().enumerate() {
            if slot.take(now).is_none() {
                continue;
            }
            let job = Arc::clone(&self.registry.jobs()[index]);
            let scope = job.scope();
            if scope == Scope::Cluster && !self.leadership.leads() {
                if let Some(ready) = self.leadership.ready() {
                    slot.defer(ready);
                    continue;
                }
                let schedule = job.schedule();
                slot.settle(&schedule, now);
                self.publish(
                    Report::new(job.name(), Outcome::Skipped, Duration::ZERO),
                    schedule.period(),
                );
                continue;
            }
            tracing::debug!(job = job.name(), "Housekeeping sweep started");
            let handle = sweeps.spawn(job.sweep());
            owners.insert(
                handle.id(),
                Dispatch {
                    index,
                    scope,
                    started: now,
                    handle,
                    aborted: false,
                },
            );
        }
    }

    fn abort(&self, owners: &mut HashMap<Id, Dispatch>) {
        for dispatch in owners
            .values_mut()
            .filter(|dispatch| dispatch.needs_leadership())
        {
            tracing::warn!(
                job = self.registry.jobs()[dispatch.index].name(),
                "This replica stopped leading; aborting its cluster sweep"
            );
            dispatch.handle.abort();
            dispatch.aborted = true;
        }
    }

    fn settle(
        &self,
        slots: &mut [Slot],
        owners: &mut HashMap<Id, Dispatch>,
        id: Id,
        outcome: Outcome,
    ) {
        let Some(dispatch) = owners.remove(&id) else {
            tracing::error!("A housekeeping sweep finished without a job to attribute it to");
            return;
        };

        let now = Instant::now();
        let job = &self.registry.jobs()[dispatch.index];
        let schedule = job.schedule();
        let elapsed = now.saturating_duration_since(dispatch.started);
        slots[dispatch.index].settle(&schedule, now);

        self.publish(Report::new(job.name(), outcome, elapsed), schedule.period());
    }

    fn publish(&self, report: Report, period: Duration) {
        match report.outcome() {
            Outcome::Completed if report.elapsed() > period => tracing::warn!(
                job = report.job(),
                elapsed_seconds = report.elapsed().as_secs(),
                period_seconds = period.as_secs(),
                "Housekeeping sweep outran its own period"
            ),
            Outcome::Completed => tracing::debug!(
                job = report.job(),
                elapsed_seconds = report.elapsed().as_secs(),
                "Housekeeping sweep finished"
            ),
            Outcome::Failed(failure) => tracing::warn!(
                job = report.job(),
                reason = failure.reason(),
                "Housekeeping sweep failed; retrying on its next turn"
            ),
            Outcome::Panicked => tracing::error!(
                job = report.job(),
                "Housekeeping sweep panicked; retrying on its next turn"
            ),
            Outcome::Skipped => tracing::trace!(
                job = report.job(),
                "Housekeeping cluster sweep skipped; another replica leads"
            ),
            Outcome::Aborted => tracing::warn!(
                job = report.job(),
                elapsed_seconds = report.elapsed().as_secs(),
                "Housekeeping cluster sweep aborted when this replica stopped leading"
            ),
        }

        if let Some(reports) = &self.reports {
            let _ = reports.send(report);
        }
    }
}

/// Wait for the earliest turn, or forever when every job is already sweeping.
async fn waiting(wake: Option<Instant>) {
    match wake {
        Some(wake) => sleep_until(wake).await,
        None => pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::mpsc::{self, UnboundedReceiver};

    use super::super::catchup::Catchup;
    use super::super::leader::Timing;
    use super::super::schedule::Schedule;
    use super::super::sweep::Sweep;
    use super::super::warmup::Warmup;
    use super::*;

    const PERIOD: Duration = Duration::from_secs(60);

    fn follower() -> Leadership {
        Leadership::manual(Timing::default())
    }

    fn leader() -> Leadership {
        let leadership = follower();
        leadership.promote(Instant::now() - Timing::default().wait);
        leadership
    }

    /// Answer a ping every `ping` interval for as long as the test runs.
    fn confirming(leadership: &Leadership) -> JoinHandle<()> {
        let leadership = leadership.clone();
        tokio::spawn(async move {
            loop {
                leadership.confirm(Instant::now());
                tokio::time::sleep(leadership.timing().ping).await;
            }
        })
    }

    fn cluster(name: &'static str, seconds: u64, turns: &Arc<AtomicUsize>) -> Sweep {
        let turns = Arc::clone(turns);
        Sweep::new(
            name,
            every(seconds, Catchup::Skip),
            Scope::Cluster,
            move || {
                let turns = Arc::clone(&turns);
                async move {
                    turns.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        )
    }

    fn lingering(name: &'static str, finished: &Arc<AtomicUsize>) -> Sweep {
        let finished = Arc::clone(finished);
        Sweep::new(name, every(60, Catchup::Skip), Scope::Cluster, move || {
            let finished = Arc::clone(&finished);
            async move {
                tokio::time::sleep(Duration::from_secs(600)).await;
                finished.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_follower_skips_cluster_turns_but_keeps_their_cadence() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let clustered = Arc::new(AtomicUsize::new(0));
        let local = Arc::new(AtomicUsize::new(0));
        let registry = Registry::new()
            .with(cluster("clustered", 60, &clustered))
            .with(counting("local", every(60, Catchup::Skip), &local));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(185)).await;
        handle.abort();

        assert_eq!(
            clustered.load(Ordering::SeqCst),
            0,
            "a follower never runs a cluster sweep"
        );
        assert_eq!(
            local.load(Ordering::SeqCst),
            3,
            "a replica sweep runs on every replica"
        );
        let skipped = drain(&mut receiver)
            .into_iter()
            .filter(|report| report.outcome() == &Outcome::Skipped)
            .map(|report| report.job())
            .collect::<Vec<_>>();
        assert_eq!(skipped, ["clustered", "clustered", "clustered"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_leader_runs_its_cluster_turns() {
        let clustered = Arc::new(AtomicUsize::new(0));
        let leadership = leader();
        let pinging = confirming(&leadership);

        let handle = Worker::new(
            Registry::new().with(cluster("clustered", 60, &clustered)),
            leadership,
        )
        .with_jitter(Jitter::none())
        .spawn();
        tokio::time::sleep(Duration::from_secs(185)).await;
        handle.abort();
        pinging.abort();

        assert_eq!(clustered.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_leader_sits_out_its_wait_before_the_first_cluster_sweep() {
        let clustered = Arc::new(AtomicUsize::new(0));
        let leadership = follower();
        leadership.promote(Instant::now());
        let pinging = confirming(&leadership);

        let handle = Worker::new(
            Registry::new().with(cluster("clustered", 1, &clustered)),
            leadership,
        )
        .with_jitter(Jitter::none())
        .spawn();
        tokio::time::sleep(Timing::default().wait - Duration::from_millis(500)).await;
        let waiting = clustered.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(2)).await;
        handle.abort();
        pinging.abort();

        assert_eq!(
            waiting, 0,
            "no cluster sweep may start inside the wait after acquiring"
        );
        assert!(clustered.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_turn_due_during_the_wait_runs_when_the_wait_ends_rather_than_a_period_later() {
        let turns = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Mutex::new(None));
        let leadership = follower();
        let promoted = Instant::now();
        leadership.promote(promoted);
        let pinging = confirming(&leadership);
        let sweep = {
            let turns = Arc::clone(&turns);
            let started = Arc::clone(&started);
            Sweep::new("start-up", immediately(600), Scope::Cluster, move || {
                let turns = Arc::clone(&turns);
                let started = Arc::clone(&started);
                async move {
                    started
                        .lock()
                        .expect("no sweep panics")
                        .get_or_insert(Instant::now());
                    turns.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
        };

        let handle = Worker::new(Registry::new().with(sweep), leadership)
            .with_jitter(Jitter::none())
            .spawn();
        tokio::time::sleep(Timing::default().wait + Duration::from_secs(10)).await;
        handle.abort();
        pinging.abort();

        assert_eq!(turns.load(Ordering::SeqCst), 1);
        assert_eq!(
            *started.lock().expect("no sweep panics"),
            Some(promoted + Timing::default().wait),
            "a single server's start-up sweep waits out the election, not a whole period"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn demotion_aborts_a_running_cluster_sweep_and_spares_replica_sweeps() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let finished = Arc::new(AtomicUsize::new(0));
        let local = Arc::new(AtomicUsize::new(0));
        let leadership = leader();
        let pinging = confirming(&leadership);
        let registry = Registry::new()
            .with(lingering("lingering", &finished))
            .with(counting("local", every(60, Catchup::Skip), &local));

        let handle = Worker::new(registry, leadership.clone())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(90)).await;
        pinging.abort();
        leadership.demote();
        tokio::time::sleep(Duration::from_secs(1)).await;

        let aborted: Vec<_> = drain(&mut receiver)
            .into_iter()
            .filter(|report| report.outcome() == &Outcome::Aborted)
            .map(|report| report.job())
            .collect();
        assert_eq!(
            aborted,
            ["lingering"],
            "demotion stops the cluster sweep at once"
        );

        tokio::time::sleep(Duration::from_secs(1_200)).await;
        handle.abort();
        assert_eq!(
            finished.load(Ordering::SeqCst),
            0,
            "an aborted sweep never finishes"
        );
        assert!(
            local.load(Ordering::SeqCst) >= 20,
            "replica sweeps keep running after demotion"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_leader_whose_pings_stop_being_answered_aborts_without_being_told() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let finished = Arc::new(AtomicUsize::new(0));
        let leadership = leader();
        let pinging = confirming(&leadership);

        let handle = Worker::new(
            Registry::new().with(lingering("lingering", &finished)),
            leadership.clone(),
        )
        .with_jitter(Jitter::none())
        .reporting_to(sender)
        .spawn();
        tokio::time::sleep(Duration::from_secs(61)).await;
        pinging.abort();
        let confirmed = leadership
            .tenure()
            .expect("still recorded as held")
            .confirmed();

        let report = receiver.recv().await.expect("the sweep is reported");
        handle.abort();

        assert_eq!(report.outcome(), &Outcome::Aborted);
        assert!(leadership.tenure().is_some(), "nobody demoted it");
        assert!(
            Instant::now() <= confirmed + Timing::default().freshness,
            "the sweep must stop by the freshness deadline, not after it"
        );
    }

    fn every(seconds: u64, catchup: Catchup) -> Schedule {
        Schedule::new(Duration::from_secs(seconds), Warmup::Period, catchup)
    }

    fn immediately(seconds: u64) -> Schedule {
        Schedule::new(
            Duration::from_secs(seconds),
            Warmup::Immediate,
            Catchup::Skip,
        )
    }

    fn counting(name: &'static str, schedule: Schedule, turns: &Arc<AtomicUsize>) -> Sweep {
        let turns = Arc::clone(turns);
        Sweep::new(name, schedule, Scope::Replica, move || {
            let turns = Arc::clone(&turns);
            async move {
                turns.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
    }

    fn drain(receiver: &mut UnboundedReceiver<Report>) -> Vec<Report> {
        let mut collected = Vec::new();
        while let Ok(report) = receiver.try_recv() {
            collected.push(report);
        }
        collected
    }

    fn names(reports: &[Report]) -> Vec<&'static str> {
        reports.iter().map(Report::job).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn an_empty_registry_stops_instead_of_spinning() {
        Worker::new(Registry::new(), follower()).run().await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_sweeps_once_per_period() {
        let turns = Arc::new(AtomicUsize::new(0));
        let worker = Worker::new(
            Registry::new().with(counting("counted", every(60, Catchup::Skip), &turns)),
            follower(),
        )
        .with_jitter(Jitter::none());

        let handle = worker.spawn();
        tokio::time::sleep(Duration::from_secs(305)).await;
        handle.abort();

        assert_eq!(
            turns.load(Ordering::SeqCst),
            5,
            "five minutes of a one-minute job, after sitting out the first period"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_warm_up_period_keeps_the_first_sweep_off_the_start_up_path() {
        let turns = Arc::new(AtomicUsize::new(0));
        let worker = Worker::new(
            Registry::new().with(counting("counted", every(60, Catchup::Skip), &turns)),
            follower(),
        )
        .with_jitter(Jitter::none());

        let handle = worker.spawn();
        tokio::time::sleep(Duration::from_secs(59)).await;
        assert_eq!(turns.load(Ordering::SeqCst), 0);

        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(turns.load(Ordering::SeqCst), 1);
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn an_immediate_job_sweeps_without_waiting_for_a_period() {
        let turns = Arc::new(AtomicUsize::new(0));
        let worker = Worker::new(
            Registry::new().with(counting("counted", immediately(60), &turns)),
            follower(),
        )
        .with_jitter(Jitter::none());

        let handle = worker.spawn();
        tokio::time::sleep(Duration::from_secs(1)).await;
        handle.abort();

        assert_eq!(turns.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn jitter_delays_a_first_sweep_without_disturbing_the_period() {
        let turns = Arc::new(AtomicUsize::new(0));
        let worker = Worker::new(
            Registry::new().with(counting("counted", every(60, Catchup::Skip), &turns)),
            follower(),
        )
        .with_jitter(Jitter::new(1.0));

        let handle = worker.spawn();
        tokio::time::sleep(Duration::from_secs(59)).await;
        let early = turns.load(Ordering::SeqCst);

        tokio::time::sleep(Duration::from_secs(361)).await;
        handle.abort();
        let total = turns.load(Ordering::SeqCst);

        assert_eq!(
            early, 0,
            "a jittered first sweep cannot run before its turn"
        );
        assert!(
            (6..=7).contains(&total),
            "seven minutes of a one-minute job swept {total} times; jitter must offset the \
             phase by under a period, not change the cadence"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_sweep_that_outruns_its_period_is_never_started_alongside_itself() {
        let inside = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let turns = Arc::new(AtomicUsize::new(0));

        let sweep = {
            let inside = Arc::clone(&inside);
            let peak = Arc::clone(&peak);
            let turns = Arc::clone(&turns);
            Sweep::new("slow", every(1, Catchup::Skip), Scope::Replica, move || {
                let inside = Arc::clone(&inside);
                let peak = Arc::clone(&peak);
                let turns = Arc::clone(&turns);
                async move {
                    let concurrent = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(concurrent, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    turns.fetch_add(1, Ordering::SeqCst);
                    inside.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }
            })
        };

        let handle = Worker::new(Registry::new().with(sweep), follower())
            .with_jitter(Jitter::none())
            .spawn();
        tokio::time::sleep(Duration::from_secs(120)).await;
        handle.abort();

        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "a sweep ten times longer than its period must never overlap itself"
        );
        assert!(
            turns.load(Ordering::SeqCst) >= 2,
            "overlap prevention must not stop the job running again once it is free"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_sweep_costs_its_own_turn_and_nothing_else() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let survivor = Arc::new(AtomicUsize::new(0));
        let registry = Registry::new()
            .with(Sweep::new(
                "panicking",
                every(60, Catchup::Skip),
                Scope::Replica,
                || async {
                    panic!("a panicking sweep must not take the housekeeping worker with it")
                },
            ))
            .with(counting("surviving", every(60, Catchup::Skip), &survivor));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(185)).await;
        handle.abort();

        let reports = drain(&mut receiver);
        let panics: Vec<_> = reports
            .iter()
            .filter(|report| report.outcome() == &Outcome::Panicked)
            .collect();

        assert_eq!(panics.len(), 3, "the panicking job is retried every period");
        assert!(panics.iter().all(|report| report.job() == "panicking"));
        assert_eq!(
            survivor.load(Ordering::SeqCst),
            3,
            "a sibling of a panicking job keeps its own cadence"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failing_sweep_is_reported_and_comes_back_next_turn() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let registry = Registry::new().with(Sweep::new(
            "failing",
            every(60, Catchup::Skip),
            Scope::Replica,
            || async { Err(Failure::new("pool timed out")) },
        ));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(185)).await;
        handle.abort();

        let reports = drain(&mut receiver);
        assert_eq!(reports.len(), 3, "a failure must not stop the next turn");
        for report in &reports {
            assert_eq!(
                report.outcome().failure().map(Failure::reason),
                Some("pool timed out")
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn jobs_that_fall_due_together_are_dispatched_in_registration_order() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut registry = Registry::new();
        for name in ["first", "second", "third"] {
            let recorded = Arc::clone(&order);
            registry = registry.with(Sweep::new(
                name,
                every(60, Catchup::Skip),
                Scope::Replica,
                move || {
                    let recorded = Arc::clone(&recorded);
                    async move {
                        recorded.lock().expect("no sweep panics").push(name);
                        Ok(())
                    }
                },
            ));
        }

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .spawn();
        tokio::time::sleep(Duration::from_secs(61)).await;
        handle.abort();

        assert_eq!(
            *order.lock().expect("no sweep panics"),
            ["first", "second", "third"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn each_job_keeps_its_own_period() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let registry = Registry::new()
            .with(Sweep::new(
                "fast",
                every(60, Catchup::Skip),
                Scope::Replica,
                || async { Ok(()) },
            ))
            .with(Sweep::new(
                "slow",
                every(300, Catchup::Skip),
                Scope::Replica,
                || async { Ok(()) },
            ));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(605)).await;
        handle.abort();

        let reports = drain(&mut receiver);
        let swept = names(&reports);

        assert_eq!(swept.iter().filter(|job| **job == "fast").count(), 10);
        assert_eq!(swept.iter().filter(|job| **job == "slow").count(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_delaying_job_gives_itself_a_whole_period_after_overrunning() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let registry = Registry::new().with(Sweep::new(
            "delaying",
            every(60, Catchup::Delay),
            Scope::Replica,
            || async {
                tokio::time::sleep(Duration::from_secs(90)).await;
                Ok(())
            },
        ));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(560)).await;
        handle.abort();

        assert_eq!(
            drain(&mut receiver).len(),
            3,
            "ninety seconds of work plus a fresh sixty-second period is one sweep per 150s, \
             so nine minutes buys three"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_skipping_job_rejoins_its_grid_after_overrunning() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let registry = Registry::new().with(Sweep::new(
            "skipping",
            every(60, Catchup::Skip),
            Scope::Replica,
            || async {
                tokio::time::sleep(Duration::from_secs(90)).await;
                Ok(())
            },
        ));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(560)).await;
        handle.abort();

        assert_eq!(
            drain(&mut receiver).len(),
            4,
            "rejoining the minute grid after ninety seconds is one sweep per 120s, which is \
             one more than the same job would get by delaying"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_report_measures_how_long_its_sweep_took() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let registry = Registry::new().with(Sweep::new(
            "timed",
            immediately(600),
            Scope::Replica,
            || async {
                tokio::time::sleep(Duration::from_secs(7)).await;
                Ok(())
            },
        ));

        let handle = Worker::new(registry, follower())
            .with_jitter(Jitter::none())
            .reporting_to(sender)
            .spawn();
        tokio::time::sleep(Duration::from_secs(30)).await;
        handle.abort();

        let reports = drain(&mut receiver);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].elapsed(), Duration::from_secs(7));
        assert_eq!(reports[0].outcome(), &Outcome::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn the_worker_reports_every_job_it_was_given() {
        let worker = Worker::new(
            Registry::new()
                .with(Sweep::new(
                    "one",
                    every(60, Catchup::Skip),
                    Scope::Replica,
                    || async { Ok(()) },
                ))
                .with(Sweep::new(
                    "two",
                    every(60, Catchup::Skip),
                    Scope::Replica,
                    || async { Ok(()) },
                )),
            follower(),
        );

        assert_eq!(worker.registry().names(), ["one", "two"]);
        assert_eq!(
            worker.registry().schedule("one").map(|one| one.period()),
            Some(PERIOD)
        );
    }
}
