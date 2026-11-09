use zone_context::adapters::GitLabAdapter;

use crate::db::sources::GITHUB;
use crate::db::sources::GITLAB;
use crate::db::sources::ICAL;
use crate::db::sources::WEB;

const OWNER_FIELD: &str = "owner";

const REPOSITORY_FIELD: &str = "repo";

const URL_FIELD: &str = "url";

const PROJECT_ID_FIELD: &str = "project_id";

const HOST_FIELD: &str = "host";

const GITHUB_ORIGIN: &str = "https://github.com";

const GITLAB_ORIGIN: &str = "https://gitlab.com";

/// The web page a source of `source_type` reading `config` is linked to,
/// when its config names one
pub fn derived_link(source_type: &str, config: &serde_json::Value) -> Option<String> {
    match source_type {
        GITHUB => Some(format!(
            "{GITHUB_ORIGIN}/{}/{}",
            named(config, OWNER_FIELD)?,
            named(config, REPOSITORY_FIELD)?
        )),
        GITLAB => GitLabAdapter::web_url(config),
        WEB | ICAL => named(config, URL_FIELD).map(ToString::to_string),
        _ => None,
    }
}

/// The legacy GitLab link, which released code derived for a GitLab source's
/// `config`: its `project_id` appended to its `host` less any trailing
/// slashes, or to `https://gitlab.com` when it names no `host`
pub fn legacy_link(config: &serde_json::Value) -> Option<String> {
    let project = named(config, PROJECT_ID_FIELD)?;
    let host = named(config, HOST_FIELD)
        .unwrap_or(GITLAB_ORIGIN)
        .trim_end_matches('/');
    Some(format!("{host}/{project}"))
}

fn named<'a>(config: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    config
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
}
