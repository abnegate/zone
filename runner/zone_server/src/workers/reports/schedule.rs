//! When a digest is owed, and what it should cover.
//!
//! The obvious way to schedule a digest is to tick every hour and send when the
//! clock reads the configured hour. It breaks the moment the process is down at
//! that hour: the slot passes, nothing notices, and the period is lost.
//!
//! So nothing here compares the clock to an hour. [`due`] compares the last
//! delivery to the most recent slot that has passed, which means a process that
//! was down for three days notices on the way back up. It also means a run of
//! missed slots produces exactly one digest, covering everything since the last
//! delivery — a queue of three catch-up digests is noise, and dropping two days
//! of history to keep the window tidy is worse. The number of slots that went by
//! is reported so the digest can say why it covers more than usual.

use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

use crate::workers::analytics::{TimePeriod, TimeWindow, last_weekday_before};

const DEFAULT_HOUR: u32 = 8;
const DEFAULT_DAY_OF_MONTH: u32 = 1;

/// How often a digest goes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    Daily,
    Weekly,
    Monthly,
}

impl Cadence {
    pub const ALL: [Cadence; 3] = [Cadence::Daily, Cadence::Weekly, Cadence::Monthly];

    pub fn as_str(self) -> &'static str {
        match self {
            Cadence::Daily => "daily",
            Cadence::Weekly => "weekly",
            Cadence::Monthly => "monthly",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let normalized = text.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|cadence| cadence.as_str() == normalized)
    }

    /// The window a digest on this cadence describes.
    pub fn period(self) -> TimePeriod {
        match self {
            Cadence::Daily => TimePeriod::Day,
            Cadence::Weekly => TimePeriod::Week,
            Cadence::Monthly => TimePeriod::Month,
        }
    }
}

impl std::fmt::Display for Cadence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The moments a digest is owed, in UTC.
///
/// `weekday` is only read on a weekly cadence and `day_of_month` only on a
/// monthly one, so one value describes every cadence and switching between them
/// needs no other change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub cadence: Cadence,
    pub hour: u32,
    pub weekday: Weekday,
    pub day_of_month: u32,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            cadence: Cadence::Weekly,
            hour: DEFAULT_HOUR,
            weekday: Weekday::Mon,
            day_of_month: DEFAULT_DAY_OF_MONTH,
        }
    }
}

impl Schedule {
    /// The most recent slot at or before `moment`.
    pub fn slot_at_or_before(&self, moment: NaiveDateTime) -> NaiveDateTime {
        let candidate = match self.cadence {
            Cadence::Daily => at_hour(moment.date(), self.hour),
            Cadence::Weekly => at_hour(last_weekday_before(moment, self.weekday).date(), self.hour),
            Cadence::Monthly => monthly_slot(moment.date().year(), moment.date().month(), self),
        };

        if candidate <= moment {
            candidate
        } else {
            self.previous_slot(candidate)
        }
    }

    /// The slot one cadence before `slot`.
    pub fn previous_slot(&self, slot: NaiveDateTime) -> NaiveDateTime {
        match self.cadence {
            Cadence::Daily => slot - Duration::days(1),
            Cadence::Weekly => slot - Duration::days(7),
            Cadence::Monthly => {
                let first = slot.date().with_day(1).unwrap_or(slot.date());
                let shifted = first.checked_sub_months(Months::new(1)).unwrap_or(first);
                monthly_slot(shifted.year(), shifted.month(), self)
            }
        }
    }

    /// The next slot strictly after `moment`.
    pub fn next_after(&self, moment: NaiveDateTime) -> NaiveDateTime {
        let latest = self.slot_at_or_before(moment);
        match self.cadence {
            Cadence::Daily => latest + Duration::days(1),
            Cadence::Weekly => latest + Duration::days(7),
            Cadence::Monthly => {
                let first = latest.date().with_day(1).unwrap_or(latest.date());
                let shifted = first.checked_add_months(Months::new(1)).unwrap_or(first);
                monthly_slot(shifted.year(), shifted.month(), self)
            }
        }
    }

    /// Slots that went by strictly between two moments.
    pub fn slots_between(&self, after: NaiveDateTime, before: NaiveDateTime) -> usize {
        let mut count = 0usize;
        let mut cursor = self.previous_slot(before);
        while cursor > after {
            count += 1;
            cursor = self.previous_slot(cursor);
        }
        count
    }
}

fn at_hour(date: NaiveDate, hour: u32) -> NaiveDateTime {
    date.and_time(NaiveTime::MIN) + Duration::hours(i64::from(hour.min(23)))
}

/// The slot in one calendar month, clamped to the last day the month has.
///
/// A schedule set to the 31st is not skipped in February; it lands on the 28th,
/// or the 29th in a leap year.
fn monthly_slot(year: i32, month: u32, schedule: &Schedule) -> NaiveDateTime {
    let day = schedule.day_of_month.clamp(1, days_in_month(year, month));
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default());
    at_hour(date, schedule.hour)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|first| first.pred_opt())
        .map(|last| last.day())
        .unwrap_or(28)
}

/// A digest that is owed, and everything it should cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    /// The slot this digest is for.
    pub slot: NaiveDateTime,
    /// Everything since the last delivery, so a missed slot loses nothing.
    pub window: TimeWindow,
    /// Slots that passed undelivered and were folded into this one.
    pub missed: usize,
}

/// Whether a digest is owed, and what it covers.
///
/// With no previous delivery the window is one cadence back from the slot, which
/// is what a first digest should say. Callers holding a ledger across restarts
/// should seed it with [`Schedule::slot_at_or_before`] instead, so a restart does
/// not send a digest the previous process already sent.
pub fn due(
    schedule: &Schedule,
    last_delivered: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Option<Due> {
    let slot = schedule.slot_at_or_before(now);

    match last_delivered {
        Some(last) if last >= slot => None,
        Some(last) => Some(Due {
            slot,
            window: TimeWindow::new(last, now),
            missed: schedule.slots_between(last, slot),
        }),
        None => Some(Due {
            slot,
            window: TimeWindow::new(schedule.previous_slot(slot), now),
            missed: 0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moment(year: i32, month: u32, day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, 0, 0)
            .expect("valid time")
    }

    fn daily() -> Schedule {
        Schedule {
            cadence: Cadence::Daily,
            hour: 8,
            ..Schedule::default()
        }
    }

    fn weekly() -> Schedule {
        Schedule {
            cadence: Cadence::Weekly,
            hour: 8,
            weekday: Weekday::Mon,
            ..Schedule::default()
        }
    }

    fn monthly(day_of_month: u32) -> Schedule {
        Schedule {
            cadence: Cadence::Monthly,
            hour: 8,
            day_of_month,
            ..Schedule::default()
        }
    }

    #[test]
    fn cadences_round_trip_through_their_names() {
        for cadence in Cadence::ALL {
            assert_eq!(Cadence::parse(cadence.as_str()), Some(cadence));
        }
        assert_eq!(Cadence::parse("  WEEKLY "), Some(Cadence::Weekly));
        assert_eq!(Cadence::parse("hourly"), None);
    }

    #[test]
    fn a_cadence_names_the_window_it_describes() {
        assert_eq!(Cadence::Daily.period(), TimePeriod::Day);
        assert_eq!(Cadence::Weekly.period(), TimePeriod::Week);
        assert_eq!(Cadence::Monthly.period(), TimePeriod::Month);
    }

    #[test]
    fn the_daily_slot_is_todays_hour_until_it_arrives() {
        assert_eq!(
            daily().slot_at_or_before(moment(2026, 9, 8, 7)),
            moment(2026, 9, 7, 8),
            "before today's hour, the last slot was yesterday"
        );
        assert_eq!(
            daily().slot_at_or_before(moment(2026, 9, 8, 8)),
            moment(2026, 9, 8, 8),
            "the hour itself counts"
        );
        assert_eq!(
            daily().slot_at_or_before(moment(2026, 9, 8, 23)),
            moment(2026, 9, 8, 8)
        );
    }

    #[test]
    fn the_weekly_slot_walks_back_to_its_weekday() {
        // 2026-09-07 is a Monday.
        assert_eq!(
            weekly().slot_at_or_before(moment(2026, 9, 9, 15)),
            moment(2026, 9, 7, 8)
        );
        assert_eq!(
            weekly().slot_at_or_before(moment(2026, 9, 7, 7)),
            moment(2026, 8, 31, 8),
            "an hour before Monday's slot, the last one was the Monday before"
        );
    }

    #[test]
    fn a_monthly_slot_on_the_thirty_first_lands_on_the_last_day_february_has() {
        assert_eq!(
            monthly(31).slot_at_or_before(moment(2026, 2, 28, 12)),
            moment(2026, 2, 28, 8),
            "February 2026 has 28 days"
        );
        assert_eq!(
            monthly(31).slot_at_or_before(moment(2028, 2, 29, 12)),
            moment(2028, 2, 29, 8),
            "2028 is a leap year"
        );
    }

    #[test]
    fn stepping_back_a_month_from_a_clamped_slot_stays_clamped() {
        assert_eq!(
            monthly(31).previous_slot(moment(2026, 3, 31, 8)),
            moment(2026, 2, 28, 8)
        );
        assert_eq!(
            monthly(31).previous_slot(moment(2026, 1, 31, 8)),
            moment(2025, 12, 31, 8),
            "and across a year boundary"
        );
    }

    #[test]
    fn the_next_slot_is_always_after_the_moment_it_is_asked_about() {
        for schedule in [daily(), weekly(), monthly(1), monthly(31)] {
            for moment in [
                moment(2026, 1, 1, 0),
                moment(2026, 2, 28, 8),
                moment(2026, 9, 7, 8),
                moment(2026, 12, 31, 23),
            ] {
                let next = schedule.next_after(moment);
                assert!(next > moment, "{schedule:?} at {moment} gave {next}");
                assert!(
                    schedule.previous_slot(next) <= moment,
                    "{schedule:?} skipped a slot between {moment} and {next}"
                );
            }
        }
    }

    #[test]
    fn a_digest_already_delivered_for_this_slot_is_not_owed_again() {
        let now = moment(2026, 9, 8, 12);

        assert_eq!(due(&daily(), Some(moment(2026, 9, 8, 8)), now), None);
        assert_eq!(due(&daily(), Some(moment(2026, 9, 8, 11)), now), None);
    }

    #[test]
    fn a_first_digest_covers_one_cadence() {
        let owed = due(&weekly(), None, moment(2026, 9, 9, 12)).expect("owed");

        assert_eq!(owed.slot, moment(2026, 9, 7, 8));
        assert_eq!(owed.window.start, moment(2026, 8, 31, 8));
        assert_eq!(owed.window.end, moment(2026, 9, 9, 12));
        assert_eq!(owed.missed, 0);
    }

    #[test]
    fn a_missed_slot_is_folded_into_one_digest_that_loses_nothing() {
        let last = moment(2026, 9, 4, 8);
        let now = moment(2026, 9, 8, 9);

        let owed = due(&daily(), Some(last), now).expect("owed");

        assert_eq!(owed.slot, moment(2026, 9, 8, 8));
        assert_eq!(
            owed.window.start, last,
            "the window reaches back to the last delivery, not to one cadence"
        );
        assert_eq!(owed.window.end, now);
        assert_eq!(
            owed.missed, 3,
            "the 5th, 6th and 7th went by undelivered and are folded in"
        );
    }

    #[test]
    fn an_on_time_delivery_reports_no_missed_slots() {
        let owed = due(&daily(), Some(moment(2026, 9, 7, 8)), moment(2026, 9, 8, 8)).expect("owed");

        assert_eq!(owed.missed, 0);
        assert_eq!(owed.window.start, moment(2026, 9, 7, 8));
    }

    #[test]
    fn a_month_of_downtime_is_still_one_weekly_digest() {
        let owed = due(
            &weekly(),
            Some(moment(2026, 8, 3, 8)),
            moment(2026, 9, 7, 9),
        )
        .expect("owed");

        assert_eq!(owed.missed, 4);
        assert_eq!(owed.window.start, moment(2026, 8, 3, 8));
        assert_eq!(owed.window.duration().num_days(), 35);
    }

    #[test]
    fn slots_between_counts_only_what_passed_undelivered() {
        assert_eq!(
            daily().slots_between(moment(2026, 9, 7, 8), moment(2026, 9, 8, 8)),
            0,
            "consecutive slots have nothing between them"
        );
        assert_eq!(
            daily().slots_between(moment(2026, 9, 1, 8), moment(2026, 9, 8, 8)),
            6
        );
    }

    #[test]
    fn an_hour_outside_the_clock_is_clamped_rather_than_rejected() {
        let schedule = Schedule {
            cadence: Cadence::Daily,
            hour: 99,
            ..Schedule::default()
        };

        assert_eq!(
            schedule.slot_at_or_before(moment(2026, 9, 8, 23)),
            moment(2026, 9, 8, 23)
        );
    }
}
