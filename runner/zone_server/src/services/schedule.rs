//! When a recurring automation fires next.
//!
//! The subset of RFC 5545 `RRULE` an automation may carry, and the arithmetic
//! that turns one into the next instant after a given moment. Pure: it takes a
//! rule and a clock reading and answers with an instant, so every rule in the
//! tool's description can be tested without a database or a worker.
//!
//! The subset is deliberate rather than partial. `FREQ`, `INTERVAL`, `BYDAY`,
//! `BYHOUR`, `BYMINUTE`, `BYMONTHDAY`, `UNTIL` and `COUNT` are what the
//! automation contract needs; anything else in a rule is refused at creation
//! rather than ignored, because a rule that silently drops a clause fires at a
//! time nobody asked for and the owner finds out by being interrupted.

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc, Weekday};
use uuid::Uuid;

/// The shortest period an automation may repeat on.
///
/// Hourly is the ceiling, not a default: a schedule is a standing claim on
/// somebody's attention and on a model's budget, and anything faster is a poll
/// wearing a schedule's clothes. A condition that changes faster than this
/// wants an event, which is what `wait_for` is for.
pub const MIN_PERIOD: Duration = Duration::hours(1);

/// How far ahead a search gives up.
///
/// A rule whose clauses cannot all be satisfied — 31 February, or a `BYDAY`
/// no month in the pattern reaches — would otherwise walk forward for ever.
/// Four years clears every leap-year case and costs a bounded loop.
const MAX_PERIODS: u32 = 1_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

impl Frequency {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "HOURLY" => Some(Self::Hourly),
            "DAILY" => Some(Self::Daily),
            "WEEKLY" => Some(Self::Weekly),
            "MONTHLY" => Some(Self::Monthly),
            _ => None,
        }
    }

    /// The rule's own period at `INTERVAL=1`, used to hold a rule to
    /// `MIN_PERIOD`. A month is measured at its shortest, because the bound
    /// has to hold for February as well as for July.
    fn shortest(self) -> Duration {
        match self {
            Self::Hourly => Duration::hours(1),
            Self::Daily => Duration::days(1),
            Self::Weekly => Duration::weeks(1),
            Self::Monthly => Duration::days(28),
        }
    }
}

/// One accepted `RRULE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    frequency: Frequency,
    interval: u32,
    by_day: Vec<Weekday>,
    by_hour: Vec<u32>,
    by_minute: Vec<u32>,
    by_month_day: Vec<u32>,
    until: Option<DateTime<Utc>>,
    count: Option<u32>,
}

impl Recurrence {
    /// Reads an `RRULE` line, with or without its `RRULE:` prefix.
    ///
    /// Every failure names the clause it read and what it wanted, because the
    /// caller is a model composing a rule from a person's sentence and a
    /// refusal it cannot act on costs the person another turn.
    pub fn parse(rule: &str) -> Result<Self, String> {
        let body = rule
            .trim()
            .strip_prefix("RRULE:")
            .unwrap_or_else(|| rule.trim());
        if body.is_empty() {
            return Err("An rrule cannot be empty. Give at least FREQ.".to_string());
        }

        let mut frequency = None;
        let mut interval = 1;
        let mut by_day = Vec::new();
        let mut by_hour = Vec::new();
        let mut by_minute = Vec::new();
        let mut by_month_day = Vec::new();
        let mut until = None;
        let mut count = None;

        for part in body.split(';').filter(|part| !part.trim().is_empty()) {
            let (name, value) = part
                .split_once('=')
                .ok_or_else(|| format!("\"{part}\" is not NAME=VALUE."))?;
            let value = value.trim();
            match name.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    frequency = Some(
                        Frequency::parse(&value.to_ascii_uppercase()).ok_or_else(|| {
                            format!(
                                "FREQ={value} is not supported. Use HOURLY, DAILY, WEEKLY or MONTHLY."
                            )
                        })?,
                    );
                }
                "INTERVAL" => {
                    interval = value
                        .parse::<u32>()
                        .ok()
                        .filter(|parsed| *parsed >= 1)
                        .ok_or_else(|| {
                            format!("INTERVAL={value} must be a positive whole number.")
                        })?;
                }
                "BYDAY" => {
                    by_day = list(value, |raw| weekday(raw).map(Day), "BYDAY")?
                        .into_iter()
                        .map(|Day(day)| day)
                        .collect();
                }
                "BYHOUR" => by_hour = list(value, |raw| bounded(raw, 0, 23), "BYHOUR")?,
                "BYMINUTE" => by_minute = list(value, |raw| bounded(raw, 0, 59), "BYMINUTE")?,
                "BYMONTHDAY" => {
                    by_month_day = list(value, |raw| bounded(raw, 1, 31), "BYMONTHDAY")?
                }
                "UNTIL" => until = Some(timestamp(value)?),
                "COUNT" => {
                    count = Some(
                        value
                            .parse::<u32>()
                            .ok()
                            .filter(|parsed| *parsed >= 1)
                            .ok_or_else(|| {
                                format!("COUNT={value} must be a positive whole number.")
                            })?,
                    );
                }
                other => {
                    return Err(format!(
                        "{other} is not a clause this build accepts. Use FREQ, INTERVAL, BYDAY, \
                         BYHOUR, BYMINUTE, BYMONTHDAY, UNTIL or COUNT."
                    ));
                }
            }
        }

        let frequency = frequency.ok_or_else(|| "An rrule must state FREQ.".to_string())?;
        if until.is_some() && count.is_some() {
            return Err("Give UNTIL or COUNT, not both.".to_string());
        }
        let recurrence = Self {
            frequency,
            interval,
            by_day,
            by_hour,
            by_minute,
            by_month_day,
            until,
            count,
        };
        recurrence.within_the_floor()?;
        Ok(recurrence)
    }

    /// The nominal gap between two firings: the frequency's own period, times
    /// the interval, divided by how many times the `BY` clauses split it.
    /// `FREQ=DAILY;BYHOUR=9,17` is twelve hours, not twenty-four.
    ///
    /// Nominal because a month is measured at its shortest and a `BYDAY` list
    /// is not counted — it is what sizes a jitter offset and what holds a rule
    /// to `MIN_PERIOD`, and both want the gap at its smallest.
    pub fn period(&self) -> Duration {
        let splits = i32::try_from(self.by_hour.len().max(1) * self.by_minute.len().max(1))
            .unwrap_or(i32::MAX);
        let whole = self.frequency.shortest() * i32::try_from(self.interval).unwrap_or(i32::MAX);
        whole / splits.max(1)
    }

    /// Refuses a rule that would fire faster than `MIN_PERIOD`.
    fn within_the_floor(&self) -> Result<(), String> {
        if self.period() < MIN_PERIOD {
            return Err(
                "That repeats more often than once an hour, which is the ceiling. Ask for \
                        a slower schedule, or wait on the event itself if you need to know the \
                        moment it changes."
                    .to_string(),
            );
        }
        Ok(())
    }

    /// The first firing strictly after `after`, or `None` once the rule is
    /// spent.
    ///
    /// `anchor` is the automation's first firing: it supplies whatever the
    /// rule does not say, so `FREQ=WEEKLY` alone repeats on the anchor's own
    /// weekday and time rather than on an arbitrary one. `fired` is how many
    /// firings have already happened, which is what `COUNT` is measured
    /// against.
    pub fn next_after(
        &self,
        anchor: DateTime<Utc>,
        after: DateTime<Utc>,
        fired: u32,
    ) -> Option<DateTime<Utc>> {
        if self.count.is_some_and(|count| fired >= count) {
            return None;
        }

        let hours = if self.by_hour.is_empty() {
            vec![anchor.hour()]
        } else {
            self.by_hour.clone()
        };
        let minutes = if self.by_minute.is_empty() {
            vec![anchor.minute()]
        } else {
            self.by_minute.clone()
        };

        let mut period = self.period_start(anchor, after);
        for _ in 0..MAX_PERIODS {
            for candidate in self.candidates(period, anchor, &hours, &minutes) {
                if candidate <= after {
                    continue;
                }
                if self.until.is_some_and(|until| candidate > until) {
                    return None;
                }
                return Some(candidate);
            }
            period = self.advance(period)?;
        }
        None
    }

    /// The start of the period `after` falls in, walked back to the anchor's
    /// own phase so an `INTERVAL` counts from the automation's first firing
    /// rather than from an arbitrary epoch.
    fn period_start(&self, anchor: DateTime<Utc>, after: DateTime<Utc>) -> DateTime<Utc> {
        let mut period = anchor;
        while period > after {
            let Some(back) = self.retreat(period) else {
                break;
            };
            period = back;
        }
        while self.advance(period).is_some_and(|next| next <= after) {
            let Some(next) = self.advance(period) else {
                break;
            };
            period = next;
        }
        period
    }

    fn advance(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.step(from, 1)
    }

    fn retreat(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.step(from, -1)
    }

    fn step(&self, from: DateTime<Utc>, direction: i32) -> Option<DateTime<Utc>> {
        let interval = i32::try_from(self.interval).unwrap_or(i32::MAX) * direction;
        match self.frequency {
            Frequency::Hourly => from.checked_add_signed(Duration::hours(i64::from(interval))),
            Frequency::Daily => from.checked_add_signed(Duration::days(i64::from(interval))),
            Frequency::Weekly => from.checked_add_signed(Duration::weeks(i64::from(interval))),
            Frequency::Monthly => months(from, interval),
        }
    }

    /// The instants one period offers, in order.
    ///
    /// An hourly period *is* its firing — the hour is what the period stepped —
    /// so it is returned whole. Every other frequency picks dates and then
    /// crosses them with the hours and minutes the rule or the anchor gives.
    fn candidates(
        &self,
        period: DateTime<Utc>,
        anchor: DateTime<Utc>,
        hours: &[u32],
        minutes: &[u32],
    ) -> Vec<DateTime<Utc>> {
        if self.frequency == Frequency::Hourly {
            return period.with_second(0).into_iter().collect();
        }
        let mut instants: Vec<DateTime<Utc>> = Vec::new();
        for day in self.days_of(period, anchor) {
            for hour in hours {
                for minute in minutes {
                    instants.extend(at(day, *hour, *minute));
                }
            }
        }
        instants.sort_unstable();
        instants
    }

    /// Which dates inside one period the rule selects, in order.
    fn days_of(&self, period: DateTime<Utc>, anchor: DateTime<Utc>) -> Vec<DateTime<Utc>> {
        match self.frequency {
            Frequency::Hourly | Frequency::Daily => vec![period],
            Frequency::Weekly => {
                let days = if self.by_day.is_empty() {
                    vec![anchor.weekday()]
                } else {
                    self.by_day.clone()
                };
                let monday =
                    period - Duration::days(i64::from(period.weekday().num_days_from_monday()));
                let mut dates: Vec<DateTime<Utc>> = days
                    .iter()
                    .map(|day| monday + Duration::days(i64::from(day.num_days_from_monday())))
                    .collect();
                dates.sort_unstable();
                dates
            }
            // A day the rule names is that date or no date: BYMONTHDAY=31 asked
            // for the 31st, and February has none, so February is skipped the
            // way RFC 5545 skips it. A day inherited from the anchor is a
            // different request — "each month, at this point in it" — so it
            // clamps to the month's last day instead, and a 31 January
            // automation lands on 28 February rather than vanishing from the
            // shortest month of the year.
            Frequency::Monthly => {
                let last = last_day(period.year(), period.month());
                let days = if self.by_month_day.is_empty() {
                    vec![anchor.day().min(last)]
                } else {
                    self.by_month_day.clone()
                };
                let mut dates: Vec<DateTime<Utc>> = days
                    .iter()
                    .filter_map(|day| {
                        Utc.with_ymd_and_hms(period.year(), period.month(), *day, 0, 0, 0)
                            .single()
                    })
                    .collect();
                dates.sort_unstable();
                dates
            }
        }
    }
}

/// An hourly rule fires at the period's own hour; every other frequency takes
/// the hour and minute from the rule or the anchor.
fn at(day: DateTime<Utc>, hour: u32, minute: u32) -> Option<DateTime<Utc>> {
    Utc.with_ymd_and_hms(day.year(), day.month(), day.day(), hour, minute, 0)
        .single()
}

/// Adds whole months, clamping a day the target month does not have — 31
/// January plus one month is the last day of February, never 3 March.
fn months(from: DateTime<Utc>, count: i32) -> Option<DateTime<Utc>> {
    let zero = from.year() * 12 + i32::try_from(from.month()).ok()? - 1 + count;
    let year = zero.div_euclid(12);
    let month = u32::try_from(zero.rem_euclid(12) + 1).ok()?;
    let last = last_day(year, month);
    Utc.with_ymd_and_hms(
        year,
        month,
        from.day().min(last),
        from.hour(),
        from.minute(),
        0,
    )
    .single()
}

fn last_day(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Utc.with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
        .single()
        .and_then(|first| first.checked_sub_signed(Duration::days(1)))
        .map_or(28, |last| last.day())
}

/// `chrono::Weekday` has no `Ord`, and the one ordering that matters here is
/// the week's own, so the clause reader borrows it for the length of a parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Day(Weekday);

impl Ord for Day {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .num_days_from_monday()
            .cmp(&other.0.num_days_from_monday())
    }
}

impl PartialOrd for Day {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn list<T, F>(value: &str, read: F, clause: &str) -> Result<Vec<T>, String>
where
    F: Fn(&str) -> Option<T>,
    T: Ord,
{
    let mut parsed = Vec::new();
    for raw in value
        .split(',')
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        parsed.push(
            read(raw).ok_or_else(|| format!("{clause}={raw} is not a value {clause} takes."))?,
        );
    }
    if parsed.is_empty() {
        return Err(format!("{clause} was given no values."));
    }
    parsed.sort_unstable();
    parsed.dedup();
    Ok(parsed)
}

fn bounded(raw: &str, low: u32, high: u32) -> Option<u32> {
    raw.parse::<u32>()
        .ok()
        .filter(|parsed| (low..=high).contains(parsed))
}

fn weekday(raw: &str) -> Option<Weekday> {
    match raw.to_ascii_uppercase().as_str() {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

/// `UNTIL` in the one form RFC 5545 gives it for a UTC rule.
fn timestamp(value: &str) -> Result<DateTime<Utc>, String> {
    // The trailing `Z` is a literal in RFC 5545's UTC form, not an offset
    // chrono can read, so the stamp parses naive and is declared UTC here.
    chrono::NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
        .map(|parsed| parsed.and_utc())
        .map_err(|_| format!("UNTIL={value} must read as 20260914T090000Z."))
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

    /// Every clause the tool's description offers, read back as the rule the
    /// description promises. A clause that parses to something else would send
    /// a person a notification at an hour they did not ask for.
    #[test]
    fn each_accepted_clause_reads_back_as_itself() {
        let rule = Recurrence::parse(
            "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;BYHOUR=9;BYMINUTE=7;COUNT=4",
        )
        .expect("the rule is accepted");

        assert_eq!(rule.frequency, Frequency::Weekly);
        assert_eq!(rule.interval, 2);
        assert_eq!(rule.by_day, vec![Weekday::Mon, Weekday::Wed]);
        assert_eq!(rule.by_hour, vec![9]);
        assert_eq!(rule.by_minute, vec![7]);
        assert_eq!(rule.count, Some(4));
        assert_eq!(rule.until, None);
    }

    /// The `RRULE:` prefix is optional, because a model copying the shape out
    /// of a calendar entry keeps it and one writing from the description does
    /// not.
    #[test]
    fn the_prefix_is_optional_and_the_rule_is_the_same_either_way() {
        assert_eq!(
            Recurrence::parse("RRULE:FREQ=DAILY;BYHOUR=9"),
            Recurrence::parse("FREQ=DAILY;BYHOUR=9")
        );
    }

    /// A clause this build does not implement is refused rather than dropped:
    /// a rule that silently loses `BYSETPOS` fires on days nobody chose.
    #[test]
    fn a_clause_this_build_does_not_implement_is_refused_and_not_ignored() {
        let refused = Recurrence::parse("FREQ=MONTHLY;BYSETPOS=-1;BYDAY=FR")
            .expect_err("an unimplemented clause is refused");
        assert!(refused.contains("BYSETPOS"), "{refused}");
        assert!(
            refused.contains("FREQ"),
            "the refusal lists what is accepted: {refused}"
        );
    }

    #[test]
    fn a_rule_with_no_frequency_or_a_bad_one_is_refused_by_name() {
        for (rule, expected) in [
            ("INTERVAL=2", "FREQ"),
            ("FREQ=SECONDLY", "HOURLY"),
            ("FREQ=DAILY;INTERVAL=0", "INTERVAL"),
            ("FREQ=DAILY;BYHOUR=24", "BYHOUR"),
            ("FREQ=DAILY;BYMINUTE=60", "BYMINUTE"),
            ("FREQ=WEEKLY;BYDAY=FUNDAY", "BYDAY"),
            ("FREQ=DAILY;UNTIL=tomorrow", "UNTIL"),
            ("", "FREQ"),
        ] {
            let refused = Recurrence::parse(rule).expect_err(rule);
            assert!(refused.contains(expected), "{rule}: {refused}");
        }
    }

    /// Hourly is the ceiling, counted after the `BY` clauses have split the
    /// period rather than before.
    #[test]
    fn anything_faster_than_an_hour_is_refused_however_it_is_spelled() {
        for rule in [
            "FREQ=HOURLY;BYMINUTE=0,30",
            "FREQ=DAILY;BYHOUR=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;BYMINUTE=0,30",
        ] {
            let refused = Recurrence::parse(rule).expect_err(rule);
            assert!(refused.contains("once an hour"), "{rule}: {refused}");
        }

        for rule in [
            "FREQ=HOURLY",
            "FREQ=HOURLY;INTERVAL=4",
            "FREQ=DAILY;BYHOUR=9,17",
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9",
        ] {
            assert!(
                Recurrence::parse(rule).is_ok(),
                "{rule} is at or under the ceiling"
            );
        }
    }

    #[test]
    fn until_and_count_cannot_both_stop_the_same_rule() {
        let refused = Recurrence::parse("FREQ=DAILY;UNTIL=20260101T000000Z;COUNT=3")
            .expect_err("two stopping conditions are refused");
        assert!(refused.contains("not both"), "{refused}");
    }

    /// What the rule does not say, the automation's first firing supplies.
    #[test]
    fn a_rule_that_names_no_time_repeats_at_the_anchors_own() {
        let anchor = at_utc("2026-09-14T06:47:00Z");
        let rule = Recurrence::parse("FREQ=DAILY").expect("accepted");

        assert_eq!(
            rule.next_after(anchor, anchor, 1),
            Some(at_utc("2026-09-15T06:47:00Z"))
        );
    }

    #[test]
    fn a_daily_rule_walks_its_hours_in_order_and_then_the_next_day() {
        let anchor = at_utc("2026-09-14T09:00:00Z");
        let rule = Recurrence::parse("FREQ=DAILY;BYHOUR=9,17;BYMINUTE=3").expect("accepted");

        let first = rule
            .next_after(anchor, at_utc("2026-09-14T00:00:00Z"), 0)
            .expect("a firing");
        assert_eq!(first, at_utc("2026-09-14T09:03:00Z"));
        assert_eq!(
            rule.next_after(anchor, first, 1),
            Some(at_utc("2026-09-14T17:03:00Z"))
        );
        assert_eq!(
            rule.next_after(anchor, at_utc("2026-09-14T17:03:00Z"), 2),
            Some(at_utc("2026-09-15T09:03:00Z"))
        );
    }

    #[test]
    fn a_weekly_rule_fires_on_each_named_day_and_then_skips_its_interval() {
        // A Monday.
        let anchor = at_utc("2026-09-14T09:00:00Z");
        let rule =
            Recurrence::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;BYHOUR=9").expect("accepted");

        let thursday = rule.next_after(anchor, anchor, 1).expect("a firing");
        assert_eq!(thursday, at_utc("2026-09-17T09:00:00Z"));
        assert_eq!(thursday.weekday(), Weekday::Thu);

        // Two weeks on from the anchor's week, not one.
        assert_eq!(
            rule.next_after(anchor, thursday, 2),
            Some(at_utc("2026-09-28T09:00:00Z"))
        );
    }

    /// A month that has no 31st gets its last day rather than sliding into the
    /// next month, which is the failure that turns a month-end report into a
    /// first-of-the-month one.
    #[test]
    fn a_monthly_rule_clamps_a_day_the_month_does_not_have() {
        let anchor = at_utc("2026-01-31T08:00:00Z");
        let rule = Recurrence::parse("FREQ=MONTHLY").expect("accepted");

        let february = rule.next_after(anchor, anchor, 1).expect("a firing");
        assert_eq!(february, at_utc("2026-02-28T08:00:00Z"));
        assert_eq!(
            rule.next_after(anchor, february, 2),
            Some(at_utc("2026-03-31T08:00:00Z")),
            "a clamped month must not drag the following ones back with it"
        );
    }

    #[test]
    fn a_monthly_rule_on_a_day_no_month_has_gives_up_rather_than_looping() {
        let anchor = at_utc("2026-01-01T08:00:00Z");
        let rule = Recurrence::parse("FREQ=MONTHLY;BYMONTHDAY=31;BYHOUR=8").expect("accepted");

        // Only the long months, in order, and never a 31st that does not exist.
        let march = rule.next_after(anchor, at_utc("2026-02-01T00:00:00Z"), 1);
        assert_eq!(march, Some(at_utc("2026-03-31T08:00:00Z")));
    }

    #[test]
    fn an_hourly_rule_counts_from_the_anchor_and_keeps_its_minute() {
        let anchor = at_utc("2026-09-14T06:47:00Z");
        let rule = Recurrence::parse("FREQ=HOURLY;INTERVAL=6").expect("accepted");

        let next = rule.next_after(anchor, anchor, 1).expect("a firing");
        assert_eq!(next, at_utc("2026-09-14T12:47:00Z"));
        assert_eq!(
            rule.next_after(anchor, next, 2),
            Some(at_utc("2026-09-14T18:47:00Z"))
        );
    }

    #[test]
    fn count_stops_the_rule_after_the_firings_it_names() {
        let anchor = at_utc("2026-09-14T09:00:00Z");
        let rule = Recurrence::parse("FREQ=DAILY;COUNT=3").expect("accepted");

        assert!(rule.next_after(anchor, anchor, 1).is_some());
        assert!(rule.next_after(anchor, anchor, 2).is_some());
        assert_eq!(
            rule.next_after(anchor, anchor, 3),
            None,
            "the third firing is the last, so a fourth is never scheduled"
        );
    }

    #[test]
    fn until_stops_the_rule_at_the_instant_it_names() {
        let anchor = at_utc("2026-09-14T09:00:00Z");
        let rule = Recurrence::parse("FREQ=DAILY;UNTIL=20260916T090000Z").expect("accepted");

        assert_eq!(
            rule.next_after(anchor, anchor, 1),
            Some(at_utc("2026-09-15T09:00:00Z"))
        );
        assert_eq!(
            rule.next_after(anchor, at_utc("2026-09-15T09:00:00Z"), 2),
            Some(at_utc("2026-09-16T09:00:00Z")),
            "UNTIL includes the instant it names"
        );
        assert_eq!(
            rule.next_after(anchor, at_utc("2026-09-16T09:00:00Z"), 3),
            None
        );
    }

    /// A firing missed while the server was down does not replay: the next one
    /// is the next one from now, not the one that was slept through.
    #[test]
    fn a_schedule_resumed_after_an_outage_fires_next_rather_than_catching_up() {
        let anchor = at_utc("2026-09-01T09:00:00Z");
        let rule = Recurrence::parse("FREQ=DAILY;BYHOUR=9").expect("accepted");

        assert_eq!(
            rule.next_after(anchor, at_utc("2026-09-14T06:47:00Z"), 13),
            Some(at_utc("2026-09-14T09:00:00Z"))
        );
    }

    /// A rule whose clauses can never all hold answers `None` instead of
    /// walking forward for ever. Every February, asking for a 31st: the period
    /// never reaches a month that has one, so only the bound ends the search.
    #[test]
    fn a_rule_nothing_can_satisfy_gives_up_instead_of_spinning() {
        let february = at_utc("2026-02-01T09:00:00Z");
        let rule =
            Recurrence::parse("FREQ=MONTHLY;INTERVAL=12;BYMONTHDAY=31;BYHOUR=9").expect("accepted");

        assert_eq!(rule.next_after(february, february, 1), None);
    }
}
