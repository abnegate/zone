//! Last-resort parsing for a tool whose output shape is unknown.

use crate::workers::evaluation::category::EvalCategory;
use crate::workers::evaluation::diagnostic::{Diagnostic, DiagnosticSeverity};
use crate::workers::evaluation::snapshot::EvalSnapshot;

/// Diagnostics kept from an unrecognised stream, so one noisy tool cannot
/// dominate the report.
const MAX_DIAGNOSTICS: usize = 50;

pub fn parse(stdout: &str, stderr: &str, category: EvalCategory) -> EvalSnapshot {
    let mut snapshot = EvalSnapshot::for_category(category);

    for line in stdout.lines().chain(stderr.lines()) {
        let trimmed = line.trim_start();
        let severity = if trimmed.starts_with("error") || trimmed.starts_with("ERROR") {
            DiagnosticSeverity::Error
        } else if trimmed.starts_with("warning") || trimmed.starts_with("WARNING") {
            DiagnosticSeverity::Warning
        } else {
            continue;
        };

        if snapshot.diagnostics.len() < MAX_DIAGNOSTICS {
            snapshot.push(Diagnostic::new("unknown", severity, trimmed.trim_end()));
        } else {
            match severity {
                DiagnosticSeverity::Error => snapshot.errors += 1,
                DiagnosticSeverity::Warning => snapshot.warnings += 1,
                DiagnosticSeverity::Info => {}
            }
        }
    }

    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_error_and_warning_lines() {
        let output = "warning: unused variable `total`\nerror: could not compile `zone_server`\nnote: run with RUST_BACKTRACE=1\n";

        let snapshot = parse(output, "", EvalCategory::Build);

        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.warnings, 1);
        assert_eq!(snapshot.diagnostics.len(), 2);
        assert_eq!(snapshot.category, EvalCategory::Build);
    }

    #[test]
    fn reads_both_streams() {
        let snapshot = parse(
            "error: from stdout\n",
            "error: from stderr\n",
            EvalCategory::Build,
        );
        assert_eq!(snapshot.errors, 2);
    }

    #[test]
    fn reports_nothing_for_quiet_output() {
        let snapshot = parse(
            "Compiling zone_server v0.1.0\nFinished in 12s\n",
            "",
            EvalCategory::Build,
        );
        assert_eq!(snapshot.findings(), 0);
    }

    #[test]
    fn keeps_counting_past_the_diagnostic_cap() {
        let output = "error: boom\n".repeat(MAX_DIAGNOSTICS + 20);

        let snapshot = parse(&output, "", EvalCategory::Build);

        assert_eq!(snapshot.diagnostics.len(), MAX_DIAGNOSTICS);
        assert_eq!(
            snapshot.errors,
            (MAX_DIAGNOSTICS + 20) as u32,
            "the count stays honest even once diagnostics stop being kept"
        );
    }
}
