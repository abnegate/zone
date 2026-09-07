//! Turning captured tool output into a structured snapshot.

mod cargo;
mod generic;
mod javascript;

use super::category::EvalCategory;
use super::detector::OutputFormat;
use super::snapshot::EvalSnapshot;

pub fn parse(
    format: OutputFormat,
    category: EvalCategory,
    stdout: &str,
    stderr: &str,
) -> EvalSnapshot {
    let mut snapshot = match format {
        OutputFormat::CargoTest => cargo::parse_test(stdout, stderr),
        OutputFormat::CargoDiagnostics => cargo::parse_diagnostics(stdout, stderr, category),
        OutputFormat::CargoFormat => cargo::parse_format(stdout, stderr),
        OutputFormat::CargoCoverage => cargo::parse_coverage(stdout, stderr),
        OutputFormat::JavaScriptTest => javascript::parse_test(stdout, stderr),
        OutputFormat::JavaScriptDiagnostics => {
            javascript::parse_diagnostics(stdout, stderr, category)
        }
        OutputFormat::JavaScriptCoverage => javascript::parse_coverage(stdout, stderr),
        OutputFormat::Generic => generic::parse(stdout, stderr, category),
    };
    snapshot.category = category;
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_cargo_test_output_to_the_cargo_parser() {
        let snapshot = parse(
            OutputFormat::CargoTest,
            EvalCategory::Test,
            "test result: ok. 615 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
            "",
        );
        assert_eq!(snapshot.passed, 615);
    }

    #[test]
    fn routes_bun_test_output_to_the_javascript_parser() {
        let snapshot = parse(
            OutputFormat::JavaScriptTest,
            EvalCategory::Test,
            " 12 pass\n 1 fail\n",
            "",
        );
        assert_eq!(snapshot.passed, 12);
        assert_eq!(snapshot.failed, 1);
    }

    #[test]
    fn every_format_reports_the_category_it_was_asked_for() {
        let formats = [
            OutputFormat::CargoTest,
            OutputFormat::CargoDiagnostics,
            OutputFormat::CargoFormat,
            OutputFormat::CargoCoverage,
            OutputFormat::JavaScriptTest,
            OutputFormat::JavaScriptDiagnostics,
            OutputFormat::JavaScriptCoverage,
            OutputFormat::Generic,
        ];
        for format in formats {
            let snapshot = parse(format, EvalCategory::Build, "", "");
            assert_eq!(snapshot.category, EvalCategory::Build, "{format:?}");
        }
    }

    #[test]
    fn every_format_survives_empty_output() {
        let formats = [
            OutputFormat::CargoTest,
            OutputFormat::CargoDiagnostics,
            OutputFormat::CargoFormat,
            OutputFormat::CargoCoverage,
            OutputFormat::JavaScriptTest,
            OutputFormat::JavaScriptDiagnostics,
            OutputFormat::JavaScriptCoverage,
            OutputFormat::Generic,
        ];
        for format in formats {
            let snapshot = parse(format, EvalCategory::Test, "", "");
            assert_eq!(snapshot.passed, 0, "{format:?}");
            assert_eq!(snapshot.findings(), 0, "{format:?}");
            assert!(snapshot.diagnostics.is_empty(), "{format:?}");
        }
    }
}
