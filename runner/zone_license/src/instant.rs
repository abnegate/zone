use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

use crate::instant_error::InstantError;

const DATE_FORMAT: &str = "%Y-%m-%d";

pub fn parse(text: &str) -> Result<DateTime<Utc>, InstantError> {
    let text = text.trim();
    if let Ok(instant) = DateTime::parse_from_rfc3339(text) {
        return Ok(instant.to_utc());
    }
    NaiveDate::parse_from_str(text, DATE_FORMAT)
        .map(|date| date.and_time(NaiveTime::MIN).and_utc())
        .map_err(|_| InstantError)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn parses_a_utc_timestamp() {
        assert_eq!(
            parse("2027-11-01T12:30:00Z"),
            Ok(Utc.with_ymd_and_hms(2027, 11, 1, 12, 30, 0).unwrap())
        );
    }

    #[test]
    fn converts_an_offset_timestamp_to_utc() {
        assert_eq!(
            parse("2027-11-01T12:30:00+02:00"),
            Ok(Utc.with_ymd_and_hms(2027, 11, 1, 10, 30, 0).unwrap())
        );
    }

    #[test]
    fn reads_a_date_as_midnight_utc() {
        assert_eq!(
            parse("2027-11-01"),
            Ok(Utc.with_ymd_and_hms(2027, 11, 1, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn refuses_anything_else() {
        for text in [
            "",
            "tomorrow",
            "2027-13-01",
            "2027-11-01T25:00:00Z",
            "01/11/2027",
            "2027-11-01 12:30",
        ] {
            assert_eq!(parse(text), Err(InstantError), "accepted {text:?}");
        }
    }
}
