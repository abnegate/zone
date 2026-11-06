use serde::Serialize;

use super::login::Login;
use super::permissions::Permissions;

pub const MANIFEST_CALLBACK_PATH: &str = "/api/github/callbacks/manifest";
pub const SETUP_CALLBACK_PATH: &str = "/api/github/callbacks/setup";
pub const MAXIMUM_NAME_LENGTH: usize = 34;

const NAME_PREFIX: &str = "Zone";
const SUFFIX_LENGTH: usize = 4;
const SUFFIX_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

const ORGANIZATIONS_PATH: &str = "organizations";
const APPS_SETTINGS_PATH: &str = "settings/apps";
const NEW_APP_PATH: &str = "new";
const NEW_INSTALLATION_PATH: &str = "installations/new";
const STATE_PARAMETER: &str = "state";

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    name: String,
    url: String,
    redirect_url: String,
    setup_url: String,
    setup_on_update: bool,
    request_oauth_on_install: bool,
    public: bool,
    default_permissions: Permissions,
}

impl Manifest {
    pub fn new(app_base_url: &str, organization_slug: &str) -> Self {
        Self::named(app_base_url, organization_slug, &random_suffix())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a manifest of strings and booleans always serializes")
    }

    fn named(app_base_url: &str, organization_slug: &str, suffix: &str) -> Self {
        let base = app_base_url.trim().trim_end_matches('/');
        Self {
            name: name(organization_slug, suffix),
            url: base.to_string(),
            redirect_url: format!("{base}{MANIFEST_CALLBACK_PATH}"),
            setup_url: format!("{base}{SETUP_CALLBACK_PATH}"),
            setup_on_update: true,
            request_oauth_on_install: false,
            public: false,
            default_permissions: Permissions::zone(),
        }
    }
}

pub fn registration_url(web: &str, owner: Option<&Login>, state: &str) -> String {
    format!(
        "{}/{NEW_APP_PATH}?{STATE_PARAMETER}={}",
        apps_settings(web, owner),
        urlencoding::encode(state)
    )
}

pub fn installation_url(html_url: &str, state: &str) -> String {
    format!(
        "{}/{NEW_INSTALLATION_PATH}?{STATE_PARAMETER}={}",
        html_url.trim().trim_end_matches('/'),
        urlencoding::encode(state)
    )
}

pub fn settings_url(web: &str, owner: Option<&Login>, slug: &str) -> String {
    format!(
        "{}/{}",
        apps_settings(web, owner),
        urlencoding::encode(slug)
    )
}

fn apps_settings(web: &str, owner: Option<&Login>) -> String {
    let web = web.trim().trim_end_matches('/');
    match owner {
        Some(organization) => {
            format!("{web}/{ORGANIZATIONS_PATH}/{organization}/{APPS_SETTINGS_PATH}")
        }
        None => format!("{web}/{APPS_SETTINGS_PATH}"),
    }
}

fn name(organization_slug: &str, suffix: &str) -> String {
    let reserved = NAME_PREFIX.len() + suffix.chars().count() + 2;
    let slug: String = organization_slug
        .trim()
        .chars()
        .take(MAXIMUM_NAME_LENGTH.saturating_sub(reserved))
        .collect();
    let slug = slug.trim_end_matches(['-', ' ']);
    match slug.is_empty() {
        true => format!("{NAME_PREFIX} {suffix}"),
        false => format!("{NAME_PREFIX} {slug} {suffix}"),
    }
}

fn random_suffix() -> String {
    (0..SUFFIX_LENGTH)
        .map(|_| char::from(SUFFIX_ALPHABET[rand::random_range(0..SUFFIX_ALPHABET.len())]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn the_manifest_registers_a_private_app_with_no_webhook() {
        let manifest = Manifest::named(" http://192.168.1.5:3000/ ", "acme", "x7k2");
        let snapshot: Value = serde_json::from_str(&manifest.to_json()).unwrap();
        assert_eq!(
            snapshot,
            json!({
                "name": "Zone acme x7k2",
                "url": "http://192.168.1.5:3000",
                "redirect_url": "http://192.168.1.5:3000/api/github/callbacks/manifest",
                "setup_url": "http://192.168.1.5:3000/api/github/callbacks/setup",
                "setup_on_update": true,
                "request_oauth_on_install": false,
                "public": false,
                "default_permissions": {
                    "contents": "write",
                    "pull_requests": "write",
                    "metadata": "read",
                    "checks": "read",
                    "actions": "read",
                    "issues": "write",
                    "workflows": "write",
                    "statuses": "read",
                    "deployments": "read",
                    "repository_hooks": "write",
                },
            })
        );
        for absent in ["hook_attributes", "default_events", "callback_urls"] {
            assert!(
                snapshot.get(absent).is_none(),
                "{absent} must be omitted, or GitHub requires a reachable webhook"
            );
        }
    }

    #[test]
    fn a_long_slug_is_truncated_to_fit_the_name_limit() {
        let slug = "a".repeat(60);
        let manifest = Manifest::new("https://zone.example.com", &slug);
        let name = manifest.name();
        assert_eq!(
            name.len(),
            MAXIMUM_NAME_LENGTH,
            "{name:?} should fill, and never exceed, the length GitHub allows"
        );
        let mut words = name.split(' ');
        assert_eq!(words.next(), Some("Zone"));
        assert!(
            words
                .next()
                .is_some_and(|slug| slug.chars().all(|character| character == 'a'))
        );
        let suffix = words.next().expect("a random suffix");
        assert_eq!(suffix.len(), SUFFIX_LENGTH);
        assert!(suffix.bytes().all(|byte| SUFFIX_ALPHABET.contains(&byte)));
        assert_eq!(words.next(), None);
    }

    #[test]
    fn a_truncated_slug_never_ends_in_a_separator() {
        let slug = format!("{}-tail", "b".repeat(23));
        assert_eq!(name(&slug, "x7k2"), format!("Zone {} x7k2", "b".repeat(23)));
        assert_eq!(name("   ", "x7k2"), "Zone x7k2");
    }

    #[test]
    fn two_manifests_for_one_organization_get_distinct_names() {
        let names: std::collections::HashSet<String> = (0..16)
            .map(|_| {
                Manifest::new("https://zone.example.com", "acme")
                    .name()
                    .to_string()
            })
            .collect();
        assert!(
            names.len() > 1,
            "the suffix must vary between registrations"
        );
    }

    #[test]
    fn registration_posts_to_the_owners_new_app_page() {
        let state = "a+b/c=d&e";
        let organization = Login::parse("acme-labs").unwrap();
        assert_eq!(
            registration_url("https://github.com/", None, state),
            "https://github.com/settings/apps/new?state=a%2Bb%2Fc%3Dd%26e"
        );
        assert_eq!(
            registration_url("https://ghe.local", Some(&organization), state),
            "https://ghe.local/organizations/acme-labs/settings/apps/new?state=a%2Bb%2Fc%3Dd%26e"
        );
    }

    #[test]
    fn installation_starts_from_the_apps_public_page() {
        assert_eq!(
            installation_url("https://github.com/apps/zone-acme-x7k2/", "a+b/c=d&e"),
            "https://github.com/apps/zone-acme-x7k2/installations/new?state=a%2Bb%2Fc%3Dd%26e"
        );
    }

    #[test]
    fn settings_are_under_the_owners_apps() {
        let organization = Login::parse("acme").unwrap();
        assert_eq!(
            settings_url("https://github.com", None, "zone-acme-x7k2"),
            "https://github.com/settings/apps/zone-acme-x7k2"
        );
        assert_eq!(
            settings_url("http://192.168.1.5/", Some(&organization), "zone-acme-x7k2"),
            "http://192.168.1.5/organizations/acme/settings/apps/zone-acme-x7k2"
        );
    }
}
