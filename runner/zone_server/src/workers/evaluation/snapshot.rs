//! One measurement of a single tool at a single point in time.

use serde::{Deserialize, Serialize};

use super::category::EvalCategory;
use super::diagnostic::{Diagnostic, DiagnosticSeverity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Completed,
    TimedOut,
    BudgetExhausted,
    SpawnFailed,
}

impl RunOutcome {
    pub fn is_measured(self) -> bool {
        matches!(self, RunOutcome::Completed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalSnapshot {
    pub tool: String,
    pub category: EvalCategory,
    pub outcome: RunOutcome,
    pub exit_code: Option<i32>,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub errors: u32,
    pub warnings: u32,
    pub diagnostics: Vec<Diagnostic>,
    pub raw_output: String,
    pub duration_milliseconds: u64,
    pub line_coverage_percent: Option<f64>,
}

impl Default for EvalSnapshot {
    fn default() -> Self {
        Self {
            tool: String::new(),
            category: EvalCategory::Test,
            outcome: RunOutcome::Completed,
            exit_code: None,
            passed: 0,
            failed: 0,
            skipped: 0,
            errors: 0,
            warnings: 0,
            diagnostics: Vec::new(),
            raw_output: String::new(),
            duration_milliseconds: 0,
            line_coverage_percent: None,
        }
    }
}

impl EvalSnapshot {
    pub fn for_category(category: EvalCategory) -> Self {
        Self {
            category,
            ..Self::default()
        }
    }

    pub fn unmeasured(
        tool: impl Into<String>,
        category: EvalCategory,
        outcome: RunOutcome,
    ) -> Self {
        Self {
            tool: tool.into(),
            category,
            outcome,
            ..Self::default()
        }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        match diagnostic.severity {
            DiagnosticSeverity::Error => self.errors += 1,
            DiagnosticSeverity::Warning => self.warnings += 1,
            DiagnosticSeverity::Info => {}
        }
        self.diagnostics.push(diagnostic);
    }

    pub fn findings(&self) -> u32 {
        self.errors + self.warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_severities_as_diagnostics_are_pushed() {
        let mut snapshot = EvalSnapshot::for_category(EvalCategory::Lint);
        snapshot.push(Diagnostic::new("a.rs", DiagnosticSeverity::Error, "boom"));
        snapshot.push(Diagnostic::new("b.rs", DiagnosticSeverity::Warning, "meh"));
        snapshot.push(Diagnostic::new("c.rs", DiagnosticSeverity::Warning, "meh"));
        snapshot.push(Diagnostic::new("d.rs", DiagnosticSeverity::Info, "fyi"));

        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.warnings, 2);
        assert_eq!(snapshot.findings(), 3);
        assert_eq!(snapshot.diagnostics.len(), 4);
    }

    #[test]
    fn marks_unmeasured_runs() {
        let snapshot =
            EvalSnapshot::unmeasured("cargo test", EvalCategory::Test, RunOutcome::TimedOut);
        assert!(!snapshot.outcome.is_measured());
        assert_eq!(snapshot.tool, "cargo test");
        assert!(snapshot.exit_code.is_none());
    }

    #[test]
    fn round_trips_through_serde() {
        let mut snapshot = EvalSnapshot::for_category(EvalCategory::Test);
        snapshot.tool = "cargo test".to_string();
        snapshot.passed = 615;
        snapshot.line_coverage_percent = Some(82.5);

        let encoded = serde_json::to_string(&snapshot).expect("snapshot serialises");
        let decoded: EvalSnapshot = serde_json::from_str(&encoded).expect("snapshot deserialises");
        assert_eq!(decoded, snapshot);
    }
}
