//! Where and when this turn runs.

use chrono::{DateTime, FixedOffset, Local};
use std::path::PathBuf;
use zone_core::llm::Effort;

use super::Vcs;
use crate::agent::tools::host_root;

/// Fallback zone name when the host exposes no IANA identifier.
const UNKNOWN_TIMEZONE: &str = "UTC";

/// Everything about the machine, the clock and the caller a section may state.
///
/// Built once per turn so a preview and the generation that follows it agree,
/// and passed by reference into every section.
#[derive(Debug, Clone)]
pub struct Environment {
    pub now: DateTime<FixedOffset>,
    pub timezone: String,
    pub directory: PathBuf,
    pub platform: &'static str,
    pub vcs: Option<Vcs>,
    pub user: Option<String>,
    pub workspace: Option<String>,
    pub effort: Option<Effort>,
}

impl Environment {
    /// Read the clock, the zone, the platform and the working directory.
    pub fn here() -> Self {
        Self {
            now: Local::now().fixed_offset(),
            timezone: iana_time_zone::get_timezone()
                .unwrap_or_else(|_| UNKNOWN_TIMEZONE.to_string()),
            directory: host_root(),
            platform: std::env::consts::OS,
            vcs: None,
            user: None,
            workspace: None,
            effort: None,
        }
    }

    /// A fixed environment, so a rendered prompt is byte-stable under test.
    pub fn at(now: DateTime<FixedOffset>, timezone: impl Into<String>, directory: PathBuf) -> Self {
        Self {
            now,
            timezone: timezone.into(),
            directory,
            platform: std::env::consts::OS,
            vcs: None,
            user: None,
            workspace: None,
            effort: None,
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

    pub fn with_effort(mut self, effort: Effort) -> Self {
        self.effort = Some(effort);
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

        let filled = fixed()
            .with_vcs(Vcs {
                branch: "main".into(),
                head: "83ff18b".into(),
            })
            .with_user("Ari")
            .with_workspace("Zone")
            .with_effort(Effort::High);
        assert_eq!(filled.vcs.unwrap().branch, "main");
        assert_eq!(filled.user.as_deref(), Some("Ari"));
        assert_eq!(filled.workspace.as_deref(), Some("Zone"));
        assert_eq!(filled.effort, Some(Effort::High));
    }

    #[test]
    fn the_live_environment_names_a_timezone_and_a_directory() {
        let environment = Environment::here();
        assert!(!environment.timezone.is_empty());
        assert!(!environment.platform.is_empty());
        assert!(!environment.directory.as_os_str().is_empty());
    }
}
