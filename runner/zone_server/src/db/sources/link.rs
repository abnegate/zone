use zone_context::adapters::GitLabAdapter;

use crate::db::sources::GITHUB;
use crate::db::sources::GITLAB;
use crate::db::sources::ICAL;
use crate::db::sources::WEB;

const OWNER_FIELD: &str = "owner";

const REPOSITORY_FIELD: &str = "repo";

const URL_FIELD: &str = "url";

const GITHUB_ORIGIN: &str = "https://github.com";

/// The web page a source of `source_type` reading `config` is linked to,
/// when its config names one
pub fn derived_link(source_type: &str, config: &serde_json::Value) -> Option<String> {
    let named = |field: &str| {
        config
            .get(field)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
    };
    match source_type {
        GITHUB => Some(format!(
            "{GITHUB_ORIGIN}/{}/{}",
            named(OWNER_FIELD)?,
            named(REPOSITORY_FIELD)?
        )),
        GITLAB => GitLabAdapter::web_url(config),
        WEB | ICAL => named(URL_FIELD).map(ToString::to_string),
        _ => None,
    }
}
