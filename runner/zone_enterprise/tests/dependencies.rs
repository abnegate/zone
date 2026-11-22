const ALLOWED: [&str; 7] = [
    "base64",
    "chrono",
    "ed25519-dalek",
    "serde",
    "serde_json",
    "thiserror",
    "uuid",
];

const FEATURES: [(&str, &[&str]); 2] = [
    ("audit", &["hex", "hmac", "md-5", "sha2", "url"]),
    (
        "sso",
        &[
            "aws-lc-rs",
            "dashmap",
            "flate2",
            "jsonwebtoken",
            "quick-xml",
            "rand",
            "reqwest",
            "rustls-pki-types",
            "rustls-webpki",
            "sha2",
            "url",
        ],
    ),
];

const DEPENDENCY_PREFIX: &str = "dep:";

fn manifest() -> toml::Table {
    toml::from_str(include_str!("../Cargo.toml")).unwrap()
}

fn optional(specification: &toml::Value) -> bool {
    specification
        .get("optional")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

fn dependencies(optional_only: bool) -> Vec<String> {
    let manifest = manifest();
    manifest["dependencies"]
        .as_table()
        .unwrap()
        .iter()
        .filter(|(_, specification)| optional(specification) == optional_only)
        .map(|(name, _)| name.clone())
        .collect()
}

fn features() -> toml::Table {
    manifest()["features"].as_table().unwrap().clone()
}

fn entries(feature: &toml::Value) -> Vec<String> {
    feature
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.as_str().unwrap().to_owned())
        .collect()
}

fn dependency(entry: &str) -> String {
    entry
        .strip_prefix(DEPENDENCY_PREFIX)
        .unwrap_or(entry)
        .split(['/', '?'])
        .next()
        .unwrap()
        .to_owned()
}

fn listed(feature: &str) -> Option<&'static [&'static str]> {
    FEATURES
        .iter()
        .find(|(name, _)| *name == feature)
        .map(|(_, dependencies)| *dependencies)
}

#[test]
fn depends_only_on_the_allowlist() {
    for name in dependencies(false) {
        assert!(
            ALLOWED.contains(&name.as_str()),
            "{name} is not an allowed dependency of zone_enterprise"
        );
    }
}

#[test]
fn every_optional_dependency_is_listed_by_a_feature() {
    let mut optional = dependencies(true);
    optional.sort();
    let mut listed: Vec<String> = FEATURES
        .iter()
        .flat_map(|(_, dependencies)| dependencies.iter().map(|name| (*name).to_owned()))
        .collect();
    listed.sort();
    listed.dedup();
    assert_eq!(
        optional, listed,
        "optional dependencies must match the feature allowlists"
    );
}

#[test]
fn each_feature_enables_exactly_its_dependencies() {
    let features = features();
    for (name, dependencies) in FEATURES {
        let mut enabled = entries(&features[name]);
        enabled.sort();
        let expected: Vec<String> = dependencies
            .iter()
            .map(|dependency| format!("{DEPENDENCY_PREFIX}{dependency}"))
            .collect();
        assert_eq!(enabled, expected, "the {name} feature");
    }
}

#[test]
fn no_other_feature_enables_an_optional_dependency() {
    let optional = dependencies(true);
    for (name, feature) in features() {
        if listed(&name).is_some() {
            continue;
        }
        for entry in entries(&feature) {
            let dependency = dependency(&entry);
            assert!(
                !optional.contains(&dependency),
                "{name} enables {dependency}"
            );
        }
    }
}

#[test]
fn default_enables_nothing() {
    let features = features();
    let enabled = features.get("default").map(entries).unwrap_or_default();
    assert!(enabled.is_empty(), "default enables {enabled:?}");
}

#[test]
fn has_no_target_specific_or_build_dependencies() {
    let manifest = manifest();
    assert!(
        !manifest.contains_key("target"),
        "target-specific dependencies bypass the allowlist"
    );
    assert!(
        !manifest.contains_key("build-dependencies"),
        "build dependencies bypass the allowlist"
    );
}
