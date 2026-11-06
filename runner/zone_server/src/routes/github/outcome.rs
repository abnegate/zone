//! Where the GitHub callbacks send the browser back to in Zone, and what they
//! tell the settings page happened.

use uuid::Uuid;

pub const SETTINGS_PATH: &str = "/settings/organization/github";

const OUTCOME_PARAMETER: &str = "github";
const ORGANIZATION_PARAMETER: &str = "organization";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    Requested,
    Sync,
    Expired,
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Requested => "requested",
            Self::Sync => "sync",
            Self::Expired => "expired",
            Self::Failed => "failed",
        }
    }
}

/// The organization is named only when the callback consumed a state, so the
/// settings page can switch to the organization the browser started from.
pub fn location(app_base_url: &str, outcome: Outcome, organization: Option<Uuid>) -> String {
    let base = app_base_url.trim().trim_end_matches('/');
    let outcome = outcome.as_str();
    match organization {
        Some(organization) => format!(
            "{base}{SETTINGS_PATH}?{OUTCOME_PARAMETER}={outcome}&{ORGANIZATION_PARAMETER}={organization}"
        ),
        None => format!("{base}{SETTINGS_PATH}?{OUTCOME_PARAMETER}={outcome}"),
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Url;

    use super::*;

    const BASE: &str = "https://zone.example.com";

    #[test]
    fn the_settings_path_is_the_organization_github_page_of_zone() {
        let url = Url::parse(&format!("{BASE}{SETTINGS_PATH}")).unwrap();
        assert_eq!(url.origin(), Url::parse(BASE).unwrap().origin());
        assert_eq!(
            url.path_segments().unwrap().collect::<Vec<_>>(),
            ["settings", "organization", "github"]
        );
        assert_eq!(url.query(), None);
    }

    #[test]
    fn the_outcome_opens_the_query() {
        let organization = Uuid::new_v4();
        let url = Url::parse(&location(BASE, Outcome::Installed, Some(organization))).unwrap();
        assert_eq!(url.path(), SETTINGS_PATH);
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs,
            [
                ("github".to_string(), "installed".to_string()),
                ("organization".to_string(), organization.to_string()),
            ]
        );
    }

    #[test]
    fn each_outcome_is_spelled_as_the_settings_page_reads_it() {
        for (outcome, spelled) in [
            (Outcome::Installed, "installed"),
            (Outcome::Requested, "requested"),
            (Outcome::Sync, "sync"),
            (Outcome::Expired, "expired"),
            (Outcome::Failed, "failed"),
        ] {
            assert_eq!(outcome.as_str(), spelled);
            assert_eq!(
                location(BASE, outcome, None),
                format!("{BASE}{SETTINGS_PATH}?github={spelled}")
            );
        }
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_dropped() {
        assert_eq!(
            location(" http://192.168.1.5:3000/ ", Outcome::Sync, None),
            format!("http://192.168.1.5:3000{SETTINGS_PATH}?github=sync")
        );
    }

    #[test]
    fn the_organization_is_named_only_when_given() {
        let organization = Uuid::new_v4();
        assert_eq!(
            location(BASE, Outcome::Installed, Some(organization)),
            format!("{BASE}{SETTINGS_PATH}?github=installed&organization={organization}")
        );
        assert!(!location(BASE, Outcome::Expired, None).contains("organization"));
    }
}
