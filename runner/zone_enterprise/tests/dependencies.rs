const ALLOWED: [&str; 7] = [
    "base64",
    "chrono",
    "ed25519-dalek",
    "serde",
    "serde_json",
    "thiserror",
    "uuid",
];

#[test]
fn depends_only_on_the_allowlist() {
    let manifest: toml::Table = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    let dependencies = manifest["dependencies"].as_table().unwrap();
    for name in dependencies.keys() {
        assert!(
            ALLOWED.contains(&name.as_str()),
            "{name} is not an allowed dependency of zone_enterprise"
        );
    }
}

#[test]
fn has_no_target_specific_or_build_dependencies() {
    let manifest: toml::Table = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    assert!(
        !manifest.contains_key("target"),
        "target-specific dependencies bypass the allowlist"
    );
    assert!(
        !manifest.contains_key("build-dependencies"),
        "build dependencies bypass the allowlist"
    );
}
