//! The window a set of numbers covers, and how it is cut into buckets.
//!
//! Every window here is half-open, `[start, end)`. A run that finishes at the
//! exact moment one bucket ends belongs to the next one, so no run is counted
//! twice and none falls between two buckets.
//!
//! Buckets are aligned to the clock rather than to `now`: an hourly bucket
//! starts on the hour, a daily one at midnight, a weekly one on Monday. Two
//! reports generated minutes apart then describe the same buckets, which is
//! what makes one comparable with the next.

use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime, Timelike, Weekday};
use serde::{Deserialize, Serialize};

/// How much history a set of numbers covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimePeriod {
    Day,
    Week,
    Month,
    Quarter,
}

impl TimePeriod {
    pub const ALL: [TimePeriod; 4] = [
        TimePeriod::Day,
        TimePeriod::Week,
        TimePeriod::Month,
        TimePeriod::Quarter,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TimePeriod::Day => "day",
            TimePeriod::Week => "week",
            TimePeriod::Month => "month",
            TimePeriod::Quarter => "quarter",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|period| period.as_str() == text)
    }

    pub fn days(self) -> i64 {
        match self {
            TimePeriod::Day => 1,
            TimePeriod::Week => 7,
            TimePeriod::Month => 30,
            TimePeriod::Quarter => 90,
        }
    }

    pub fn span(self) -> Duration {
        Duration::days(self.days())
    }

    /// The bucket width that gives a readable series for this period.
    pub fn bucket(self) -> BucketSize {
        match self {
            TimePeriod::Day => BucketSize::Hour,
            TimePeriod::Week | TimePeriod::Month => BucketSize::Day,
            TimePeriod::Quarter => BucketSize::Week,
        }
    }

    /// The window of this length ending at `end`.
    pub fn window_ending(self, end: NaiveDateTime) -> TimeWindow {
        TimeWindow::new(end - self.span(), end)
    }
}

impl std::fmt::Display for TimePeriod {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How wide one bucket of a time series is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BucketSize {
    Hour,
    Day,
    Week,
}

impl BucketSize {
    pub fn as_str(self) -> &'static str {
        match self {
            BucketSize::Hour => "hour",
            BucketSize::Day => "day",
            BucketSize::Week => "week",
        }
    }

    pub fn span(self) -> Duration {
        match self {
            BucketSize::Hour => Duration::hours(1),
            BucketSize::Day => Duration::days(1),
            BucketSize::Week => Duration::days(7),
        }
    }

    /// The start of the bucket `moment` falls in.
    pub fn align(self, moment: NaiveDateTime) -> NaiveDateTime {
        let midnight = moment.date().and_time(NaiveTime::MIN);
        match self {
            BucketSize::Hour => midnight + Duration::hours(i64::from(moment.hour())),
            BucketSize::Day => midnight,
            BucketSize::Week => {
                let since_monday = i64::from(moment.date().weekday().num_days_from_monday());
                midnight - Duration::days(since_monday)
            }
        }
    }
}

impl std::fmt::Display for BucketSize {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A half-open stretch of time, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimeWindow {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl TimeWindow {
    /// A window from `start` to `end`, empty rather than inverted if they cross.
    pub fn new(start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    pub fn contains(&self, moment: NaiveDateTime) -> bool {
        moment >= self.start && moment < self.end
    }

    pub fn duration(&self) -> Duration {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// The earlier and later halves, for comparing one against the other.
    pub fn halves(&self) -> (TimeWindow, TimeWindow) {
        let middle = self.start + self.duration() / 2;
        (
            TimeWindow::new(self.start, middle),
            TimeWindow::new(middle, self.end),
        )
    }

    /// Every bucket covering this window, aligned to the clock and gapless.
    ///
    /// The first bucket starts at or before [`TimeWindow::start`], so a window
    /// that begins mid-bucket still has that partial bucket represented rather
    /// than dropping the runs inside it.
    pub fn buckets(&self, size: BucketSize) -> Vec<TimeWindow> {
        if self.is_empty() {
            return Vec::new();
        }

        let span = size.span();
        let mut buckets = Vec::new();
        let mut cursor = size.align(self.start);
        while cursor < self.end {
            let next = cursor + span;
            buckets.push(TimeWindow::new(cursor, next));
            cursor = next;
        }
        buckets
    }
}

/// The most recent `weekday` at or before `moment`'s date, at midnight.
pub fn last_weekday_before(moment: NaiveDateTime, weekday: Weekday) -> NaiveDateTime {
    let midnight = moment.date().and_time(NaiveTime::MIN);
    let current = i64::from(moment.date().weekday().num_days_from_monday());
    let target = i64::from(weekday.num_days_from_monday());
    midnight - Duration::days((current - target).rem_euclid(7))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn moment(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    #[test]
    fn periods_round_trip_through_their_names() {
        for period in TimePeriod::ALL {
            assert_eq!(TimePeriod::parse(period.as_str()), Some(period));
        }
        assert_eq!(TimePeriod::parse("fortnight"), None);
    }

    #[test]
    fn a_window_is_half_open_at_both_ends() {
        let window = TimeWindow::new(moment(2026, 9, 7, 0, 0), moment(2026, 9, 8, 0, 0));

        assert!(window.contains(window.start), "the start is inside");
        assert!(!window.contains(window.end), "the end is not");
        assert!(window.contains(moment(2026, 9, 7, 23, 59)));
        assert!(!window.contains(moment(2026, 9, 6, 23, 59)));
    }

    #[test]
    fn an_inverted_window_is_empty_rather_than_negative() {
        let window = TimeWindow::new(moment(2026, 9, 8, 0, 0), moment(2026, 9, 7, 0, 0));

        assert!(window.is_empty());
        assert_eq!(window.duration(), Duration::zero());
        assert!(window.buckets(BucketSize::Hour).is_empty());
    }

    #[test]
    fn buckets_align_to_the_clock_not_to_the_window_start() {
        let window = TimeWindow::new(moment(2026, 9, 7, 10, 37), moment(2026, 9, 7, 13, 0));
        let buckets = window.buckets(BucketSize::Hour);

        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].start, moment(2026, 9, 7, 10, 0));
        assert_eq!(buckets[0].end, moment(2026, 9, 7, 11, 0));
        assert_eq!(buckets[2].end, moment(2026, 9, 7, 13, 0));
    }

    #[test]
    fn buckets_are_gapless_and_never_overlap() {
        let window = TimeWindow::new(moment(2026, 9, 1, 6, 0), moment(2026, 9, 8, 6, 0));
        let buckets = window.buckets(BucketSize::Day);

        for pair in buckets.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "{pair:?} leaves a gap");
        }
        assert!(
            buckets
                .first()
                .is_some_and(|first| first.start <= window.start)
        );
        assert!(buckets.last().is_some_and(|last| last.end >= window.end));
    }

    #[test]
    fn a_run_on_a_bucket_boundary_lands_in_the_later_bucket() {
        let window = TimeWindow::new(moment(2026, 9, 7, 0, 0), moment(2026, 9, 7, 3, 0));
        let buckets = window.buckets(BucketSize::Hour);
        let boundary = moment(2026, 9, 7, 1, 0);

        let owning: Vec<usize> = buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| bucket.contains(boundary))
            .map(|(index, _)| index)
            .collect();

        assert_eq!(owning, vec![1], "exactly one bucket owns the boundary");
    }

    #[test]
    fn weekly_buckets_start_on_monday() {
        // 2026-09-07 is a Monday, so a window opening on the Wednesday after it
        // still belongs to the bucket that opened on that Monday.
        let wednesday = moment(2026, 9, 9, 15, 30);
        assert_eq!(
            BucketSize::Week.align(wednesday),
            moment(2026, 9, 7, 0, 0),
            "aligned to the Monday of the same week"
        );
    }

    #[test]
    fn daily_alignment_drops_the_time_of_day() {
        assert_eq!(
            BucketSize::Day.align(moment(2026, 9, 7, 23, 59)),
            moment(2026, 9, 7, 0, 0)
        );
    }

    #[test]
    fn halves_meet_in_the_middle_and_cover_the_whole_window() {
        let window = TimeWindow::new(moment(2026, 9, 1, 0, 0), moment(2026, 9, 8, 0, 0));
        let (earlier, later) = window.halves();

        assert_eq!(earlier.start, window.start);
        assert_eq!(earlier.end, later.start);
        assert_eq!(later.end, window.end);
        assert_eq!(earlier.duration(), later.duration());
    }

    #[test]
    fn a_period_window_ends_where_it_was_asked_to() {
        let end = moment(2026, 9, 8, 9, 0);
        let window = TimePeriod::Week.window_ending(end);

        assert_eq!(window.end, end);
        assert_eq!(window.start, moment(2026, 9, 1, 9, 0));
        assert_eq!(TimePeriod::Week.bucket(), BucketSize::Day);
    }

    #[test]
    fn the_last_weekday_before_a_moment_walks_backwards() {
        let wednesday = moment(2026, 9, 9, 15, 30);

        assert_eq!(
            last_weekday_before(wednesday, Weekday::Mon),
            moment(2026, 9, 7, 0, 0)
        );
        assert_eq!(
            last_weekday_before(wednesday, Weekday::Wed),
            moment(2026, 9, 9, 0, 0),
            "the same day counts, at midnight"
        );
        assert_eq!(
            last_weekday_before(wednesday, Weekday::Thu),
            moment(2026, 9, 3, 0, 0),
            "a weekday later in the week is a full week back"
        );
    }
}
