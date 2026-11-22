use uuid::Uuid;
use zone_enterprise::fleet::{Error, Labels, Version};

pub const NAME: &str = "ZONE_RUNNER_NAME";
pub const LABELS: &str = "ZONE_RUNNER_LABELS";

const HOSTNAME: &str = "HOSTNAME";
const HOSTNAME_FILE: &str = "/etc/hostname";
const FALLBACK_PREFIX: &str = "zone-";
const FALLBACK_LENGTH: usize = 8;

/// What this server process registers as a runner with. Its name carries the runner's state
/// across restarts, so it should be unique to the server and stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub version: Version,
    pub labels: Labels,
}

impl Identity {
    /// The host name, this build's version and no labels.
    pub fn detected() -> Self {
        Self {
            name: host(std::env::var(HOSTNAME).ok(), || {
                std::fs::read_to_string(HOSTNAME_FILE).ok()
            }),
            version: version(),
            labels: Labels::default(),
        }
    }

    /// [`Identity::detected`], with the name and labels the environment sets. Refuses labels
    /// that don't parse, so a misconfigured server can't boot into the wrong placement.
    pub fn from_env() -> Result<Self, Error> {
        Self::configured(
            std::env::var(NAME).ok().as_deref(),
            std::env::var(LABELS).ok().as_deref(),
        )
    }

    fn configured(name: Option<&str>, labels: Option<&str>) -> Result<Self, Error> {
        let labels = labels
            .map(Labels::parse_list)
            .transpose()?
            .unwrap_or_default();
        let detected = Self::detected();
        let name = name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or(detected.name, str::to_string);
        Ok(Self {
            name,
            labels,
            version: detected.version,
        })
    }
}

/// This build's version.
pub fn version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is a semantic version")
}

fn host(variable: Option<String>, file: impl FnOnce() -> Option<String>) -> String {
    named(variable)
        .or_else(|| named(file()))
        .unwrap_or_else(fallback)
}

fn named(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn fallback() -> String {
    let random = Uuid::new_v4().simple().to_string();
    format!("{FALLBACK_PREFIX}{}", &random[..FALLBACK_LENGTH])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_version_is_reported() {
        assert_eq!(version().to_string(), env!("CARGO_PKG_VERSION"));
        assert_eq!(Identity::detected().version, version());
    }

    #[test]
    fn a_detected_runner_carries_no_labels() {
        assert!(Identity::detected().labels.is_empty());
        assert!(!Identity::detected().name.is_empty());
    }

    #[test]
    fn the_host_name_variable_wins_over_the_file() {
        assert_eq!(
            host(Some(" pod-a \n".into()), || Some("file".into())),
            "pod-a"
        );
        assert_eq!(host(None, || Some("pod-b\n".into())), "pod-b");
        assert_eq!(host(Some("  ".into()), || Some("pod-c".into())), "pod-c");
    }

    #[test]
    fn without_a_host_name_the_name_is_random() {
        let name = host(None, || None);
        assert_eq!(name.len(), FALLBACK_PREFIX.len() + FALLBACK_LENGTH);
        assert!(name.starts_with(FALLBACK_PREFIX));
        assert!(
            name[FALLBACK_PREFIX.len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_ne!(name, host(None, || Some(String::new())));
    }

    #[test]
    fn the_environment_names_and_labels_the_runner() {
        let identity = Identity::configured(Some(" eu-1 "), Some(" gpu, region=eu,gpu ")).unwrap();
        assert_eq!(identity.name, "eu-1");
        assert_eq!(identity.labels.to_vec(), ["gpu", "region=eu"]);
        assert_eq!(identity.version, version());
    }

    #[test]
    fn an_empty_name_falls_back_to_the_detected_one() {
        let identity = Identity::configured(Some(" "), Some("")).unwrap();
        assert!(!identity.name.trim().is_empty());
        assert!(identity.labels.is_empty());
        assert!(Identity::configured(None, None).unwrap().labels.is_empty());
    }

    #[test]
    fn an_invalid_label_refuses_the_identity() {
        let refused = Identity::configured(Some("eu-1"), Some("gpu,GPU")).unwrap_err();
        assert_eq!(refused, Error::LabelInvalid("GPU".into()));
        assert_eq!(refused.code(), "fleet_label_invalid");
    }

    #[test]
    fn the_variables_are_the_documented_names() {
        assert_eq!(NAME, "ZONE_RUNNER_NAME");
        assert_eq!(LABELS, "ZONE_RUNNER_LABELS");
    }
}
