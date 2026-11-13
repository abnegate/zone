use abnegate_secret::SecretValue;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Configuration {
    pub(crate) owner: String,
    #[serde(rename = "repo")]
    pub(crate) repository: String,
    pub(crate) branch: Option<String>,
    pub(crate) path: Option<String>,
    #[serde(skip)]
    pub(crate) token: Option<SecretValue>,
    /// Which review bots count as review evidence for this source. Absent
    /// means every bot this build recognises.
    pub(crate) review_signals: Option<Vec<String>>,
}

impl Configuration {
    pub(crate) fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repository)
    }

    pub(crate) fn web_url(&self, origin: &str) -> String {
        format!("{}/{}", origin.trim_end_matches('/'), self.full_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::github::testing::OWNER;
    use crate::services::github::testing::REPOSITORY;
    use crate::services::github::testing::STORED_TOKEN;
    use serde_json::json;

    #[test]
    fn a_repository_is_named_and_linked_as_github_shows_it() {
        let configuration = Configuration {
            owner: OWNER.into(),
            repository: REPOSITORY.into(),
            branch: None,
            path: None,
            token: None,
            review_signals: None,
        };

        assert_eq!(configuration.full_name(), format!("{OWNER}/{REPOSITORY}"));
        for origin in ["https://github.example.com", "https://github.example.com/"] {
            assert_eq!(
                configuration.web_url(origin),
                format!("https://github.example.com/{OWNER}/{REPOSITORY}"),
                "{origin}"
            );
        }
    }

    #[test]
    fn a_configuration_never_prints_its_token() {
        let configuration = Configuration {
            owner: OWNER.into(),
            repository: REPOSITORY.into(),
            branch: None,
            path: None,
            token: Some(STORED_TOKEN.into()),
            review_signals: None,
        };

        let printed = format!("{configuration:?}");

        assert!(!printed.contains(STORED_TOKEN), "{printed}");
        assert!(printed.contains(OWNER), "{printed}");
    }

    #[test]
    fn a_token_in_the_stored_configuration_is_never_read_as_the_credential() {
        let configuration = Configuration::deserialize(&json!({
            "owner": OWNER,
            "repo": REPOSITORY,
            "token": STORED_TOKEN,
        }))
        .expect("the configuration parses");

        assert!(configuration.token.is_none());
    }

    #[test]
    fn the_stored_repo_key_is_read_as_the_repository() {
        let configuration = Configuration::deserialize(&json!({
            "owner": OWNER,
            "repo": REPOSITORY,
        }))
        .expect("the configuration parses");

        assert_eq!(configuration.repository, REPOSITORY);
        assert!(
            Configuration::deserialize(&json!({
                "owner": OWNER,
                "repository": REPOSITORY,
            }))
            .is_err(),
            "only the stored repo key names the repository"
        );
    }
}
