//! The wall clock a person reads.
//!
//! The IANA time zone, and the instants its times name across a daylight-saving
//! change, come from `abnegate_recurrence`. Zone keeps its own wording for a
//! name it refuses, because the refusal reaches a model as a tool result and
//! names the `time_zone` field it has to fix.

pub use abnegate_recurrence::Timezone as TimeZone;

/// Reads an IANA name, such as `Europe/London`, ignoring surrounding
/// whitespace, and refuses one the tz database does not hold with an example
/// of a name it does.
pub fn parse(name: &str) -> Result<TimeZone, String> {
    #[expect(clippy::disallowed_methods)]
    let parsed = TimeZone::parse(name);
    parsed.map_err(|_| {
        format!(
            "time_zone \"{name}\" is not an IANA time zone name. Use one such as \
             Europe/London or Pacific/Auckland."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    #[test]
    fn a_name_the_tz_database_does_not_hold_is_refused_with_an_example() {
        assert_eq!(
            parse("Middle/Earth"),
            Err(
                "time_zone \"Middle/Earth\" is not an IANA time zone name. Use one such as \
                 Europe/London or Pacific/Auckland."
                    .to_string()
            )
        );
        assert_eq!(
            parse(" Pacific/Auckland ").map(TimeZone::name),
            Ok("Pacific/Auckland")
        );
    }

    /// A wall-clock time past the last instant UTC holds has no instant.
    #[test]
    fn the_calendars_last_minute_behind_utc_has_no_instant() {
        assert_eq!(
            parse("America/New_York")
                .unwrap()
                .instant(NaiveDateTime::MAX),
            None
        );
    }
}
