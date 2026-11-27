use std::fs;
use std::path::Path;

fn sources(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

const NEXTEST: &str = include_str!("../../../.config/nextest.toml");
const USE: &str = "use ";
const REPLICAS: &str = "common::replica";
const GROUP_START: &str = "test(/^(";
const GROUP_END: &str = ")::/)";

/// The integration files the nextest `replicas` group caps the concurrency of.
fn grouped() -> Vec<&'static str> {
    let start = NEXTEST
        .find(GROUP_START)
        .expect("the replicas override filters on test names")
        + GROUP_START.len();
    let end = start
        + NEXTEST[start..]
            .find(GROUP_END)
            .expect("the replicas filter closes its group");
    NEXTEST[start..end].split('|').collect()
}

#[test]
fn every_file_starting_replicas_runs_in_the_capped_nextest_group() {
    let integration = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration");
    let grouped = grouped();
    let uncapped: Vec<String> = sources(&integration)
        .into_iter()
        .filter(|name| {
            fs::read_to_string(integration.join(format!("{name}.rs")))
                .unwrap()
                .lines()
                .any(|line| line.starts_with(USE) && line.contains(REPLICAS))
        })
        .filter(|name| !grouped.contains(&name.as_str()))
        .collect();
    assert!(
        uncapped.is_empty(),
        "each process-per-test replica test holds several 20-connection pools; add these to the replicas group in runner/.config/nextest.toml: {uncapped:?}"
    );
}

#[test]
fn no_test_file_sits_outside_the_integration_target() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    assert_eq!(
        sources(&tests),
        Vec::<String>::new(),
        "autotests is off, so a file in tests/ never compiles; move it into tests/integration/ and declare it in main.rs"
    );
}

#[test]
fn every_integration_file_is_declared_in_main() {
    let main = include_str!("main.rs");
    let integration = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration");
    let undeclared: Vec<String> = sources(&integration)
        .into_iter()
        .filter(|name| name != "main" && !main.contains(&format!("mod {name};")))
        .collect();
    assert!(
        undeclared.is_empty(),
        "declare these in tests/integration/main.rs or they never compile: {undeclared:?}"
    );
}
