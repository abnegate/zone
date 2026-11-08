//! Zone's limits on a recurring automation.
//!
//! The rule and its arithmetic are `abnegate_recurrence::Recurrence`; this file
//! holds the hourly floor, the lifetime and the per-automation offset.

pub use abnegate_recurrence::Recurrence;
use chrono::Duration;
use uuid::Uuid;

/// The shortest period an automation may repeat on.
///
/// Hourly is the ceiling, not a default: a schedule is a standing claim on
/// somebody's attention and on a model's budget, and anything faster is a poll
/// wearing a schedule's clothes. A condition that changes faster than this
/// wants an event, which is what `wait_for` is for.
pub const MIN_PERIOD: Duration = Duration::hours(1);

/// Reads an `RRULE` line as `abnegate_recurrence` does, and refuses one that
/// fires faster than `MIN_PERIOD`.
///
/// The refusal names the gap it measured, because a rule that trips the floor
/// is usually one whose clauses multiplied out further than whoever wrote it
/// expected, and "too often" alone does not say by how much.
pub fn parse(rule: &str) -> Result<Recurrence, String> {
    #[expect(clippy::disallowed_methods)]
    let recurrence = Recurrence::parse(rule).map_err(|error| error.to_string())?;
    let shortest = recurrence.period();
    if shortest < MIN_PERIOD {
        return Err(format!(
            "Two of those firings are {} minutes apart, and once an hour is the ceiling. Ask \
             for a slower schedule, or wait on the event itself if you need to know the \
             moment it changes.",
            shortest.num_minutes().max(0)
        ));
    }
    Ok(recurrence)
}

/// How long a recurring automation lives before it has to be asked for again.
///
/// A schedule nobody renews is a schedule nobody wanted: the person who asked
/// for a daily digest in March is not asking for it in December, and an
/// automation that outlives its reason is noise the owner has to go and find.
/// Seven days is short enough that a forgotten one stops on its own.
pub const MAX_LIFETIME: Duration = Duration::days(7);

/// The longest a firing is ever nudged, whatever the period.
const MAX_JITTER: Duration = Duration::minutes(15);

/// How far off its nominal minute one automation fires.
///
/// Everyone who asks for "every morning" is given nine o'clock, and every
/// hourly schedule lands on the hour, so a fleet of them arrives at the model
/// in one burst and queues behind itself. The offset spreads that burst out. It
/// is derived from the automation's own id rather than drawn at random, so a
/// firing is at the same offset every time and a reader watching two
/// consecutive runs sees a schedule rather than a wobble.
///
/// Up to a tenth of the period, capped at `MAX_JITTER`: proportional so an
/// hourly job moves by minutes and a weekly one does not move by days, and
/// capped so no automation drifts far enough from the time the person named to
/// read as a different time.
pub fn jitter(id: Uuid, period: Duration) -> Duration {
    let span = (period / 10).min(MAX_JITTER).num_seconds().max(0);
    if span == 0 {
        return Duration::zero();
    }
    let mut mixed = 0u64;
    for byte in id.as_bytes() {
        // FNV-1a. Any stable mixing would do; what matters is that the same id
        // gives the same offset on every process that computes it.
        mixed = (mixed ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let offset = i64::try_from(mixed % u64::try_from(span).unwrap_or(1)).unwrap_or(0);
    Duration::seconds(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::time_zone::{self, TimeZone};
    use chrono::{DateTime, Datelike, NaiveDate, Utc};

    /// The same automation always fires at the same offset, and two do not
    /// share one, which is the whole point of spreading them.
    #[test]
    fn one_automation_keeps_its_offset_and_two_do_not_share_it() {
        let period = Duration::hours(1);
        let one = Uuid::from_u128(1);
        let two = Uuid::from_u128(2);

        assert_eq!(jitter(one, period), jitter(one, period));
        assert_ne!(jitter(one, period), jitter(two, period));
    }

    /// Proportional, and capped, so an hourly job moves by minutes and a weekly
    /// one does not move by a day.
    #[test]
    fn an_offset_is_a_tenth_of_the_period_and_never_more_than_the_cap() {
        for id in 0..64u128 {
            let id = Uuid::from_u128(id);
            let hourly = jitter(id, Duration::hours(1));
            assert!(
                hourly >= Duration::zero() && hourly < Duration::minutes(6),
                "hourly moved {hourly}"
            );

            let weekly = jitter(id, Duration::weeks(1));
            assert!(
                weekly >= Duration::zero() && weekly <= MAX_JITTER,
                "weekly moved {weekly} past the cap"
            );
        }
    }

    /// A period too short to split leaves the firing where it is rather than
    /// dividing by zero.
    #[test]
    fn a_period_with_no_room_to_move_is_left_alone() {
        assert_eq!(
            jitter(Uuid::from_u128(7), Duration::seconds(5)),
            Duration::zero()
        );
        assert_eq!(
            jitter(Uuid::from_u128(7), Duration::zero()),
            Duration::zero()
        );
    }

    fn at_utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("the fixture is a timestamp")
            .with_timezone(&Utc)
    }

    /// Hourly is the ceiling, counted at the shortest gap a rule's clauses
    /// produce rather than at their average.
    ///
    /// The distinction is the whole point: a rule that fires twice in an hour
    /// and then not again until the next day averages out well above the
    /// floor, and an average is no comfort to whoever the two firings half an
    /// hour apart interrupt.
    #[test]
    fn anything_faster_than_an_hour_is_refused_however_it_is_spelled() {
        // Every spelling here uses only clauses its frequency reads, so the
        // floor is what refuses it rather than the clause check ahead of it.
        for (rule, gap) in [
            (
                "FREQ=DAILY;BYHOUR=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;BYMINUTE=0,30",
                "30",
            ),
            (
                "FREQ=WEEKLY;BYHOUR=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;BYMINUTE=0,5,10,15,20,25,30,35",
                "5",
            ),
            // Four firings a day, which averages six hours apart and is three
            // half-hours and one long wait.
            ("FREQ=DAILY;BYHOUR=0,1;BYMINUTE=0,30", "30"),
            // Twice a day, averaging twelve hours, half an hour apart.
            ("FREQ=DAILY;BYMINUTE=0,30", "30"),
            // Two dates a week, and the shortest gap is the one across
            // midnight between them -- Monday 23:45 to Tuesday 00:00 --
            // rather than either of the forty-five-minute ones inside a day.
            ("FREQ=WEEKLY;BYDAY=MO,TU;BYHOUR=0,23;BYMINUTE=0,45", "15"),
            // The same shape a month apart: the 1st at 23:45 to the 2nd at
            // 00:00.
            (
                "FREQ=MONTHLY;BYMONTHDAY=1,2;BYHOUR=0,23;BYMINUTE=0,45",
                "15",
            ),
        ] {
            let refused = parse(rule).expect_err(rule);
            assert!(refused.contains("once an hour"), "{rule}: {refused}");
            assert!(
                refused.contains(&format!("{gap} minutes apart")),
                "{rule} should name its own shortest gap: {refused}"
            );
        }

        for rule in [
            "FREQ=HOURLY",
            "FREQ=HOURLY;INTERVAL=4",
            "FREQ=DAILY;BYHOUR=9,17",
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9",
            // Exactly the floor, from the far end of one day into the near
            // end of the next. At the ceiling is not over it.
            "FREQ=DAILY;BYHOUR=0,23",
            "FREQ=WEEKLY;BYDAY=MO,TU;BYHOUR=0,23",
            "FREQ=MONTHLY;BYMONTHDAY=1,2;BYHOUR=0,23",
        ] {
            assert!(parse(rule).is_ok(), "{rule} is at or under the ceiling");
        }

        assert_eq!(
            parse("FREQ=DAILY;BYMINUTE=0,30"),
            Err(
                "Two of those firings are 30 minutes apart, and once an hour is the ceiling. Ask \
                 for a slower schedule, or wait on the event itself if you need to know the \
                 moment it changes."
                    .to_string()
            )
        );
    }

    /// A monthly step past the calendar has no next period, rather than
    /// overflowing the month count, and the anchor still fires.
    #[test]
    fn a_monthly_interval_past_the_calendar_fires_once_and_stops() {
        let anchor = at_utc("2027-01-15T09:00:00Z");
        for rule in [
            "FREQ=MONTHLY;INTERVAL=2147483647",
            "FREQ=MONTHLY;INTERVAL=4294967295",
        ] {
            let parsed = parse(rule).expect(rule);
            assert!(
                parsed.fires_at(anchor, TimeZone::UTC),
                "{rule} fires at its anchor"
            );
            assert_eq!(
                parsed.next_after(anchor, anchor, 1, TimeZone::UTC),
                None,
                "{rule} has no second period inside the calendar"
            );
        }
    }

    /// The evening of the calendar's last day in New York is past the last
    /// instant UTC can hold, so the period that lands on it has no firing
    /// rather than stepping off the end of the calendar.
    #[test]
    fn a_daily_period_on_the_calendars_last_evening_in_new_york_is_skipped() {
        let new_york = time_zone::parse("America/New_York").unwrap();
        let anchor = at_utc("2027-01-16T04:00:00Z");
        let days = (NaiveDate::MAX - NaiveDate::from_ymd_opt(2027, 1, 15).unwrap()).num_days();
        let rule = parse(&format!("FREQ=DAILY;INTERVAL={days}")).expect("accepted");

        assert!(rule.fires_at(anchor, new_york));
        assert_eq!(rule.next_after(anchor, anchor, 1, new_york), None);
    }

    /// A weekly period in the calendar's last week keeps the days inside it
    /// and skips the ones past its end.
    #[test]
    fn a_weekly_period_at_the_end_of_the_calendar_skips_the_days_past_it() {
        let monday = at_utc("2027-01-04T09:00:00Z");
        let sunday = at_utc("2027-01-10T09:00:00Z");
        let weeks = (NaiveDate::MAX - monday.date_naive()).num_weeks();
        let last = monday + Duration::weeks(weeks);
        let rule = parse(&format!("FREQ=WEEKLY;INTERVAL={weeks};BYDAY=MO,SU")).expect("accepted");

        assert_eq!(
            rule.next_after(monday, sunday, 2, TimeZone::UTC),
            Some(last)
        );
        assert_eq!(rule.next_after(monday, last, 3, TimeZone::UTC), None);
    }

    /// A weekly period that reaches back into the calendar's first week keeps
    /// the days inside it, so an ordinary anchor still fires.
    #[test]
    fn a_weekly_period_at_the_start_of_the_calendar_keeps_the_days_inside_it() {
        for day in 0..7 {
            let anchor = at_utc("2027-01-04T09:00:00Z") + Duration::days(day);
            let weeks = (anchor.date_naive() - NaiveDate::MIN).num_weeks();
            for interval in [weeks - 1, weeks, weeks + 1] {
                let rule = format!("FREQ=WEEKLY;INTERVAL={interval};BYDAY=MO,TU,WE,TH,FR,SA,SU");
                let parsed = parse(&rule).expect(&rule);
                assert!(
                    parsed.fires_at(anchor, TimeZone::UTC),
                    "{rule} fires at its anchor {anchor}"
                );
            }
        }
    }

    /// An `INTERVAL` too large for an `i32` is counted in full rather than cut
    /// to the largest one, so the period and the next firing are the ones the
    /// rule names.
    #[test]
    fn an_interval_past_the_largest_i32_counts_in_full() {
        let anchor = at_utc("2027-01-15T09:00:00Z");
        let rule = parse("FREQ=HOURLY;INTERVAL=2147483648").expect("accepted");
        assert_eq!(rule.period(), Duration::hours(2_147_483_648));
        assert_eq!(
            rule.next_after(anchor, anchor, 1, TimeZone::UTC),
            Some(anchor + Duration::hours(2_147_483_648))
        );

        let beyond = parse("FREQ=HOURLY;INTERVAL=4294967295").expect("accepted");
        assert_eq!(beyond.next_after(anchor, anchor, 1, TimeZone::UTC), None);
    }

    /// The calendar's last December has a 31st even though no January follows
    /// it, so a day inherited from a 31 January anchor is kept.
    #[test]
    fn a_monthly_rule_reaching_the_calendars_last_december_keeps_its_31st() {
        let anchor = at_utc("2027-01-31T09:00:00Z");
        let months = i64::from(NaiveDate::MAX.year() - 2027) * 12 + 11;
        let rule = parse(&format!("FREQ=MONTHLY;INTERVAL={months}")).expect("accepted");

        assert_eq!(
            rule.next_after(anchor, anchor, 1, TimeZone::UTC),
            Some(NaiveDate::MAX.and_hms_opt(9, 0, 0).unwrap().and_utc())
        );
    }
}
