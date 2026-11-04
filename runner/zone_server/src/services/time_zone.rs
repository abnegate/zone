//! The wall clock a person reads, and the instants its times name.
//!
//! A recurring reminder is asked for in a person's own hours — "every weekday
//! at nine" — and fires at instants, so every rule is evaluated on this clock
//! and only its answer is turned into UTC. Two kinds of wall-clock time have
//! no single instant, and both are settled the same way every time:
//!
//! - A time the clocks skip, in the hour a spring-forward removes, moves
//!   forward to the first time the clocks do show, which is the instant they
//!   change. 01:30 on the morning London skips fires at 02:00 BST.
//! - A time the clocks show twice, in the hour a fall-back repeats, is the
//!   first of the two. 01:30 on the morning London repeats fires at 01:30 BST,
//!   and not again an hour later.

use chrono::{
    DateTime, Duration, FixedOffset, LocalResult, NaiveDateTime, Offset, TimeZone as _, Timelike,
    Utc,
};
use chrono_tz::Tz;

/// How far a skipped time is carried before it is given up on. Longer than the
/// longest gap the tz database holds, the whole day Samoa skipped in 2011.
const LONGEST_GAP: Duration = Duration::hours(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeZone(Tz);

impl TimeZone {
    pub const UTC: Self = Self(Tz::UTC);

    /// Reads an IANA name, such as `Europe/London`.
    pub fn parse(name: &str) -> Result<Self, String> {
        name.trim().parse::<Tz>().map(Self).map_err(|_| {
            format!(
                "time_zone \"{name}\" is not an IANA time zone name. Use one such as \
                 Europe/London or Pacific/Auckland."
            )
        })
    }

    pub fn name(self) -> &'static str {
        self.0.name()
    }

    /// The offset from UTC this zone's clocks show at `instant`.
    pub fn offset(self, instant: DateTime<Utc>) -> FixedOffset {
        self.0.offset_from_utc_datetime(&instant.naive_utc()).fix()
    }

    /// What this zone's clocks show at `instant`.
    pub fn local(self, instant: DateTime<Utc>) -> NaiveDateTime {
        instant.with_timezone(&self.0).naive_local()
    }

    /// The instant this zone's clocks show `local`, under the rule the module
    /// states for a time they skip or show twice.
    pub fn instant(self, local: NaiveDateTime) -> Option<DateTime<Utc>> {
        let mut shown = local;
        while shown - local <= LONGEST_GAP {
            match self.0.from_local_datetime(&shown) {
                LocalResult::Single(instant) | LocalResult::Ambiguous(instant, _) => {
                    return Some(instant.with_timezone(&Utc));
                }
                LocalResult::None => {
                    shown = (shown + Duration::minutes(1))
                        .with_second(0)?
                        .with_nanosecond(0)?;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").expect("the fixture is a time")
    }

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().expect("the fixture is an instant")
    }

    #[test]
    fn a_name_the_tz_database_does_not_hold_is_refused_with_an_example() {
        let refused = TimeZone::parse("Middle/Earth").expect_err("no such zone");
        assert!(refused.contains("Middle/Earth"), "{refused}");
        assert!(refused.contains("Europe/London"), "{refused}");
        assert_eq!(
            TimeZone::parse(" Pacific/Auckland ").map(TimeZone::name),
            Ok("Pacific/Auckland")
        );
    }

    #[test]
    fn a_skipped_time_fires_at_the_instant_the_clocks_change() {
        let london = TimeZone::parse("Europe/London").unwrap();
        assert_eq!(
            london.instant(local("2027-03-28 01:30")),
            Some(utc("2027-03-28T01:00:00Z"))
        );
        let auckland = TimeZone::parse("Pacific/Auckland").unwrap();
        assert_eq!(
            auckland.instant(local("2027-09-26 02:30")),
            Some(utc("2027-09-25T14:00:00Z"))
        );
    }

    #[test]
    fn a_time_shown_twice_fires_at_the_first_of_them() {
        let london = TimeZone::parse("Europe/London").unwrap();
        assert_eq!(
            london.instant(local("2027-10-31 01:30")),
            Some(utc("2027-10-31T00:30:00Z"))
        );
        let auckland = TimeZone::parse("Pacific/Auckland").unwrap();
        assert_eq!(
            auckland.instant(local("2027-04-04 02:30")),
            Some(utc("2027-04-03T13:30:00Z"))
        );
    }
}
