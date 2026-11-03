//! The names zone records an agent plan's usage windows under, the ones its
//! usage endpoint gives them, so a window the agent reports while a turn runs
//! lands on the same window of a login's usage.

pub const FIVE_HOURS: &str = "5h";
pub const SEVEN_DAYS: &str = "7d";
pub const SEVEN_DAYS_OPUS: &str = "7d Opus";
pub const SEVEN_DAYS_SONNET: &str = "7d Sonnet";

#[cfg(test)]
mod tests {
    use abnegate_llm::{Window, WindowKind};
    use chrono::{DateTime, Utc};

    use super::*;

    /// A row of `agent_logins.windows` as zone has always written it.
    const STORED: &str = r#"[{"name":"5h","used_percent":42.0,"used":null,"limit":null,"resets_at":"2026-10-05T00:00:00Z"},{"name":"7d","used_percent":31.0,"used":310,"limit":1000,"resets_at":null}]"#;

    fn midnight() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-05T00:00:00Z")
            .expect("a valid timestamp")
            .with_timezone(&Utc)
    }

    fn five_hours() -> Window {
        Window::new(FIVE_HOURS)
            .with_used_percent(42.0)
            .with_resets_at(midnight())
    }

    fn seven_days() -> Window {
        Window::new(SEVEN_DAYS)
            .with_used_percent(31.0)
            .with_used(310)
            .with_limit(1000)
    }

    #[test]
    fn a_stored_row_of_windows_still_reads() {
        let read: Vec<Window> = serde_json::from_str(STORED).expect("a stored row to read");

        assert_eq!(read, [five_hours(), seven_days()]);
    }

    #[test]
    fn windows_are_written_as_zone_has_always_stored_them() {
        let written = [five_hours().with_kind(WindowKind::FiveHours), seven_days()];

        assert_eq!(
            serde_json::to_string(&written).expect("windows to serialize"),
            STORED
        );
    }
}
