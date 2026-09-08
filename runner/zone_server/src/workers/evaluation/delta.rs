//! Before/after comparison of evaluation snapshots.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::category::EvalCategory;
use super::diagnostic::Diagnostic;
use super::snapshot::EvalSnapshot;

/// Diagnostics kept per tool when the report is written to the run artifacts.
const REPORTED_DIAGNOSTICS: usize = 10;

/// Coverage moves below this many percentage points are treated as noise.
const COVERAGE_EPSILON: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Improved,
    Unchanged,
    Regressed,
    NotMeasured,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Improved => "improved",
            Verdict::Unchanged => "unchanged",
            Verdict::Regressed => "regressed",
            Verdict::NotMeasured => "not_measured",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalDelta {
    pub tool: String,
    pub category: EvalCategory,
    pub before: EvalSnapshot,
    pub after: EvalSnapshot,
    pub passed_delta: i64,
    pub failed_delta: i64,
    pub error_delta: i64,
    pub warning_delta: i64,
    pub coverage_delta_percent: Option<f64>,
    pub introduced: Vec<Diagnostic>,
    pub resolved: Vec<Diagnostic>,
}

impl EvalDelta {
    pub fn compute(before: EvalSnapshot, after: EvalSnapshot) -> Self {
        let previous: HashSet<&Diagnostic> = before.diagnostics.iter().collect();
        let current: HashSet<&Diagnostic> = after.diagnostics.iter().collect();

        let introduced: Vec<Diagnostic> = after
            .diagnostics
            .iter()
            .filter(|d| !previous.contains(d))
            .cloned()
            .collect();
        let resolved: Vec<Diagnostic> = before
            .diagnostics
            .iter()
            .filter(|d| !current.contains(d))
            .cloned()
            .collect();

        let coverage_delta_percent =
            match (before.line_coverage_percent, after.line_coverage_percent) {
                (Some(previous), Some(current)) => Some(current - previous),
                _ => None,
            };

        Self {
            tool: after.tool.clone(),
            category: after.category,
            passed_delta: i64::from(after.passed) - i64::from(before.passed),
            failed_delta: i64::from(after.failed) - i64::from(before.failed),
            error_delta: i64::from(after.errors) - i64::from(before.errors),
            warning_delta: i64::from(after.warnings) - i64::from(before.warnings),
            coverage_delta_percent,
            introduced,
            resolved,
            before,
            after,
        }
    }

    /// Both sides ran to completion, so the numbers can be compared.
    pub fn is_comparable(&self) -> bool {
        self.before.outcome.is_measured() && self.after.outcome.is_measured()
    }

    pub fn is_regression(&self) -> bool {
        if !self.is_comparable() {
            return false;
        }
        self.failed_delta > 0
            || self.passed_delta < 0
            || self.error_delta > 0
            || self.warning_delta > 0
            || !self.introduced.is_empty()
            || self
                .coverage_delta_percent
                .is_some_and(|change| change < -COVERAGE_EPSILON)
    }

    pub fn is_improvement(&self) -> bool {
        if !self.is_comparable() || self.is_regression() {
            return false;
        }
        self.passed_delta > 0
            || self.failed_delta < 0
            || self.error_delta < 0
            || self.warning_delta < 0
            || !self.resolved.is_empty()
            || self
                .coverage_delta_percent
                .is_some_and(|change| change > COVERAGE_EPSILON)
    }

    pub fn verdict(&self) -> Verdict {
        if !self.is_comparable() {
            Verdict::NotMeasured
        } else if self.is_regression() {
            Verdict::Regressed
        } else if self.is_improvement() {
            Verdict::Improved
        } else {
            Verdict::Unchanged
        }
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.is_comparable() {
            parts.push(format!("not measured ({:?})", self.after.outcome));
        }
        if self.category == EvalCategory::Test {
            parts.push(format!(
                "{} passed ({:+}), {} failed ({:+})",
                self.after.passed, self.passed_delta, self.after.failed, self.failed_delta
            ));
        }
        if self.after.findings() > 0 || self.before.findings() > 0 {
            parts.push(format!(
                "{} errors ({:+}), {} warnings ({:+})",
                self.after.errors, self.error_delta, self.after.warnings, self.warning_delta
            ));
        }
        if let Some(change) = self.coverage_delta_percent {
            parts.push(format!("coverage {:+.2}%", change));
        }
        if parts.is_empty() {
            parts.push("no change".to_string());
        }
        format!("{} [{}]: {}", self.tool, self.verdict(), parts.join(", "))
    }

    fn artifact(&self) -> Value {
        json!({
            "tool": self.tool,
            "category": self.category,
            "verdict": self.verdict(),
            "outcome": self.after.outcome,
            "before": counts(&self.before),
            "after": counts(&self.after),
            "passed_delta": self.passed_delta,
            "failed_delta": self.failed_delta,
            "error_delta": self.error_delta,
            "warning_delta": self.warning_delta,
            "coverage_delta_percent": self.coverage_delta_percent,
            "introduced": self
                .introduced
                .iter()
                .take(REPORTED_DIAGNOSTICS)
                .map(|d| json!({ "location": d.location(), "severity": d.severity, "code": d.code, "message": d.message }))
                .collect::<Vec<Value>>(),
            "introduced_total": self.introduced.len(),
            "resolved_total": self.resolved.len(),
        })
    }
}

fn counts(snapshot: &EvalSnapshot) -> Value {
    json!({
        "passed": snapshot.passed,
        "failed": snapshot.failed,
        "skipped": snapshot.skipped,
        "errors": snapshot.errors,
        "warnings": snapshot.warnings,
        "exit_code": snapshot.exit_code,
        "duration_milliseconds": snapshot.duration_milliseconds,
        "line_coverage_percent": snapshot.line_coverage_percent,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub verdict: Verdict,
    pub deltas: Vec<EvalDelta>,
    pub summary: String,
}

impl EvaluationReport {
    pub fn from_deltas(deltas: Vec<EvalDelta>) -> Self {
        let verdict = Self::overall_verdict(&deltas);
        let summary = Self::summarise(verdict, &deltas);
        Self {
            verdict,
            deltas,
            summary,
        }
    }

    pub fn empty() -> Self {
        Self::from_deltas(Vec::new())
    }

    pub fn has_regressions(&self) -> bool {
        self.deltas.iter().any(EvalDelta::is_regression)
    }

    pub fn regressed_tools(&self) -> Vec<&str> {
        self.deltas
            .iter()
            .filter(|delta| delta.is_regression())
            .map(|delta| delta.tool.as_str())
            .collect()
    }

    pub fn artifact(&self) -> Value {
        json!({
            "verdict": self.verdict,
            "summary": self.summary,
            "tools": self.deltas.iter().map(EvalDelta::artifact).collect::<Vec<Value>>(),
        })
    }

    fn overall_verdict(deltas: &[EvalDelta]) -> Verdict {
        let comparable: Vec<&EvalDelta> = deltas.iter().filter(|d| d.is_comparable()).collect();
        if comparable.is_empty() {
            Verdict::NotMeasured
        } else if comparable.iter().any(|d| d.is_regression()) {
            Verdict::Regressed
        } else if comparable.iter().any(|d| d.is_improvement()) {
            Verdict::Improved
        } else {
            Verdict::Unchanged
        }
    }

    fn summarise(verdict: Verdict, deltas: &[EvalDelta]) -> String {
        if deltas.is_empty() {
            return "No evaluation tooling was detected for this workspace.".to_string();
        }
        let mut lines = vec![format!("Code quality: {}", verdict)];
        lines.extend(deltas.iter().map(EvalDelta::describe));
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::evaluation::diagnostic::DiagnosticSeverity;
    use crate::workers::evaluation::snapshot::RunOutcome;

    fn tests(tool: &str, passed: u32, failed: u32) -> EvalSnapshot {
        EvalSnapshot {
            tool: tool.to_string(),
            category: EvalCategory::Test,
            passed,
            failed,
            errors: failed,
            exit_code: Some(if failed > 0 { 1 } else { 0 }),
            ..EvalSnapshot::default()
        }
    }

    fn lints(tool: &str, diagnostics: Vec<Diagnostic>) -> EvalSnapshot {
        let mut snapshot = EvalSnapshot {
            tool: tool.to_string(),
            category: EvalCategory::Lint,
            ..EvalSnapshot::default()
        };
        for diagnostic in diagnostics {
            snapshot.push(diagnostic);
        }
        snapshot
    }

    fn warning(file: &str, line: u32, message: &str) -> Diagnostic {
        Diagnostic::new(file, DiagnosticSeverity::Warning, message).at(Some(line), None)
    }

    #[test]
    fn reports_an_improvement_when_failures_turn_into_passes() {
        let delta = EvalDelta::compute(tests("cargo test", 10, 2), tests("cargo test", 12, 0));

        assert_eq!(delta.passed_delta, 2);
        assert_eq!(delta.failed_delta, -2);
        assert!(delta.is_improvement());
        assert!(!delta.is_regression());
        assert_eq!(delta.verdict(), Verdict::Improved);
    }

    #[test]
    fn reports_a_regression_when_fewer_tests_pass() {
        let delta = EvalDelta::compute(tests("cargo test", 615, 0), tests("cargo test", 612, 3));

        assert_eq!(delta.passed_delta, -3);
        assert_eq!(delta.failed_delta, 3);
        assert!(delta.is_regression());
        assert!(!delta.is_improvement());
        assert_eq!(delta.verdict(), Verdict::Regressed);
    }

    #[test]
    fn treats_a_dropped_pass_count_as_a_regression_even_without_new_failures() {
        let delta = EvalDelta::compute(tests("bun run test", 40, 0), tests("bun run test", 38, 0));

        assert!(
            delta.is_regression(),
            "silently losing two passing tests is a regression"
        );
    }

    #[test]
    fn reports_no_change_when_both_sides_match() {
        let delta = EvalDelta::compute(tests("cargo test", 615, 0), tests("cargo test", 615, 0));

        assert_eq!(delta.verdict(), Verdict::Unchanged);
        assert!(!delta.is_regression());
        assert!(!delta.is_improvement());
    }

    #[test]
    fn separates_introduced_from_resolved_diagnostics() {
        let before = lints(
            "cargo clippy",
            vec![warning("src/a.rs", 1, "unused import")],
        );
        let after = lints(
            "cargo clippy",
            vec![warning("src/b.rs", 9, "needless clone")],
        );

        let delta = EvalDelta::compute(before, after);

        assert_eq!(delta.introduced.len(), 1);
        assert_eq!(delta.introduced[0].file, "src/b.rs");
        assert_eq!(delta.resolved.len(), 1);
        assert_eq!(delta.resolved[0].file, "src/a.rs");
        assert!(delta.is_regression(), "a new lint finding is a regression");
    }

    #[test]
    fn ignores_diagnostics_that_persist_unchanged() {
        let shared = warning("src/a.rs", 1, "unused import");
        let delta = EvalDelta::compute(
            lints("cargo clippy", vec![shared.clone()]),
            lints("cargo clippy", vec![shared]),
        );

        assert!(delta.introduced.is_empty());
        assert!(delta.resolved.is_empty());
        assert_eq!(delta.verdict(), Verdict::Unchanged);
    }

    #[test]
    fn treats_resolved_lints_as_an_improvement() {
        let delta = EvalDelta::compute(
            lints(
                "cargo clippy",
                vec![warning("src/a.rs", 1, "unused import")],
            ),
            lints("cargo clippy", Vec::new()),
        );

        assert_eq!(delta.warning_delta, -1);
        assert!(delta.is_improvement());
    }

    #[test]
    fn tracks_coverage_movement_in_both_directions() {
        let with_coverage = |percent: f64| EvalSnapshot {
            tool: "cargo llvm-cov".to_string(),
            category: EvalCategory::Coverage,
            line_coverage_percent: Some(percent),
            ..EvalSnapshot::default()
        };

        let up = EvalDelta::compute(with_coverage(80.0), with_coverage(85.5));
        assert!((up.coverage_delta_percent.expect("coverage moved") - 5.5).abs() < 1e-9);
        assert!(up.is_improvement());

        let down = EvalDelta::compute(with_coverage(85.5), with_coverage(80.0));
        assert!(down.is_regression());
    }

    #[test]
    fn ignores_coverage_noise_below_the_epsilon() {
        let with_coverage = |percent: f64| EvalSnapshot {
            tool: "cargo llvm-cov".to_string(),
            category: EvalCategory::Coverage,
            line_coverage_percent: Some(percent),
            ..EvalSnapshot::default()
        };

        let delta = EvalDelta::compute(with_coverage(80.00), with_coverage(80.01));
        assert_eq!(delta.verdict(), Verdict::Unchanged);
    }

    #[test]
    fn refuses_to_judge_a_run_that_did_not_complete() {
        let after =
            EvalSnapshot::unmeasured("cargo test", EvalCategory::Test, RunOutcome::TimedOut);
        let delta = EvalDelta::compute(tests("cargo test", 615, 0), after);

        assert!(!delta.is_comparable());
        assert!(!delta.is_regression());
        assert!(!delta.is_improvement());
        assert_eq!(delta.verdict(), Verdict::NotMeasured);
    }

    #[test]
    fn overall_verdict_is_regressed_when_any_tool_regresses() {
        let report = EvaluationReport::from_deltas(vec![
            EvalDelta::compute(tests("cargo test", 10, 0), tests("cargo test", 12, 0)),
            EvalDelta::compute(
                lints("cargo clippy", Vec::new()),
                lints(
                    "cargo clippy",
                    vec![warning("src/a.rs", 2, "needless borrow")],
                ),
            ),
        ]);

        assert_eq!(report.verdict, Verdict::Regressed);
        assert!(report.has_regressions());
        assert_eq!(report.regressed_tools(), vec!["cargo clippy"]);
    }

    #[test]
    fn overall_verdict_is_improved_when_nothing_regresses() {
        let report = EvaluationReport::from_deltas(vec![
            EvalDelta::compute(tests("cargo test", 10, 2), tests("cargo test", 12, 0)),
            EvalDelta::compute(
                lints("cargo clippy", Vec::new()),
                lints("cargo clippy", Vec::new()),
            ),
        ]);

        assert_eq!(report.verdict, Verdict::Improved);
        assert!(!report.has_regressions());
    }

    #[test]
    fn overall_verdict_is_not_measured_when_nothing_completed() {
        let report = EvaluationReport::from_deltas(vec![EvalDelta::compute(
            EvalSnapshot::unmeasured("cargo test", EvalCategory::Test, RunOutcome::TimedOut),
            EvalSnapshot::unmeasured("cargo test", EvalCategory::Test, RunOutcome::TimedOut),
        )]);

        assert_eq!(report.verdict, Verdict::NotMeasured);
    }

    #[test]
    fn empty_report_explains_that_nothing_was_detected() {
        let report = EvaluationReport::empty();
        assert_eq!(report.verdict, Verdict::NotMeasured);
        assert!(report.deltas.is_empty());
        assert!(report.summary.contains("No evaluation tooling"));
    }

    #[test]
    fn artifact_caps_the_diagnostics_it_persists() {
        let many: Vec<Diagnostic> = (0..25)
            .map(|index| warning(&format!("src/file{index}.rs"), index, "needless clone"))
            .collect();
        let delta = EvalDelta::compute(
            lints("cargo clippy", Vec::new()),
            lints("cargo clippy", many),
        );
        let report = EvaluationReport::from_deltas(vec![delta]);

        let artifact = report.artifact();
        let tool = &artifact["tools"][0];
        assert_eq!(
            tool["introduced"]
                .as_array()
                .expect("introduced list")
                .len(),
            REPORTED_DIAGNOSTICS
        );
        assert_eq!(tool["introduced_total"], 25);
        assert_eq!(artifact["verdict"], "regressed");
    }

    #[test]
    fn artifact_carries_before_and_after_counts() {
        let delta = EvalDelta::compute(tests("cargo test", 10, 2), tests("cargo test", 12, 0));
        let artifact = EvaluationReport::from_deltas(vec![delta]).artifact();
        let tool = &artifact["tools"][0];

        assert_eq!(tool["before"]["passed"], 10);
        assert_eq!(tool["after"]["passed"], 12);
        assert_eq!(tool["passed_delta"], 2);
        assert_eq!(tool["failed_delta"], -2);
        assert_eq!(tool["category"], "test");
    }

    #[test]
    fn summary_names_every_tool() {
        let report = EvaluationReport::from_deltas(vec![
            EvalDelta::compute(tests("cargo test", 10, 0), tests("cargo test", 10, 0)),
            EvalDelta::compute(
                lints("bun run lint", Vec::new()),
                lints("bun run lint", Vec::new()),
            ),
        ]);

        assert!(report.summary.contains("cargo test"));
        assert!(report.summary.contains("bun run lint"));
    }

    #[test]
    fn report_round_trips_through_serde() {
        let report = EvaluationReport::from_deltas(vec![EvalDelta::compute(
            tests("cargo test", 10, 2),
            tests("cargo test", 12, 0),
        )]);
        let encoded = serde_json::to_string(&report).expect("report serialises");
        let decoded: EvaluationReport =
            serde_json::from_str(&encoded).expect("report deserialises");
        assert_eq!(decoded, report);
    }
}
