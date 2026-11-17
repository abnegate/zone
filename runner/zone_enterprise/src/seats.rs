use chrono::{DateTime, Datelike, Days, NaiveTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seats {
    pub used: u32,
    pub licensed: Option<u32>,
}

impl Seats {
    pub fn over(&self) -> bool {
        self.licensed.is_some_and(|licensed| self.used > licensed)
    }
}

pub fn period_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let first = now.date_naive() - Days::new(u64::from(now.day0()));
    first.and_time(NaiveTime::MIN).and_utc()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .unwrap()
    }

    #[test]
    fn exactly_licensed_is_not_over() {
        assert!(
            !Seats {
                used: 25,
                licensed: Some(25)
            }
            .over()
        );
    }

    #[test]
    fn one_more_than_licensed_is_over() {
        assert!(
            Seats {
                used: 26,
                licensed: Some(25)
            }
            .over()
        );
    }

    #[test]
    fn fewer_than_licensed_is_not_over() {
        assert!(
            !Seats {
                used: 0,
                licensed: Some(1)
            }
            .over()
        );
    }

    #[test]
    fn unlicensed_is_never_over() {
        for used in [0, 1, u32::MAX] {
            assert!(
                !Seats {
                    used,
                    licensed: None
                }
                .over()
            );
        }
    }

    #[test]
    fn period_starts_on_the_first_of_the_month() {
        assert_eq!(
            period_start(at(2026, 11, 16, 13, 45, 7)),
            at(2026, 11, 1, 0, 0, 0)
        );
    }

    #[test]
    fn period_start_keeps_the_first_instant_of_a_month() {
        assert_eq!(
            period_start(at(2027, 3, 1, 0, 0, 0)),
            at(2027, 3, 1, 0, 0, 0)
        );
    }

    #[test]
    fn period_start_on_the_last_second_of_the_year() {
        assert_eq!(
            period_start(at(2026, 12, 31, 23, 59, 59)),
            at(2026, 12, 1, 0, 0, 0)
        );
    }

    #[test]
    fn period_start_in_a_leap_year_february() {
        assert_eq!(
            period_start(at(2028, 2, 29, 12, 0, 0)),
            at(2028, 2, 1, 0, 0, 0)
        );
    }

    #[test]
    fn period_start_drops_subsecond_precision() {
        let now = at(2026, 11, 16, 0, 0, 0) + chrono::TimeDelta::nanoseconds(123_456_789);
        assert_eq!(period_start(now), at(2026, 11, 1, 0, 0, 0));
    }
}
