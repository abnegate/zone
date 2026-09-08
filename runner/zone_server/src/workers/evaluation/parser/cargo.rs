//! Turning Cargo tool output into counts and diagnostics.

use std::collections::HashSet;

use serde_json::Value;

use crate::workers::evaluation::category::EvalCategory;
use crate::workers::evaluation::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::workers::evaluation::snapshot::EvalSnapshot;

const SUMMARY_PREFIX: &str = "test result:";
const FAILURE_SUFFIX: &str = " ... FAILED";
const TEST_PREFIX: &str = "test ";
const DIFF_PREFIX: &str = "Diff in ";
const DIFF_SEPARATOR: &str = " at line ";

/// `cargo test` prints one human-readable summary line per test binary.
pub fn parse_test(stdout: &str, stderr: &str) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(EvalCategory::Test);

    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if let Some(summary) = line.strip_prefix(SUMMARY_PREFIX) {
            for (count, label) in summary_counts(summary) {
                match label {
                    "passed" => snapshot.passed += count,
                    "failed" => snapshot.failed += count,
                    "ignored" => snapshot.skipped += count,
                    _ => {}
                }
            }
        } else if let Some(name) = line
            .strip_prefix(TEST_PREFIX)
            .and_then(|rest| rest.strip_suffix(FAILURE_SUFFIX))
        {
            snapshot.diagnostics.push(Diagnostic::new(
                name.trim(),
                DiagnosticSeverity::Error,
                format!("test {} failed", name.trim()),
            ));
        }
    }

    snapshot.errors = snapshot.failed;
    snapshot
}

fn summary_counts(summary: &str) -> Vec<(u32, &str)> {
    summary
        .split(';')
        .filter_map(|segment| {
            let tokens: Vec<&str> = segment.split_whitespace().collect();
            tokens.windows(2).find_map(|pair| {
                pair[0]
                    .parse::<u32>()
                    .ok()
                    .map(|count| (count, pair[1].trim_end_matches('.')))
            })
        })
        .collect()
}

/// `cargo check` and `cargo clippy` share the `--message-format json` stream.
pub fn parse_diagnostics(stdout: &str, stderr: &str, category: EvalCategory) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(category);
    let mut seen: HashSet<Diagnostic> = HashSet::new();

    for line in stdout.lines().chain(stderr.lines()) {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        let Some(diagnostic) = compiler_message(message) else {
            continue;
        };
        if seen.insert(diagnostic.clone()) {
            snapshot.push(diagnostic);
        }
    }

    snapshot
}

fn compiler_message(message: &Value) -> Option<Diagnostic> {
    let text = message.get("message").and_then(Value::as_str)?;
    let severity = match message.get("level").and_then(Value::as_str)? {
        "error" | "error: internal compiler error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    };
    let span = message
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| spans.iter().find(|span| is_primary(span)).or(spans.first()));

    if span.is_none() && is_rollup(text) {
        return None;
    }

    let file = span
        .and_then(|span| span.get("file_name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let line = span
        .and_then(|span| span.get("line_start"))
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    let column = span
        .and_then(|span| span.get("column_start"))
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    let code = message
        .get("code")
        .and_then(|code| code.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(
        Diagnostic::new(file, severity, text)
            .at(line, column)
            .with_code(code),
    )
}

fn is_primary(span: &Value) -> bool {
    span.get("is_primary").and_then(Value::as_bool) == Some(true)
}

/// Spanless tallies that restate diagnostics already counted individually.
fn is_rollup(text: &str) -> bool {
    text.starts_with("aborting due to")
        || text.starts_with("For more information about")
        || text.starts_with("Some errors have detailed explanations")
        || text.ends_with("warning emitted")
        || text.ends_with("warnings emitted")
}

/// `cargo fmt --check` reports one diff header per badly formatted region.
pub fn parse_format(stdout: &str, stderr: &str) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(EvalCategory::Lint);

    for line in stdout.lines().chain(stderr.lines()) {
        let Some(rest) = line.trim().strip_prefix(DIFF_PREFIX) else {
            continue;
        };
        let (file, remainder) = match rest.split_once(DIFF_SEPARATOR) {
            Some((file, remainder)) => (file, Some(remainder)),
            None => (rest, None),
        };
        let line_number = remainder
            .and_then(|remainder| remainder.trim_end_matches(':').trim().parse::<u32>().ok());

        snapshot.push(
            Diagnostic::new(file, DiagnosticSeverity::Warning, "file is not formatted")
                .at(line_number, None),
        );
    }

    snapshot
}

/// `cargo llvm-cov --json` emits the llvm-cov export document.
pub fn parse_coverage(stdout: &str, _stderr: &str) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(EvalCategory::Coverage);

    let Ok(document) = serde_json::from_str::<Value>(stdout.trim()) else {
        return snapshot;
    };
    snapshot.line_coverage_percent = document
        .pointer("/data/0/totals/lines/percent")
        .and_then(Value::as_f64);

    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_OUTPUT: &str = "\nrunning 3 tests\ntest workers::evaluation::detector::tests::detects_a_bare_cargo_project ... ok\ntest workers::evaluation::delta::tests::reports_a_regression ... FAILED\ntest workers::evaluation::delta::tests::skipped_case ... ignored\n\nfailures:\n\n---- workers::evaluation::delta::tests::reports_a_regression stdout ----\nthread 'main' panicked at src/workers/evaluation/delta.rs:20:9:\nassertion `left == right` failed\n\nfailures:\n    workers::evaluation::delta::tests::reports_a_regression\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

    #[test]
    fn counts_a_cargo_test_summary_line() {
        let snapshot = parse_test(TEST_OUTPUT, "");

        assert_eq!(snapshot.passed, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.skipped, 1);
        assert_eq!(snapshot.errors, 1);
    }

    #[test]
    fn names_the_tests_that_failed() {
        let snapshot = parse_test(TEST_OUTPUT, "");

        assert_eq!(snapshot.diagnostics.len(), 1);
        assert_eq!(
            snapshot.diagnostics[0].file,
            "workers::evaluation::delta::tests::reports_a_regression"
        );
        assert_eq!(snapshot.diagnostics[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn sums_the_summary_of_every_test_binary() {
        let output = "test result: ok. 615 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.20s\ntest result: ok. 42 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.40s\n";

        let snapshot = parse_test(output, "");

        assert_eq!(snapshot.passed, 657);
        assert_eq!(snapshot.failed, 0);
        assert_eq!(snapshot.skipped, 2);
    }

    #[test]
    fn reads_a_summary_that_arrives_on_stderr() {
        let snapshot = parse_test(
            "",
            "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
        );
        assert_eq!(snapshot.passed, 7);
    }

    #[test]
    fn returns_zeroes_for_output_with_no_summary() {
        let snapshot = parse_test("error: could not compile `zone_server`\n", "");
        assert_eq!(snapshot.passed, 0);
        assert_eq!(snapshot.failed, 0);
        assert!(snapshot.diagnostics.is_empty());
    }

    const CLIPPY_OUTPUT: &str = concat!(
        r#"{"reason":"compiler-artifact","target":{"name":"zone_server"}}"#,
        "\n",
        r#"{"reason":"compiler-message","message":{"message":"unused variable: `total`","code":{"code":"unused_variables"},"level":"warning","spans":[{"file_name":"src/workers/evaluation/delta.rs","line_start":42,"column_start":9,"is_primary":true}]}}"#,
        "\n",
        r#"{"reason":"compiler-message","message":{"message":"mismatched types","code":{"code":"E0308"},"level":"error","spans":[{"file_name":"src/workers/evaluation/detector.rs","line_start":7,"column_start":5,"is_primary":true}]}}"#,
        "\n",
        r#"{"reason":"compiler-message","message":{"message":"aborting due to 1 previous error","level":"error","spans":[]}}"#,
        "\n",
        r#"{"reason":"build-finished","success":false}"#,
        "\n",
    );

    #[test]
    fn counts_cargo_json_diagnostics_by_severity() {
        let snapshot = parse_diagnostics(CLIPPY_OUTPUT, "", EvalCategory::Lint);

        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.warnings, 1);
        assert_eq!(snapshot.diagnostics.len(), 2);
        assert_eq!(snapshot.category, EvalCategory::Lint);
    }

    #[test]
    fn keeps_the_span_and_code_of_a_cargo_diagnostic() {
        let snapshot = parse_diagnostics(CLIPPY_OUTPUT, "", EvalCategory::Typecheck);
        let error = snapshot
            .diagnostics
            .iter()
            .find(|d| d.severity == DiagnosticSeverity::Error)
            .expect("the error diagnostic survives parsing");

        assert_eq!(error.location(), "src/workers/evaluation/detector.rs:7:5");
        assert_eq!(error.code.as_deref(), Some("E0308"));
        assert_eq!(error.message, "mismatched types");
    }

    #[test]
    fn drops_the_spanless_rollup_lines_cargo_appends() {
        let snapshot = parse_diagnostics(CLIPPY_OUTPUT, "", EvalCategory::Lint);

        assert!(
            !snapshot
                .diagnostics
                .iter()
                .any(|d| d.message.starts_with("aborting due to")),
            "the rollup restates errors that were already counted"
        );
    }

    #[test]
    fn drops_every_flavour_of_rollup_line() {
        for text in [
            "aborting due to 3 previous errors",
            "For more information about this error, try `rustc --explain E0308`.",
            "Some errors have detailed explanations: E0308, E0433.",
            "1 warning emitted",
            "12 warnings emitted",
        ] {
            let record = serde_json::json!({
                "reason": "compiler-message",
                "message": { "message": text, "level": "warning", "spans": [] },
            });
            let snapshot = parse_diagnostics(&record.to_string(), "", EvalCategory::Lint);
            assert!(snapshot.diagnostics.is_empty(), "{text} should be dropped");
        }
    }

    #[test]
    fn collapses_a_diagnostic_repeated_across_targets() {
        let record = r#"{"reason":"compiler-message","message":{"message":"unused import","code":{"code":"unused_imports"},"level":"warning","spans":[{"file_name":"src/lib.rs","line_start":3,"column_start":5,"is_primary":true}]}}"#;
        let output = format!("{record}\n{record}\n{record}\n");

        let snapshot = parse_diagnostics(&output, "", EvalCategory::Lint);

        assert_eq!(
            snapshot.warnings, 1,
            "--all-targets reports the same finding once per target"
        );
        assert_eq!(snapshot.diagnostics.len(), 1);
    }

    #[test]
    fn prefers_the_primary_span() {
        let record = r#"{"reason":"compiler-message","message":{"message":"mismatched types","level":"error","spans":[{"file_name":"src/other.rs","line_start":1,"column_start":1,"is_primary":false},{"file_name":"src/real.rs","line_start":9,"column_start":2,"is_primary":true}]}}"#;

        let snapshot = parse_diagnostics(record, "", EvalCategory::Typecheck);

        assert_eq!(snapshot.diagnostics[0].location(), "src/real.rs:9:2");
    }

    #[test]
    fn ignores_lines_that_are_not_json() {
        let snapshot = parse_diagnostics(
            "warning: unused\nnot json at all\n{}\n",
            "",
            EvalCategory::Lint,
        );
        assert!(snapshot.diagnostics.is_empty());
    }

    #[test]
    fn counts_unformatted_files() {
        let output = "Diff in /repo/src/main.rs at line 12:\n-let x=1;\n+let x = 1;\nDiff in /repo/src/lib.rs at line 40:\n";

        let snapshot = parse_format(output, "");

        assert_eq!(snapshot.warnings, 2);
        assert_eq!(snapshot.diagnostics[0].file, "/repo/src/main.rs");
        assert_eq!(snapshot.diagnostics[0].line, Some(12));
        assert_eq!(snapshot.diagnostics[1].line, Some(40));
    }

    #[test]
    fn reports_a_clean_format_check_as_no_findings() {
        let snapshot = parse_format("", "");
        assert_eq!(snapshot.findings(), 0);
        assert!(snapshot.diagnostics.is_empty());
    }

    #[test]
    fn reads_line_coverage_from_the_llvm_export() {
        let output = r#"{"type":"llvm.coverage.json.export","version":"2.0.1","data":[{"totals":{"lines":{"count":1000,"covered":824,"percent":82.4},"branches":{"percent":71.5}}}]}"#;

        let snapshot = parse_coverage(output, "");

        assert_eq!(snapshot.line_coverage_percent, Some(82.4));
        assert_eq!(snapshot.category, EvalCategory::Coverage);
    }

    #[test]
    fn tolerates_coverage_output_that_is_not_json() {
        assert!(
            parse_coverage("error: cargo-llvm-cov is not installed\n", "")
                .line_coverage_percent
                .is_none()
        );
    }
}
