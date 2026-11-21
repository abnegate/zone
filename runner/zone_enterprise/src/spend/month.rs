use chrono::{DateTime, Datelike, Months, NaiveDate, NaiveTime, Utc};

const LABEL: &str = "%Y-%m";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month(NaiveDate);

impl Month {
    pub fn of(moment: DateTime<Utc>) -> Self {
        let date = moment.date_naive();
        Self(date.with_day(1).unwrap_or(date))
    }

    pub fn current() -> Self {
        Self::of(Utc::now())
    }

    pub fn label(self) -> String {
        self.0.format(LABEL).to_string()
    }

    pub fn date(self) -> NaiveDate {
        self.0
    }

    pub fn start(self) -> DateTime<Utc> {
        self.0.and_time(NaiveTime::MIN).and_utc()
    }

    pub fn end(self) -> DateTime<Utc> {
        self.0
            .checked_add_months(Months::new(1))
            .map_or(DateTime::<Utc>::MAX_UTC, |next| {
                next.and_time(NaiveTime::MIN).and_utc()
            })
    }
}
