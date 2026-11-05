//! Where and when this turn runs.

use abnegate_llm::Effort;
use chrono::{DateTime, FixedOffset, Local, Utc};
use std::path::PathBuf;

use super::Vcs;
use crate::agent::tools::host_root;
use crate::services::time_zone::TimeZone;

/// Fallback zone name when the host exposes no IANA identifier.
const UNKNOWN_TIMEZONE: &str = "UTC";

/// Everything about the machine, the clock and the caller a section may state.
///
/// Built once per turn and passed by reference into every section, so one turn
/// states one instant. A preview and the generation that follows it are separate
/// turns that read the clock separately, so the two agree on width rather than
/// on bytes: every field renders at a fixed width, which is what keeps the
/// preview's cost estimate true of the send it predicts.
#[derive(Debug, Clone)]
pub struct Environment {
    pub now: DateTime<FixedOffset>,
    pub timezone: String,
    /// Whether `timezone` is the user's own, as their console reported it,
    /// rather than the zone this server runs in.
    pub reported: bool,
    pub directory: PathBuf,
    pub platform: &'static str,
    pub vcs: Option<Vcs>,
    pub user: Option<String>,
    pub workspace: Option<String>,
    pub effort: Option<Effort>,
    /// Whether the workspace has an active GitHub source for zone's own
    /// GitHub tools to act on.
    pub github: bool,
}

impl Environment {
    /// Read the clock, the zone, the platform and the working directory.
    pub fn here() -> Self {
        Self {
            now: Local::now().fixed_offset(),
            timezone: iana_time_zone::get_timezone()
                .unwrap_or_else(|_| UNKNOWN_TIMEZONE.to_string()),
            reported: false,
            directory: host_root(),
            platform: std::env::consts::OS,
            vcs: None,
            user: None,
            workspace: None,
            effort: None,
            github: false,
        }
    }

    /// A fixed environment, so a rendered prompt is byte-stable under test.
    pub fn at(now: DateTime<FixedOffset>, timezone: impl Into<String>, directory: PathBuf) -> Self {
        Self {
            now,
            timezone: timezone.into(),
            reported: true,
            directory,
            platform: std::env::consts::OS,
            vcs: None,
            user: None,
            workspace: None,
            effort: None,
            github: false,
        }
    }

    pub fn with_vcs(mut self, vcs: Vcs) -> Self {
        self.vcs = Some(vcs);
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    pub fn with_directory(mut self, directory: PathBuf) -> Self {
        self.directory = directory;
        self
    }

    pub fn with_effort(mut self, effort: Effort) -> Self {
        self.effort = Some(effort);
        self
    }

    pub fn with_github(mut self) -> Self {
        self.github = true;
        self
    }

    pub fn with_time_zone(mut self, zone: TimeZone) -> Self {
        let instant = self.now.with_timezone(&Utc);
        self.now = instant.with_timezone(&zone.offset(instant));
        self.timezone = zone.name().to_string();
        self.reported = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> Environment {
        Environment::at(
            DateTime::parse_from_rfc3339("2026-09-09T09:30:00+12:00").unwrap(),
            "Pacific/Auckland",
            PathBuf::from("/srv/zone"),
        )
    }

    #[test]
    fn a_fixed_environment_starts_empty_and_builders_fill_it() {
        let environment = fixed();
        assert_eq!(environment.timezone, "Pacific/Auckland");
        assert_eq!(environment.directory, PathBuf::from("/srv/zone"));
        assert_eq!(environment.platform, std::env::consts::OS);
        assert!(environment.vcs.is_none());
        assert!(environment.user.is_none());
        assert!(environment.workspace.is_none());
        assert!(environment.effort.is_none());
        assert!(!environment.github);

        let filled = fixed()
            .with_vcs(Vcs {
                branch: "main".into(),
                head: "83ff18b".into(),
            })
            .with_user("Ari")
            .with_workspace("Zone")
            .with_effort(Effort::High)
            .with_github();
        assert!(filled.github);
        assert_eq!(filled.vcs.unwrap().branch, "main");
        assert_eq!(filled.user.as_deref(), Some("Ari"));
        assert_eq!(filled.workspace.as_deref(), Some("Zone"));
        assert_eq!(filled.effort, Some(Effort::High));
    }

    #[test]
    fn a_person_s_zone_restates_the_same_instant_on_their_clock() {
        let environment = fixed().with_time_zone(TimeZone::parse("Europe/London").expect("a zone"));
        assert_eq!(environment.timezone, "Europe/London");
        assert_eq!(
            environment.now.to_rfc3339(),
            "2026-09-08T22:30:00+01:00",
            "09:30 in Auckland is 22:30 the evening before in London"
        );
    }

    #[test]
    fn the_live_environment_names_a_timezone_and_a_directory() {
        let environment = Environment::here();
        assert!(!environment.timezone.is_empty());
        assert!(!environment.platform.is_empty());
        assert!(!environment.directory.as_os_str().is_empty());
    }
}
