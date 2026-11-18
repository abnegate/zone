const ALLOWED: [&str; 7] = [
    "base64",
    "chrono",
    "ed25519-dalek",
    "serde",
    "serde_json",
    "thiserror",
    "uuid",
];

const SSO: [&str; 6] = ["dashmap", "jsonwebtoken", "rand", "reqwest", "sha2", "url"];

const SSO_FEATURE: &str = "sso";

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
fn every_optional_dependency_is_for_single_sign_on() {
    let mut optional = dependencies(true);
    optional.sort();
    assert_eq!(
        optional, SSO,
        "optional dependencies must match the sso allowlist"
    );
}

#[test]
fn only_the_sso_feature_enables_optional_dependencies() {
    let optional = dependencies(true);
    for (name, feature) in features() {
        for entry in entries(&feature) {
            let dependency = entry
                .strip_prefix(DEPENDENCY_PREFIX)
                .unwrap_or(&entry)
                .split(['/', '?'])
                .next()
                .unwrap()
                .to_owned();
            if optional.contains(&dependency) {
                assert_eq!(name, SSO_FEATURE, "{name} enables {dependency}");
            }
        }
    }
}

#[test]
fn the_sso_feature_enables_exactly_its_dependencies() {
    let mut enabled = entries(&features()[SSO_FEATURE]);
    enabled.sort();
    let expected: Vec<String> = SSO
        .iter()
        .map(|name| format!("{DEPENDENCY_PREFIX}{name}"))
        .collect();
    assert_eq!(enabled, expected);
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
