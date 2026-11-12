//! The streamable-HTTP stub proves itself, so it still answers when a test first attaches it.

use std::path::PathBuf;
use std::process::Command;

const PYTHON: &str = "python3";
const ISOLATED: &str = "-I";
const SELF_TEST: &str = "--self-test";
const TOKEN: &str = "--token";
const KEY: &str = "--key";
const SECRET: &str = "secret";
const PASSED: &str = "self-test passed";

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_mcp_http_server.py")
}

fn passes(arguments: &[&str]) {
    let output = Command::new(PYTHON)
        .arg(ISOLATED)
        .arg(stub())
        .args(arguments)
        .output()
        .expect("python3 runs the stub");
    let report = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "{arguments:?}: {report}");
    assert_eq!(report.trim(), PASSED, "{arguments:?}");
}

#[test]
fn the_http_stub_passes_its_self_test() {
    passes(&[SELF_TEST]);
}

#[test]
fn the_http_stub_passes_its_self_test_behind_a_token() {
    passes(&[TOKEN, SECRET, SELF_TEST]);
}

#[test]
fn the_http_stub_passes_its_self_test_behind_a_key() {
    passes(&[KEY, SECRET, SELF_TEST]);
}
