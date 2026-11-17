use serde::Deserialize;

use crate::workers::analytics::TimePeriod;

pub const INVALID_PERIOD: &str = "period must be one of day, week, month, quarter";

const DEFAULT_PERIOD: TimePeriod = TimePeriod::Week;

#[derive(Debug, Default, Deserialize)]
pub struct PeriodQuery {
    pub period: Option<TimePeriod>,
}

impl PeriodQuery {
    pub fn period(&self) -> TimePeriod {
        self.period.unwrap_or(DEFAULT_PERIOD)
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::Query;
    use axum::http::Uri;

    use super::*;

    fn read(query: &str) -> Option<TimePeriod> {
        let uri: Uri = format!("/analytics{query}").parse().expect("a valid uri");
        Query::<PeriodQuery>::try_from_uri(&uri)
            .ok()
            .map(|Query(query)| query.period())
    }

    #[test]
    fn no_period_reads_as_a_week() {
        assert_eq!(read(""), Some(TimePeriod::Week));
    }

    #[test]
    fn every_period_parses() {
        for period in TimePeriod::ALL {
            assert_eq!(
                read(&format!("?period={}", period.as_str())),
                Some(period),
                "{period}"
            );
        }
    }

    #[test]
    fn a_capitalised_period_is_refused() {
        assert_eq!(read("?period=Week"), None);
    }

    #[test]
    fn an_unknown_or_empty_period_is_refused() {
        assert_eq!(read("?period=bogus"), None);
        assert_eq!(read("?period="), None);
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
