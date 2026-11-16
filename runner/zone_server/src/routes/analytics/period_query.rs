use serde::Deserialize;

use crate::error::ServerError;
use crate::workers::analytics::TimePeriod;

pub const INVALID_PERIOD: &str = "period must be one of day, week, month, quarter";

const DEFAULT_PERIOD: TimePeriod = TimePeriod::Week;

#[derive(Debug, Default, Deserialize)]
pub struct PeriodQuery {
    pub period: Option<String>,
}

impl PeriodQuery {
    pub fn period(&self) -> Result<TimePeriod, ServerError> {
        match self.period.as_deref() {
            None => Ok(DEFAULT_PERIOD),
            Some(text) => TimePeriod::parse(text)
                .ok_or_else(|| ServerError::BadRequest(INVALID_PERIOD.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(period: Option<&str>) -> PeriodQuery {
        PeriodQuery {
            period: period.map(str::to_string),
        }
    }

    fn refused(period: &str) -> String {
        match query(Some(period)).period() {
            Err(ServerError::BadRequest(message)) => message,
            other => panic!("expected a bad request for {period:?}, got {other:?}"),
        }
    }

    #[test]
    fn no_period_reads_as_a_week() {
        assert_eq!(query(None).period().ok(), Some(TimePeriod::Week));
    }

    #[test]
    fn every_period_parses() {
        for (text, period) in [
            ("day", TimePeriod::Day),
            ("week", TimePeriod::Week),
            ("month", TimePeriod::Month),
            ("quarter", TimePeriod::Quarter),
        ] {
            assert_eq!(query(Some(text)).period().ok(), Some(period), "{text}");
        }
    }

    #[test]
    fn a_capitalised_period_is_refused() {
        assert_eq!(refused("Week"), INVALID_PERIOD);
    }

    #[test]
    fn an_unknown_period_is_refused() {
        assert_eq!(refused("bogus"), INVALID_PERIOD);
    }

    #[test]
    fn the_refusal_names_every_period() {
        let names: Vec<&str> = TimePeriod::ALL.map(TimePeriod::as_str).to_vec();
        assert_eq!(
            INVALID_PERIOD,
            format!("period must be one of {}", names.join(", "))
        );
    }
}
