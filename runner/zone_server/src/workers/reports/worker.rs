//! The pass that builds a digest and hands it to every channel.
//!
//! Delivery goes through [`zone_notify::Fanout`], which returns a report rather
//! than a result. That matters here: a workspace with a stale Discord webhook
//! and a working Slack should get its digest on Slack, and the run should be
//! recorded as delivered rather than retried into a duplicate.
//!
//! The last delivery per workspace is held in memory and seeded, on first
//! sight, with the slot that has already passed. A workspace is therefore never
//! sent a digest the previous process already sent, at the cost of losing at
//! most one slot across a restart. Sending twice is worse than sending late.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{NaiveDateTime, Utc, Weekday};
use sqlx::PgPool;
use uuid::Uuid;
use zone_notify::{Fanout, Report};

use super::digest::{Digest, generate};
use super::schedule::{Cadence, Due, Schedule, due};
use crate::db::DbResult;
use crate::db::analytics::{self, ReportableWorkspace};
use crate::state::AppState;
use crate::workers::analytics::load_runs;
use crate::workers::regression::{
    RecurrenceChecker, RegressionChecker, RegressionSettings, ReopenChecker, scan_workspace,
};

pub const REPORT_TICK_SECONDS: u64 = 15 * 60;
const MAXIMUM_RUNS_PER_WORKSPACE: i64 = 10_000;

const ENABLED_VARIABLE: &str = "ZONE_REPORT_ENABLED";
const CADENCE_VARIABLE: &str = "ZONE_REPORT_CADENCE";
const HOUR_VARIABLE: &str = "ZONE_REPORT_HOUR";
const WEEKDAY_VARIABLE: &str = "ZONE_REPORT_WEEKDAY";
const DAY_OF_MONTH_VARIABLE: &str = "ZONE_REPORT_DAY_OF_MONTH";

/// The raw environment a schedule is resolved from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportEnvironment {
    pub enabled: Option<String>,
    pub cadence: Option<String>,
    pub hour: Option<String>,
    pub weekday: Option<String>,
    pub day_of_month: Option<String>,
}

impl ReportEnvironment {
    pub fn from_process() -> Self {
        Self {
            enabled: std::env::var(ENABLED_VARIABLE).ok(),
            cadence: std::env::var(CADENCE_VARIABLE).ok(),
            hour: std::env::var(HOUR_VARIABLE).ok(),
            weekday: std::env::var(WEEKDAY_VARIABLE).ok(),
            day_of_month: std::env::var(DAY_OF_MONTH_VARIABLE).ok(),
        }
    }
}

/// When digests go out, and how much history each one reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSettings {
    pub enabled: bool,
    pub schedule: Schedule,
    pub maximum_runs_per_workspace: i64,
    pub regression: RegressionSettings,
}

impl Default for ReportSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: Schedule::default(),
            maximum_runs_per_workspace: MAXIMUM_RUNS_PER_WORKSPACE,
            regression: RegressionSettings::default(),
        }
    }
}

/// A setting the operator wrote that could not be understood.
///
/// Falling back silently means a digest arrives on a day nobody chose, and the
/// only record that the stated schedule was discarded is its absence.
fn or_default<T>(variable: &str, raw: Option<&str>, parsed: Option<T>, fallback: T) -> T {
    match (raw, parsed) {
        (Some(raw), None) => {
            tracing::warn!(
                variable,
                value = raw,
                "Value could not be read; falling back to the default"
            );
            fallback
        }
        (_, Some(value)) => value,
        (None, None) => fallback,
    }
}

impl ReportSettings {
    pub fn resolve(environment: &ReportEnvironment) -> Self {
        let defaults = Self::default();
        let cadence = or_default(
            CADENCE_VARIABLE,
            environment.cadence.as_deref(),
            environment.cadence.as_deref().and_then(Cadence::parse),
            defaults.schedule.cadence,
        );

        Self {
            enabled: environment
                .enabled
                .as_deref()
                .map(parse_flag)
                .unwrap_or(defaults.enabled),
            schedule: Schedule {
                cadence,
                hour: or_default(
                    HOUR_VARIABLE,
                    environment.hour.as_deref(),
                    environment
                        .hour
                        .as_deref()
                        .and_then(|value| value.trim().parse::<u32>().ok())
                        .filter(|hour| *hour < 24),
                    defaults.schedule.hour,
                ),
                weekday: or_default(
                    WEEKDAY_VARIABLE,
                    environment.weekday.as_deref(),
                    environment
                        .weekday
                        .as_deref()
                        .and_then(|value| Weekday::from_str(value.trim()).ok()),
                    defaults.schedule.weekday,
                ),
                day_of_month: or_default(
                    DAY_OF_MONTH_VARIABLE,
                    environment.day_of_month.as_deref(),
                    environment
                        .day_of_month
                        .as_deref()
                        .and_then(|value| value.trim().parse::<u32>().ok())
                        .filter(|day| (1..=31).contains(day)),
                    defaults.schedule.day_of_month,
                ),
            },
            ..defaults
        }
    }

    pub fn from_process_environment() -> Self {
        Self::resolve(&ReportEnvironment::from_process())
    }
}

fn parse_flag(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The last delivery per workspace, held across cycles.
pub type Ledger = BTreeMap<Uuid, NaiveDateTime>;

/// What a workspace is owed now, seeding an unseen workspace with the slot that
/// has already gone by so a restart does not resend it.
pub fn owed(
    ledger: &mut Ledger,
    workspace_id: Uuid,
    schedule: &Schedule,
    now: NaiveDateTime,
) -> Option<Due> {
    let last = *ledger
        .entry(workspace_id)
        .or_insert_with(|| schedule.slot_at_or_before(now));
    due(schedule, Some(last), now)
}

/// Build one workspace's digest for the window it is owed.
pub async fn build(
    pool: &PgPool,
    workspace: &ReportableWorkspace,
    settings: &ReportSettings,
    checkers: &[Arc<dyn RegressionChecker>],
    owed: &Due,
    now: NaiveDateTime,
) -> DbResult<Digest> {
    let runs = load_runs(
        pool,
        workspace.workspace_id,
        owed.window,
        settings.maximum_runs_per_workspace,
    )
    .await?;

    let regressions = scan_workspace(
        pool,
        workspace.workspace_id,
        checkers,
        &settings.regression,
        now,
    )
    .await?;

    Ok(generate(
        workspace.name.clone(),
        settings.schedule.cadence.period(),
        owed.window,
        &runs,
        regressions,
        owed.missed,
        now,
    ))
}

/// Hand one digest to every channel, and count what each one did.
///
/// Returns whether anyone at all received it. A fan-out with nothing registered
/// counts as delivered: a workspace that has configured no channels has not
/// failed, and retrying forever would only build a backlog nobody reads.
pub async fn deliver(fanout: &Fanout, digest: &Digest) -> Report {
    let report = fanout.deliver(&digest.to_notification()).await;

    for delivery in report.deliveries() {
        crate::metrics::record_report_delivery(
            delivery.channel().as_str(),
            if delivery.is_delivered() {
                "delivered"
            } else {
                "failed"
            },
        );
    }

    report
}

/// Whether the digest reached anyone, or had nobody to reach.
fn delivered(report: &Report) -> bool {
    report.is_empty() || report.any_delivered()
}

/// Whether another tick could plausibly deliver what this one did not.
///
/// A revoked webhook answers 404 forever. Holding the slot back for it re-runs
/// the two heaviest analytics queries every tick against a window that grows,
/// because its start is frozen at the slot that never advanced.
fn worth_retrying(report: &Report) -> bool {
    report.retryable().next().is_some()
}

/// Look once at every workspace's schedule, and deliver whatever is owed.
pub async fn run_cycle(
    state: &AppState,
    settings: &ReportSettings,
    checkers: &[Arc<dyn RegressionChecker>],
    fanout: &Fanout,
    ledger: &mut Ledger,
) -> DbResult<()> {
    let pool = state.db();
    let now = Utc::now().naive_utc();
    let period = settings.schedule.cadence.period();
    let since = now - chrono::Duration::days(period.days());

    for workspace in analytics::reportable_workspaces(pool, since).await? {
        let Some(due) = owed(ledger, workspace.workspace_id, &settings.schedule, now) else {
            continue;
        };

        let digest = match build(pool, &workspace, settings, checkers, &due, now).await {
            Ok(digest) => digest,
            Err(error) => {
                tracing::warn!(
                    workspace_id = %workspace.workspace_id,
                    %error,
                    "Digest could not be built; retrying next tick"
                );
                continue;
            }
        };

        let report = deliver(fanout, &digest).await;
        let delivered = delivered(&report);
        if !delivered && !worth_retrying(&report) {
            ledger.insert(workspace.workspace_id, due.slot);
            tracing::error!(
                workspace_id = %workspace.workspace_id,
                slot = %due.slot,
                "Every channel refused this report permanently; the slot is closed rather than \
                 retried, so this digest is lost. Check the workspace's configured channels."
            );
            continue;
        }
        if delivered {
            ledger.insert(workspace.workspace_id, due.slot);
            tracing::info!(
                workspace_id = %workspace.workspace_id,
                slot = %due.slot,
                missed = due.missed,
                next = %settings.schedule.next_after(now),
                "Scheduled report delivered"
            );
        } else {
            tracing::warn!(
                workspace_id = %workspace.workspace_id,
                slot = %due.slot,
                "No channel took the scheduled report; retrying next tick"
            );
        }
    }

    Ok(())
}

/// The checkers a digest's regression section runs.
pub fn checkers(settings: &ReportSettings) -> Vec<Arc<dyn RegressionChecker>> {
    vec![
        Arc::new(RecurrenceChecker::new(settings.regression.policy)),
        Arc::new(ReopenChecker::new(settings.regression.policy)),
    ]
}

/// Say why reports will not go out, if they will not.
pub fn announce(settings: &ReportSettings, fanout: &Fanout) {
    if !settings.enabled {
        tracing::info!("Scheduled reports are off; set {ENABLED_VARIABLE}=true to turn them on");
    } else if fanout.is_empty() {
        tracing::warn!("Scheduled reports are on but no notification channel is configured");
    }
}

#[cfg(test)]
mod tests {
    use super::super::digest::fixtures::{digest, moment};
    use super::*;
    use std::sync::Mutex;
    use zone_notify::{Channel, Notification, Notifier, NotifyError};

    fn environment(cadence: &str, hour: &str) -> ReportEnvironment {
        ReportEnvironment {
            enabled: Some("true".to_string()),
            cadence: Some(cadence.to_string()),
            hour: Some(hour.to_string()),
            ..ReportEnvironment::default()
        }
    }

    #[test]
    fn reports_are_off_until_they_are_turned_on() {
        let settings = ReportSettings::resolve(&ReportEnvironment::default());

        assert!(!settings.enabled);
        assert_eq!(settings.schedule, Schedule::default());
    }

    #[test]
    fn a_configured_schedule_is_read_out_of_the_environment() {
        let settings = ReportSettings::resolve(&ReportEnvironment {
            weekday: Some("friday".to_string()),
            ..environment("weekly", "17")
        });

        assert!(settings.enabled);
        assert_eq!(settings.schedule.cadence, Cadence::Weekly);
        assert_eq!(settings.schedule.hour, 17);
        assert_eq!(settings.schedule.weekday, Weekday::Fri);
    }

    #[test]
    fn nonsense_configuration_falls_back_rather_than_failing_to_start() {
        let settings = ReportSettings::resolve(&ReportEnvironment {
            weekday: Some("caturday".to_string()),
            day_of_month: Some("99".to_string()),
            ..environment("fortnightly", "25")
        });

        assert_eq!(settings.schedule.cadence, Schedule::default().cadence);
        assert_eq!(settings.schedule.hour, Schedule::default().hour);
        assert_eq!(settings.schedule.weekday, Schedule::default().weekday);
        assert_eq!(
            settings.schedule.day_of_month,
            Schedule::default().day_of_month
        );
    }

    #[test]
    fn a_workspace_seen_for_the_first_time_waits_for_the_next_slot() {
        let schedule = Schedule {
            cadence: Cadence::Daily,
            hour: 8,
            ..Schedule::default()
        };
        let mut ledger = Ledger::new();
        let workspace_id = Uuid::from_u128(1);

        assert_eq!(
            owed(&mut ledger, workspace_id, &schedule, moment(8, 12)),
            None,
            "the slot that already passed is assumed covered"
        );
        assert_eq!(ledger.get(&workspace_id), Some(&moment(8, 8)));

        let next = owed(&mut ledger, workspace_id, &schedule, moment(9, 9))
            .expect("the next slot is owed");
        assert_eq!(next.slot, moment(9, 8));
        assert_eq!(next.missed, 0);
    }

    #[test]
    fn recording_a_delivery_stops_the_same_slot_going_out_twice() {
        let schedule = Schedule {
            cadence: Cadence::Daily,
            hour: 8,
            ..Schedule::default()
        };
        let workspace_id = Uuid::from_u128(1);
        let mut ledger = Ledger::from([(workspace_id, moment(7, 8))]);

        let due = owed(&mut ledger, workspace_id, &schedule, moment(8, 9)).expect("owed");
        ledger.insert(workspace_id, due.slot);

        assert_eq!(
            owed(&mut ledger, workspace_id, &schedule, moment(8, 23)),
            None
        );
    }

    struct Recorder {
        channel: Channel,
        failure: Option<NotifyError>,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl Recorder {
        fn working(channel: Channel, seen: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                channel,
                failure: None,
                seen,
            }
        }

        fn broken(channel: Channel, seen: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                channel,
                failure: Some(NotifyError::Rejected {
                    host: "hooks.zone.test".to_string(),
                    status: 404,
                    body: "Unknown Webhook".to_string(),
                }),
                seen,
            }
        }
    }

    #[async_trait::async_trait]
    impl Notifier for Recorder {
        fn channel(&self) -> Channel {
            self.channel.clone()
        }

        async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError> {
            match &self.failure {
                Some(error) => Err(error.clone()),
                None => {
                    self.seen
                        .lock()
                        .expect("no panic held the lock")
                        .push(notification.title().to_string());
                    Ok(())
                }
            }
        }
    }

    #[tokio::test]
    async fn a_digest_reaches_every_working_channel() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let fanout = Fanout::new()
            .with(Recorder::working(Channel::SLACK, Arc::clone(&seen)))
            .with(Recorder::working(Channel::EMAIL, Arc::clone(&seen)));

        assert!(delivered(&deliver(&fanout, &digest(Vec::new(), 0)).await));
        assert_eq!(
            seen.lock().expect("no panic held the lock").len(),
            2,
            "both channels saw the digest"
        );
    }

    #[tokio::test]
    async fn a_broken_channel_does_not_stop_the_others() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let fanout = Fanout::new()
            .with(Recorder::broken(Channel::DISCORD, Arc::clone(&seen)))
            .with(Recorder::working(Channel::SLACK, Arc::clone(&seen)))
            .with(Recorder::working(Channel::EMAIL, Arc::clone(&seen)));

        assert!(
            delivered(&deliver(&fanout, &digest(Vec::new(), 0)).await),
            "one dead webhook is not a failed report"
        );
        assert_eq!(
            *seen.lock().expect("no panic held the lock"),
            vec![
                "Platform week report: 7 of 12 runs succeeded".to_string(),
                "Platform week report: 7 of 12 runs succeeded".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn a_permanent_refusal_closes_the_slot_rather_than_retrying_forever() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let fanout = Fanout::new().with(Recorder::broken(Channel::DISCORD, seen));
        let report = deliver(&fanout, &digest(Vec::new(), 0)).await;

        assert!(!delivered(&report), "a 404 did not deliver anything");
        assert!(
            !worth_retrying(&report),
            "a revoked webhook answers 404 every tick, so holding the slot back \
             re-runs the heaviest queries against a window that only grows"
        );
    }

    #[tokio::test]
    async fn a_report_nobody_took_is_not_marked_delivered() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let fanout = Fanout::new().with(Recorder::broken(Channel::DISCORD, seen));

        assert!(!delivered(&deliver(&fanout, &digest(Vec::new(), 0)).await));
    }

    #[tokio::test]
    async fn a_workspace_with_no_channels_is_not_a_failed_delivery() {
        assert!(
            delivered(&deliver(&Fanout::new(), &digest(Vec::new(), 0)).await),
            "nowhere to send is not the same as failing to send"
        );
    }
}
