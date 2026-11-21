use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};

use crate::spend::error::Error;

const FORMAT: &str = "%Y-%m-%d";

pub const MAXIMUM_DAYS: i64 = 366;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl Range {
    /// `from` defaults to the first of `today`'s month and `to` to `today`; both ends are
    /// inclusive days, at most [`MAXIMUM_DAYS`] of them.
    pub fn parse(from: Option<&str>, to: Option<&str>, today: NaiveDate) -> Result<Self, Error> {
        let from = from
            .map(date)
            .transpose()?
            .unwrap_or_else(|| today.with_day(1).unwrap_or(today));
        let to = to.map(date).transpose()?.unwrap_or(today);
        let range = Self { from, to };
        if from > to || to.succ_opt().is_none() || range.length() > MAXIMUM_DAYS {
            return Err(Error::RangeInvalid);
        }
        Ok(range)
    }

    pub fn length(&self) -> i64 {
        (self.to - self.from).num_days() + 1
    }

    pub fn days(self) -> impl Iterator<Item = NaiveDate> {
        self.from.iter_days().take_while(move |day| *day <= self.to)
    }

    pub fn start(&self) -> DateTime<Utc> {
        midnight(self.from)
    }

    pub fn end(&self) -> DateTime<Utc> {
        midnight(self.to.succ_opt().unwrap_or(self.to))
    }
}

fn date(text: &str) -> Result<NaiveDate, Error> {
    NaiveDate::parse_from_str(text, FORMAT)
        .ok()
        .filter(|date| date.format(FORMAT).to_string() == text)
        .ok_or(Error::RangeInvalid)
}

fn midnight(date: NaiveDate) -> DateTime<Utc> {
    date.and_time(NaiveTime::MIN).and_utc()
}
