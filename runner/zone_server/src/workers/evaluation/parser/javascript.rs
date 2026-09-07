//! Turning Bun, Vitest, Jest, Biome, ESLint and TypeScript output into counts.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::workers::evaluation::category::EvalCategory;
use crate::workers::evaluation::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::workers::evaluation::snapshot::EvalSnapshot;

/// How far past a Biome header its severity marker is looked for.
const MARKER_LOOKAHEAD: usize = 6;

const ERROR_MARKERS: [char; 2] = ['\u{2716}', '\u{00d7}'];
const WARNING_MARKER: char = '\u{26a0}';

static BUN_COUNTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?P<count>\d+)\s+(?P<label>pass|fail|skip|todo)\s*$")
        .expect("bun counter pattern compiles")
});

static REPORTER_COUNTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<count>\d+)\s+(?P<label>passed|failed|skipped|pending|todo)")
        .expect("reporter counter pattern compiles")
});

static TYPESCRIPT_DIAGNOSTIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<file>\S[^(]*)\((?P<line>\d+),(?P<column>\d+)\):\s+(?P<severity>error|warning)\s+(?P<code>[A-Z]+\d+):\s+(?P<message>.+)$",
    )
    .expect("typescript diagnostic pattern compiles")
});

static ESLINT_DIAGNOSTIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s+(?P<line>\d+):(?P<column>\d+)\s+(?P<severity>error|warning)\s+(?P<message>.+?)(?:\s{2,}(?P<rule>[\w@/.-]+))?\s*$",
    )
    .expect("eslint diagnostic pattern compiles")
});

static BIOME_DIAGNOSTIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<file>[^\s:]+):(?P<line>\d+):(?P<column>\d+)\s+(?P<rule>[a-z][\w/.-]*)\s")
        .expect("biome diagnostic pattern compiles")
});

static FOUND_SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<count>\d+)\s+(?P<label>errors?|warnings?)").expect("summary pattern compiles")
});

pub fn parse_test(stdout: &str, stderr: &str) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(EvalCategory::Test);
    let mut reporter = (0u32, 0u32, 0u32);
    let mut saw_bun_counters = false;

    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(captured) = BUN_COUNTER.captures(line) {
            let count: u32 = captured["count"].parse().unwrap_or_default();
            saw_bun_counters = true;
            match &captured["label"] {
                "pass" => snapshot.passed += count,
                "fail" => snapshot.failed += count,
                _ => snapshot.skipped += count,
            }
            continue;
        }
        if line.trim_start().starts_with("Tests") {
            for captured in REPORTER_COUNTER.captures_iter(line) {
                let count: u32 = captured["count"].parse().unwrap_or_default();
                match &captured["label"] {
                    "passed" => reporter.0 += count,
                    "failed" => reporter.1 += count,
                    _ => reporter.2 += count,
                }
            }
            continue;
        }
        if let Some(name) = failing_test_name(line) {
            snapshot.diagnostics.push(Diagnostic::new(
                name,
                DiagnosticSeverity::Error,
                format!("test {name} failed"),
            ));
        }
    }

    if !saw_bun_counters {
        snapshot.passed = reporter.0;
        snapshot.failed = reporter.1;
        snapshot.skipped = reporter.2;
    }
    snapshot.errors = snapshot.failed;
    snapshot
}

fn failing_test_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    trimmed
        .strip_prefix("(fail)")
        .or_else(|| trimmed.strip_prefix('\u{2717}'))
        .or_else(|| trimmed.strip_prefix('\u{00d7}'))
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
}

pub fn parse_diagnostics(stdout: &str, stderr: &str, category: EvalCategory) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(category);
    let lines: Vec<&str> = stdout.lines().chain(stderr.lines()).collect();
    let mut current_file: Option<&str> = None;

    for (index, line) in lines.iter().enumerate() {
        if let Some(captured) = TYPESCRIPT_DIAGNOSTIC.captures(line) {
            snapshot.push(
                Diagnostic::new(
                    captured["file"].trim(),
                    severity_of(&captured["severity"]),
                    captured["message"].trim(),
                )
                .at(number(&captured["line"]), number(&captured["column"]))
                .with_code(Some(captured["code"].to_string())),
            );
            continue;
        }
        if let Some(captured) = BIOME_DIAGNOSTIC.captures(line) {
            let (severity, message) = marker_after(&lines, index);
            snapshot.push(
                Diagnostic::new(captured["file"].trim(), severity, message)
                    .at(number(&captured["line"]), number(&captured["column"]))
                    .with_code(Some(captured["rule"].to_string())),
            );
            continue;
        }
        if let Some(file) = current_file
            && let Some(captured) = ESLINT_DIAGNOSTIC.captures(line)
        {
            snapshot.push(
                Diagnostic::new(
                    file,
                    severity_of(&captured["severity"]),
                    captured["message"].trim(),
                )
                .at(number(&captured["line"]), number(&captured["column"]))
                .with_code(captured.name("rule").map(|rule| rule.as_str().to_string())),
            );
            continue;
        }
        current_file = eslint_file_heading(line);
    }

    if snapshot.diagnostics.is_empty() {
        apply_summary(&mut snapshot, &lines);
    }
    snapshot
}

fn eslint_file_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_end();
    let looks_like_a_path = !trimmed.is_empty()
        && !trimmed.starts_with(char::is_whitespace)
        && (trimmed.starts_with('/') || trimmed.starts_with('.'))
        && trimmed.contains('.');
    looks_like_a_path.then_some(trimmed)
}

fn marker_after(lines: &[&str], index: usize) -> (DiagnosticSeverity, String) {
    let end = lines.len().min(index + 1 + MARKER_LOOKAHEAD);
    for line in &lines[index + 1..end] {
        if let Some((_, message)) = line.split_once(|c| ERROR_MARKERS.contains(&c)) {
            return (DiagnosticSeverity::Error, message.trim().to_string());
        }
        if let Some((_, message)) = line.split_once(WARNING_MARKER) {
            return (DiagnosticSeverity::Warning, message.trim().to_string());
        }
    }
    (DiagnosticSeverity::Error, "lint rule violated".to_string())
}

fn apply_summary(snapshot: &mut EvalSnapshot, lines: &[&str]) {
    for line in lines {
        let trimmed = line.trim();
        let is_summary = trimmed.starts_with("Found ")
            || trimmed.contains("problems (")
            || trimmed.contains("problem (");
        if !is_summary {
            continue;
        }
        for captured in FOUND_SUMMARY.captures_iter(trimmed) {
            let count: u32 = captured["count"].parse().unwrap_or_default();
            if captured["label"].starts_with("error") {
                snapshot.errors += count;
            } else {
                snapshot.warnings += count;
            }
        }
    }
}

fn severity_of(raw: &str) -> DiagnosticSeverity {
    match raw {
        "error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    }
}

fn number(raw: &str) -> Option<u32> {
    raw.parse().ok()
}

pub fn parse_coverage(stdout: &str, stderr: &str) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(EvalCategory::Coverage);

    if let Ok(document) = serde_json::from_str::<Value>(stdout.trim())
        && let Some(percent) = document.pointer("/total/lines/pct").and_then(Value::as_f64)
    {
        snapshot.line_coverage_percent = Some(percent);
        return snapshot;
    }
    snapshot.line_coverage_percent = coverage_table(stdout).or_else(|| coverage_table(stderr));
    snapshot
}

/// Bun and Istanbul both print a pipe-separated table whose `% Lines` column
/// sits at a different index, so the header decides which field to read.
fn coverage_table(output: &str) -> Option<f64> {
    let mut column = None;
    for line in output.lines() {
        if !line.contains('|') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if column.is_none() {
            column = fields.iter().position(|field| field.contains("% Lines"));
            continue;
        }
        if fields.first().is_some_and(|first| *first == "All files") {
            return column
                .and_then(|index| fields.get(index))
                .and_then(|field| field.parse::<f64>().ok());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUN_TEST_OUTPUT: &str = "bun test v1.4.0\n\nsrc/lib/format.test.ts:\n(pass) formats a date [0.42ms]\n(pass) formats a duration [0.11ms]\n\nsrc/lib/parse.test.ts:\n(fail) parses an empty string [1.20ms]\n\n 2 pass\n 1 fail\n 1 skip\n 4 expect() calls\nRan 4 tests across 2 files. [120.00ms]\n";

    #[test]
    fn counts_bun_test_results() {
        let snapshot = parse_test(BUN_TEST_OUTPUT, "");

        assert_eq!(snapshot.passed, 2);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.skipped, 1);
        assert_eq!(snapshot.errors, 1);
    }

    #[test]
    fn names_the_bun_test_that_failed() {
        let snapshot = parse_test(BUN_TEST_OUTPUT, "");

        assert_eq!(snapshot.diagnostics.len(), 1);
        assert_eq!(
            snapshot.diagnostics[0].file,
            "parses an empty string [1.20ms]"
        );
    }

    #[test]
    fn sums_counters_across_a_filtered_workspace_run() {
        let output = " 12 pass\n 0 fail\nRan 12 tests across 3 files.\n 40 pass\n 2 fail\nRan 42 tests across 9 files.\n";

        let snapshot = parse_test(output, "");

        assert_eq!(snapshot.passed, 52);
        assert_eq!(snapshot.failed, 2);
    }

    #[test]
    fn counts_vitest_results() {
        let output = " Test Files  1 failed | 2 passed (3)\n      Tests  2 failed | 10 passed | 1 skipped (13)\n";

        let snapshot = parse_test(output, "");

        assert_eq!(snapshot.passed, 10);
        assert_eq!(snapshot.failed, 2);
        assert_eq!(snapshot.skipped, 1);
    }

    #[test]
    fn counts_jest_results() {
        let output = "Test Suites: 1 failed, 4 passed, 5 total\nTests:       3 failed, 27 passed, 30 total\n";

        let snapshot = parse_test(output, "");

        assert_eq!(snapshot.passed, 27);
        assert_eq!(snapshot.failed, 3);
    }

    #[test]
    fn prefers_bun_counters_when_both_shapes_appear() {
        let output = " 5 pass\n 0 fail\nTests:       3 failed, 27 passed, 30 total\n";

        let snapshot = parse_test(output, "");

        assert_eq!(snapshot.passed, 5);
        assert_eq!(snapshot.failed, 0);
    }

    #[test]
    fn returns_zeroes_for_test_output_it_does_not_recognise() {
        let snapshot = parse_test("error: script \"test\" exited with code 1\n", "");
        assert_eq!(snapshot.passed, 0);
        assert_eq!(snapshot.failed, 0);
    }

    const TYPESCRIPT_OUTPUT: &str = "src/routes/tasks.ts(42,17): error TS2322: Type 'string' is not assignable to type 'number'.\nsrc/lib/query.ts(8,3): error TS2554: Expected 2 arguments, but got 1.\n\nFound 2 errors in 2 files.\n";

    #[test]
    fn counts_typescript_errors() {
        let snapshot = parse_diagnostics(TYPESCRIPT_OUTPUT, "", EvalCategory::Typecheck);

        assert_eq!(snapshot.errors, 2);
        assert_eq!(snapshot.warnings, 0);
        assert_eq!(snapshot.diagnostics.len(), 2);
    }

    #[test]
    fn keeps_the_span_and_code_of_a_typescript_error() {
        let snapshot = parse_diagnostics(TYPESCRIPT_OUTPUT, "", EvalCategory::Typecheck);

        assert_eq!(
            snapshot.diagnostics[0].location(),
            "src/routes/tasks.ts:42:17"
        );
        assert_eq!(snapshot.diagnostics[0].code.as_deref(), Some("TS2322"));
        assert!(snapshot.diagnostics[0].message.starts_with("Type 'string'"));
    }

    #[test]
    fn does_not_double_count_the_typescript_summary_line() {
        let snapshot = parse_diagnostics(TYPESCRIPT_OUTPUT, "", EvalCategory::Typecheck);
        assert_eq!(
            snapshot.errors, 2,
            "the per-line diagnostics already cover what the summary restates"
        );
    }

    const BIOME_OUTPUT: &str = "src/components/Task.tsx:14:9 lint/suspicious/noExplicitAny  \u{2501}\u{2501}\u{2501}\u{2501}\n\n  \u{00d7} Unexpected any. Specify a different type.\n\n    12 \u{2502} const value: any = payload;\n\nsrc/lib/date.ts:3:1 lint/style/useConst  \u{2501}\u{2501}\u{2501}\u{2501}\n\n  \u{26a0} This let declares a variable that is never re-assigned.\n\nChecked 214 files in 82ms. No fixes applied.\nFound 1 error.\nFound 1 warning.\n";

    #[test]
    fn counts_biome_findings_with_their_severity() {
        let snapshot = parse_diagnostics(BIOME_OUTPUT, "", EvalCategory::Lint);

        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.warnings, 1);
        assert_eq!(snapshot.diagnostics.len(), 2);
    }

    #[test]
    fn keeps_the_rule_and_location_of_a_biome_finding() {
        let snapshot = parse_diagnostics(BIOME_OUTPUT, "", EvalCategory::Lint);

        assert_eq!(
            snapshot.diagnostics[0].location(),
            "src/components/Task.tsx:14:9"
        );
        assert_eq!(
            snapshot.diagnostics[0].code.as_deref(),
            Some("lint/suspicious/noExplicitAny")
        );
        assert_eq!(
            snapshot.diagnostics[0].message,
            "Unexpected any. Specify a different type."
        );
        assert_eq!(
            snapshot.diagnostics[1].severity,
            DiagnosticSeverity::Warning
        );
    }

    const ESLINT_OUTPUT: &str = "/repo/src/app.js\n  1:1   error    Unexpected var, use let or const instead  no-var\n  9:20  warning  Missing semicolon                         semi\n\n\u{2716} 2 problems (1 error, 1 warning)\n";

    #[test]
    fn counts_eslint_stylish_findings() {
        let snapshot = parse_diagnostics(ESLINT_OUTPUT, "", EvalCategory::Lint);

        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.warnings, 1);
        assert_eq!(snapshot.diagnostics[0].file, "/repo/src/app.js");
        assert_eq!(snapshot.diagnostics[0].location(), "/repo/src/app.js:1:1");
        assert_eq!(snapshot.diagnostics[0].code.as_deref(), Some("no-var"));
        assert_eq!(snapshot.diagnostics[1].code.as_deref(), Some("semi"));
    }

    #[test]
    fn falls_back_to_a_summary_when_no_finding_can_be_located() {
        let snapshot = parse_diagnostics(
            "Checked 12 files.\nFound 3 errors.\nFound 4 warnings.\n",
            "",
            EvalCategory::Lint,
        );

        assert_eq!(snapshot.errors, 3);
        assert_eq!(snapshot.warnings, 4);
        assert!(snapshot.diagnostics.is_empty());
    }

    #[test]
    fn falls_back_to_an_eslint_problem_summary() {
        let snapshot = parse_diagnostics(
            "\u{2716} 5 problems (4 errors, 1 warning)\n",
            "",
            EvalCategory::Lint,
        );

        assert_eq!(snapshot.errors, 4);
        assert_eq!(snapshot.warnings, 1);
    }

    #[test]
    fn reports_a_clean_lint_run_as_no_findings() {
        let snapshot = parse_diagnostics(
            "Checked 214 files in 82ms. No fixes applied.\n",
            "",
            EvalCategory::Lint,
        );

        assert_eq!(snapshot.findings(), 0);
        assert!(snapshot.diagnostics.is_empty());
    }

    #[test]
    fn reads_the_bun_coverage_table() {
        let output = "-------------|---------|---------|-------------------\nFile         | % Funcs | % Lines | Uncovered Line #s\n-------------|---------|---------|-------------------\nAll files    |   85.71 |   78.26 |\n src/index.ts|   90.00 |   80.00 | 12-14\n";

        let snapshot = parse_coverage(output, "");

        assert_eq!(snapshot.line_coverage_percent, Some(78.26));
    }

    #[test]
    fn reads_the_istanbul_coverage_table_from_a_different_column() {
        let output = "File      | % Stmts | % Branch | % Funcs | % Lines | Uncovered Line #s\nAll files |   85.71 |    72.22 |   90.00 |   66.50 |\n";

        let snapshot = parse_coverage(output, "");

        assert_eq!(snapshot.line_coverage_percent, Some(66.50));
    }

    #[test]
    fn reads_a_json_coverage_summary() {
        let snapshot = parse_coverage(r#"{"total":{"lines":{"pct":91.25,"total":400}}}"#, "");
        assert_eq!(snapshot.line_coverage_percent, Some(91.25));
    }

    #[test]
    fn reads_a_coverage_table_that_arrives_on_stderr() {
        let output = "File      | % Lines |\nAll files |   50.00 |\n";
        assert_eq!(
            parse_coverage("", output).line_coverage_percent,
            Some(50.00)
        );
    }

    #[test]
    fn tolerates_output_with_no_coverage_at_all() {
        assert!(
            parse_coverage("nothing to see here\n", "")
                .line_coverage_percent
                .is_none()
        );
    }
}
