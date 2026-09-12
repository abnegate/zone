//! Every periodic sweep the server runs, and the cadence it runs on.
//!
//! This is the table item 23 exists to create. Before it, nine workers each
//! opened their own `tokio::time::interval`, picked their own period, and chose
//! a missed-tick behaviour by copying the worker written before them. How often
//! zone did background work could only be answered by reading nine files.
//!
//! [`Periodic`] answers it in one enum, and every arm of every match on it has
//! to be filled in, so a new sweep cannot be added without stating its period,
//! its warm-up and what it owes for a turn it missed.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use zone_notify::Fanout;

use super::catchup::Catchup;
use super::job::Failure;
use super::registry::Registry;
use super::schedule::Schedule;
use super::sweep::Sweep;
use super::warmup::Warmup;
use crate::config::SourceIndexConfig;
use crate::state::AppState;
use crate::workers::analytics::AnalyticsPolicy;
use crate::workers::promotion::PromotionPolicy;
use crate::workers::regression::RegressionSettings;
use crate::workers::reports::{Ledger, ReportSettings};
use crate::workers::{
    analytics, knowledge_refresh, learning, notify, promotion, reception, regression, reports,
    source_resync,
};

/// One named sweep on the server's schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Periodic {
    KnowledgeRefresh,
    SourceResync,
    AnswerPromotion,
    Learning,
    AgentAnalytics,
    RegressionWatch,
    ScheduledReports,
    ReceptionSync,
}

impl Periodic {
    /// Every sweep, in the order the server used to spawn them.
    pub const ALL: [Self; 8] = [
        Self::KnowledgeRefresh,
        Self::SourceResync,
        Self::AnswerPromotion,
        Self::Learning,
        Self::AgentAnalytics,
        Self::RegressionWatch,
        Self::ScheduledReports,
        Self::ReceptionSync,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::KnowledgeRefresh => "knowledge-refresh",
            Self::SourceResync => "source-resync",
            Self::AnswerPromotion => "answer-promotion",
            Self::Learning => "learning",
            Self::AgentAnalytics => "agent-analytics",
            Self::RegressionWatch => "regression-watch",
            Self::ScheduledReports => "scheduled-reports",
            Self::ReceptionSync => "reception-sync",
        }
    }

    /// How often the sweep runs. Only the resync is configurable, because only
    /// the resync polls something whose cost depends on the deployment.
    pub fn period(self, sources: &SourceIndexConfig) -> Duration {
        let seconds = match self {
            Self::KnowledgeRefresh => knowledge_refresh::REFRESH_CHECK_INTERVAL_SECS,
            Self::SourceResync => sources.poll_interval_secs,
            Self::AnswerPromotion => promotion::PROMOTION_INTERVAL_SECONDS,
            Self::Learning => learning::worker::LEARNING_INTERVAL_SECONDS,
            Self::AgentAnalytics => analytics::worker::ANALYTICS_INTERVAL_SECONDS,
            Self::RegressionWatch => regression::worker::REGRESSION_INTERVAL_SECONDS,
            Self::ScheduledReports => reports::worker::REPORT_TICK_SECONDS,
            Self::ReceptionSync => reception::SYNC_INTERVAL_SECONDS,
        };
        Duration::from_secs(seconds)
    }

    /// What the sweep wants to have happened before it first runs.
    pub fn warmup(self) -> Warmup {
        match self {
            // A full workspace scan is the last thing a starting server needs.
            Self::KnowledgeRefresh
            | Self::AnswerPromotion
            | Self::Learning
            | Self::AgentAnalytics
            | Self::RegressionWatch
            | Self::ReceptionSync => Warmup::Period,
            // Long enough to be out of the way of start-up, short enough that a
            // server coming back up picks up files whose embeddings failed.
            Self::SourceResync => Warmup::After(source_resync::FIRST_POLL_DELAY),
            // The tick decides when to look, not what is owed. A digest whose
            // slot passed while the process was down has to be delivered on the
            // way back up, and it cannot be delivered before it is looked for.
            Self::ScheduledReports => Warmup::Immediate,
        }
    }

    /// What the sweep owes for a turn that came round while it was still busy.
    pub fn catchup(self) -> Catchup {
        match self {
            // These read the world as it currently is: which fixes have come
            // back, what the gauges should say, which answers have earned
            // promotion. A pass that was skipped would only have reached the
            // conclusion the next pass reaches anyway, so the turns are not
            // owed, and the alignment is worth keeping so the load stays evenly
            // spaced across the hour.
            //
            // Reception is on this list deliberately rather than by default. It
            // takes a bounded bite out of a shared GitHub rate limit each pass,
            // so it is the one sweep here with a real backlog to drain — and
            // that is exactly why it must not chase the turns it missed.
            // Draining slowly is the design, not a shortfall.
            Self::AnswerPromotion
            | Self::Learning
            | Self::AgentAnalytics
            | Self::RegressionWatch
            | Self::ScheduledReports
            | Self::ReceptionSync => Catchup::Skip,
            // These two never ticked on a grid at all. Both were `sleep` loops,
            // so the wait has always been measured from the end of the previous
            // pass rather than from a fixed alignment, and both queue work
            // rather than only reading: a pass slow enough to overrun wants a
            // whole fresh interval of room, not to be chased onto a grid.
            // Calling them `Skip` because the other seven are would quietly
            // change what they do.
            Self::KnowledgeRefresh | Self::SourceResync => Catchup::Delay,
        }
    }

    pub fn schedule(self, sources: &SourceIndexConfig) -> Schedule {
        Schedule::new(self.period(sources), self.warmup(), self.catchup())
    }
}

/// Build the registry this server will run.
///
/// A sweep that is switched off is left out rather than registered and skipped,
/// so the log line the worker prints on start-up lists what will actually run.
pub fn periodic(state: AppState) -> Registry {
    let sources = state.config().source_index.clone();
    let settings = ReportSettings::from_process_environment();
    let fanout = Arc::new(notify::from_process_environment());
    reports::worker::announce(&settings, &fanout);

    let mut registry = Registry::new();

    for periodic in Periodic::ALL {
        let schedule = periodic.schedule(&sources);
        let sweep = match periodic {
            Periodic::KnowledgeRefresh => refresh_knowledge(schedule, state.clone()),
            Periodic::SourceResync if !sources.enabled => {
                tracing::info!("Source resync worker disabled");
                continue;
            }
            Periodic::SourceResync => resync_sources(schedule, state.clone()),
            Periodic::AnswerPromotion => promote_answers(schedule, state.clone()),
            Periodic::Learning => learn_from_runs(schedule, state.clone()),
            Periodic::AgentAnalytics => refresh_analytics(schedule, state.clone()),
            Periodic::RegressionWatch => {
                watch_regressions(schedule, state.clone(), Arc::clone(&fanout))
            }
            Periodic::ScheduledReports if !settings.enabled => continue,
            Periodic::ScheduledReports => deliver_reports(
                schedule,
                state.clone(),
                settings.clone(),
                Arc::clone(&fanout),
            ),
            Periodic::ReceptionSync => sync_reception(schedule, state.clone()),
        };
        registry.register(Arc::new(sweep));
    }

    registry
}

fn refresh_knowledge(schedule: Schedule, state: AppState) -> Sweep {
    let permits = knowledge_refresh::permits();
    let backoff = knowledge_refresh::backoff();

    Sweep::new(Periodic::KnowledgeRefresh.name(), schedule, move || {
        let state = state.clone();
        let permits = Arc::clone(&permits);
        let backoff = Arc::clone(&backoff);
        async move {
            knowledge_refresh::run_cycle(&state, &permits, &backoff)
                .await
                .map_err(Failure::new)
        }
    })
}

fn resync_sources(schedule: Schedule, state: AppState) -> Sweep {
    Sweep::new(Periodic::SourceResync.name(), schedule, move || {
        let state = state.clone();
        async move {
            source_resync::poll_sources(&state)
                .await
                .map_err(Failure::new)
        }
    })
}

fn promote_answers(schedule: Schedule, state: AppState) -> Sweep {
    let policy = PromotionPolicy::default();

    Sweep::new(Periodic::AnswerPromotion.name(), schedule, move || {
        let state = state.clone();
        async move {
            promotion::run_cycle(&state, &policy)
                .await
                .map_err(Failure::new)
        }
    })
}

fn learn_from_runs(schedule: Schedule, state: AppState) -> Sweep {
    let policy = learning::LearningPolicy::default();

    Sweep::new(Periodic::Learning.name(), schedule, move || {
        let state = state.clone();
        async move {
            learning::worker::run_cycle(&state, &policy)
                .await
                .map_err(Failure::new)
        }
    })
}

fn refresh_analytics(schedule: Schedule, state: AppState) -> Sweep {
    let policy = Arc::new(AnalyticsPolicy::default());

    Sweep::new(Periodic::AgentAnalytics.name(), schedule, move || {
        let state = state.clone();
        let policy = Arc::clone(&policy);
        async move {
            analytics::worker::run_cycle(&state, &policy)
                .await
                .map_err(Failure::new)
        }
    })
}

fn watch_regressions(schedule: Schedule, state: AppState, fanout: Arc<Fanout>) -> Sweep {
    let settings = Arc::new(RegressionSettings::default());
    let checkers = Arc::new(regression::worker::checkers(&settings));
    let alerted = Arc::new(Mutex::new(regression::worker::Alerted::new()));

    Sweep::new(Periodic::RegressionWatch.name(), schedule, move || {
        let state = state.clone();
        let settings = Arc::clone(&settings);
        let checkers = Arc::clone(&checkers);
        let fanout = Arc::clone(&fanout);
        let alerted = Arc::clone(&alerted);
        async move {
            let mut alerted = alerted.lock().await;
            regression::worker::run_cycle(&state, &checkers, &settings, &fanout, &mut alerted)
                .await
                .map_err(Failure::new)
        }
    })
}

fn deliver_reports(
    schedule: Schedule,
    state: AppState,
    settings: ReportSettings,
    fanout: Arc<Fanout>,
) -> Sweep {
    let checkers = Arc::new(reports::worker::checkers(&settings));
    let settings = Arc::new(settings);
    let ledger = Arc::new(Mutex::new(Ledger::new()));

    Sweep::new(Periodic::ScheduledReports.name(), schedule, move || {
        let state = state.clone();
        let settings = Arc::clone(&settings);
        let checkers = Arc::clone(&checkers);
        let fanout = Arc::clone(&fanout);
        let ledger = Arc::clone(&ledger);
        async move {
            let mut ledger = ledger.lock().await;
            reports::worker::run_cycle(&state, &settings, &checkers, &fanout, &mut ledger)
                .await
                .map_err(Failure::new)
        }
    })
}

fn sync_reception(schedule: Schedule, state: AppState) -> Sweep {
    Sweep::new(Periodic::ReceptionSync.name(), schedule, move || {
        let state = state.clone();
        async move {
            match reception::run_cycle(&state).await {
                Ok(0) => Ok(()),
                Ok(recorded) => {
                    tracing::info!("Recorded reception for {} run(s)", recorded);
                    Ok(())
                }
                Err(error) => Err(Failure::new(error)),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn sources() -> SourceIndexConfig {
        SourceIndexConfig::default()
    }

    #[test]
    fn every_worker_the_server_used_to_spawn_still_has_a_sweep() {
        assert_eq!(
            Periodic::ALL.map(Periodic::name),
            [
                "knowledge-refresh",
                "source-resync",
                "answer-promotion",
                "learning",
                "agent-analytics",
                "regression-watch",
                "scheduled-reports",
                "reception-sync",
            ],
            "main.rs spawned nine workers; eight of them are periodic sweeps and the ninth, \
             reminders, is a dispatch loop that stayed where it was. Dropping one here would \
             silently stop that background work."
        );
    }

    #[test]
    fn no_two_sweeps_share_a_name() {
        let names: BTreeSet<_> = Periodic::ALL.iter().map(|one| one.name()).collect();

        assert_eq!(names.len(), Periodic::ALL.len());
    }

    #[test]
    fn every_sweep_keeps_the_period_its_own_worker_used() {
        let sources = sources();

        for (periodic, seconds) in [
            (Periodic::KnowledgeRefresh, 300),
            (Periodic::SourceResync, 300),
            (Periodic::AnswerPromotion, 6 * 60 * 60),
            (Periodic::Learning, 6 * 60 * 60),
            (Periodic::AgentAnalytics, 15 * 60),
            (Periodic::RegressionWatch, 60 * 60),
            (Periodic::ScheduledReports, 15 * 60),
            (Periodic::ReceptionSync, 30 * 60),
        ] {
            assert_eq!(
                periodic.period(&sources),
                Duration::from_secs(seconds),
                "{} changed cadence during the move onto the housekeeping worker",
                periodic.name()
            );
        }
    }

    #[test]
    fn the_resync_period_follows_the_deployment_that_configured_it() {
        let sources = SourceIndexConfig {
            poll_interval_secs: 45,
            ..SourceIndexConfig::default()
        };

        assert_eq!(
            Periodic::SourceResync.period(&sources),
            Duration::from_secs(45)
        );
        assert_eq!(
            Periodic::KnowledgeRefresh.period(&sources),
            Duration::from_secs(300),
            "one sweep's configuration must not move another's"
        );
    }

    #[test]
    fn only_the_report_tick_runs_before_it_has_warmed_up() {
        for periodic in Periodic::ALL {
            let warmup = periodic.warmup();
            let immediate = warmup == Warmup::Immediate;

            assert_eq!(
                immediate,
                periodic == Periodic::ScheduledReports,
                "{} has warm-up {warmup:?}; only the report tick may look on start-up, \
                 because only it can owe a slot that passed while the process was down",
                periodic.name()
            );
        }
    }

    #[test]
    fn the_resync_waits_the_short_delay_its_own_loop_used() {
        assert_eq!(
            Periodic::SourceResync.warmup(),
            Warmup::After(Duration::from_secs(15))
        );
    }

    #[test]
    fn the_two_sweeps_that_were_never_on_a_grid_still_are_not() {
        for periodic in Periodic::ALL {
            let slept_rather_than_ticked = matches!(
                periodic,
                Periodic::KnowledgeRefresh | Periodic::SourceResync
            );
            let expected = if slept_rather_than_ticked {
                Catchup::Delay
            } else {
                Catchup::Skip
            };

            assert_eq!(
                periodic.catchup(),
                expected,
                "{} must state what it owes for a missed turn rather than inherit it. The \
                 refresh and resync loops slept between passes, so their wait has always \
                 been measured from the end of the last pass, not from a fixed grid.",
                periodic.name()
            );
        }
    }

    #[test]
    fn nothing_registered_chases_the_turns_it_missed() {
        assert!(
            Periodic::ALL
                .iter()
                .all(|periodic| periodic.catchup() != Catchup::Burst),
            "every registered sweep recomputes what is due from stored state, so an owed \
             backlog of turns would only repeat work. The queue-draining loop that would \
             want bursting is reminders, which is not on this worker."
        );
    }

    #[test]
    fn a_schedule_is_the_three_parts_the_sweep_declared() {
        let sources = sources();

        for periodic in Periodic::ALL {
            let schedule = periodic.schedule(&sources);

            assert_eq!(schedule.period(), periodic.period(&sources));
            assert_eq!(schedule.warmup(), periodic.warmup());
            assert_eq!(schedule.catchup(), periodic.catchup());
        }
    }

    #[test]
    fn no_sweep_is_fast_enough_to_be_dispatch() {
        let sources = sources();

        for periodic in Periodic::ALL {
            assert!(
                periodic.period(&sources) >= Duration::from_secs(60),
                "{} sweeps more than once a minute, which is dispatch rather than \
                 housekeeping and belongs on its own loop",
                periodic.name()
            );
        }
    }
}
