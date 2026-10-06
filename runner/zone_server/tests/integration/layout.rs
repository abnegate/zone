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
