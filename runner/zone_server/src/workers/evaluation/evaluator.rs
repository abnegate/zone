//! Orchestration: snapshot a workspace, let the agent work, snapshot it again.

use std::path::Path;
use std::time::Instant;

use super::delta::{EvalDelta, EvaluationReport};
use super::detector::{self, DetectedTool};
use super::execution;
use super::lookup::{PathLookup, ProgramLookup};
use super::settings::EvaluationSettings;
use super::snapshot::{EvalSnapshot, RunOutcome};

pub struct Evaluator {
    settings: EvaluationSettings,
    tools: Vec<DetectedTool>,
}

impl Evaluator {
    pub fn detect(workspace: &Path, settings: EvaluationSettings) -> Self {
        Self::detect_with(workspace, settings, &PathLookup)
    }

    pub fn detect_with(
        workspace: &Path,
        settings: EvaluationSettings,
        lookup: &dyn ProgramLookup,
    ) -> Self {
        let tools = if settings.enabled {
            detector::detect(workspace, lookup)
                .into_iter()
                .filter(|tool| settings.categories.includes(tool.category))
                .collect()
        } else {
            Vec::new()
        };
        Self { settings, tools }
    }

    pub fn with_tools(settings: EvaluationSettings, tools: Vec<DetectedTool>) -> Self {
        Self { settings, tools }
    }

    pub fn tools(&self) -> &[DetectedTool] {
        &self.tools
    }

    /// Whether running the evaluation would measure anything at all.
    pub fn is_active(&self) -> bool {
        self.settings.enabled && !self.tools.is_empty()
    }

    /// Measure the workspace as the agent found it.
    pub async fn baseline(&self) -> Vec<EvalSnapshot> {
        if !self.is_active() {
            return Vec::new();
        }
        self.measure().await
    }

    /// Measure the workspace as the agent left it, against a baseline.
    pub async fn compare(&self, baseline: Vec<EvalSnapshot>) -> EvaluationReport {
        if baseline.is_empty() || !self.is_active() {
            return EvaluationReport::empty();
        }

        let after = self.measure().await;
        let deltas: Vec<EvalDelta> = baseline
            .into_iter()
            .filter_map(|before| {
                after
                    .iter()
                    .find(|candidate| candidate.tool == before.tool)
                    .map(|candidate| EvalDelta::compute(before, candidate.clone()))
            })
            .collect();

        EvaluationReport::from_deltas(deltas)
    }

    async fn measure(&self) -> Vec<EvalSnapshot> {
        let deadline = Instant::now() + self.settings.total_budget;
        let mut snapshots = Vec::with_capacity(self.tools.len());

        for tool in &self.tools {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                tracing::warn!(
                    tool = %tool.identity(),
                    "evaluation budget exhausted before the tool could run"
                );
                snapshots.push(EvalSnapshot::unmeasured(
                    tool.identity(),
                    tool.category,
                    RunOutcome::BudgetExhausted,
                ));
                continue;
            }
            snapshots
                .push(execution::run(tool, &self.settings, self.settings.slice(remaining)).await);
        }

        snapshots
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::evaluation::category::EvalCategory;
    use crate::workers::evaluation::delta::Verdict;
    use crate::workers::evaluation::detector::{Ecosystem, OutputFormat, ROOT_SCOPE};
    use crate::workers::evaluation::lookup::{EveryProgram, NamedPrograms};
    use std::path::PathBuf;
    use std::time::Duration;

    fn enabled() -> EvaluationSettings {
        EvaluationSettings {
            enabled: true,
            ..EvaluationSettings::default()
        }
    }

    fn replaying_tool(directory: &Path, name: &str, source: &str) -> DetectedTool {
        DetectedTool {
            ecosystem: Ecosystem::Cargo,
            category: EvalCategory::Test,
            name: name.to_string(),
            scope: ROOT_SCOPE.to_string(),
            directory: directory.to_path_buf(),
            program: "/bin/sh".to_string(),
            arguments: vec!["-c".to_string(), format!("cat {source}")],
            format: OutputFormat::CargoTest,
        }
    }

    fn summary(passed: u32, failed: u32) -> String {
        format!(
            "test result: ok. {passed} passed; {failed} failed; 0 ignored; 0 measured; 0 filtered out\n"
        )
    }

    #[test]
    fn detects_nothing_while_evaluation_is_disabled() {
        let evaluator = Evaluator::detect_with(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            EvaluationSettings::default(),
            &EveryProgram,
        );

        assert!(evaluator.tools().is_empty());
        assert!(!evaluator.is_active());
    }

    #[test]
    fn keeps_only_the_selected_categories() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("Cargo.toml"), "[package]\n").expect("manifest");

        let evaluator = Evaluator::detect_with(directory.path(), enabled(), &EveryProgram);
        let categories: Vec<EvalCategory> =
            evaluator.tools().iter().map(|tool| tool.category).collect();

        assert!(evaluator.is_active());
        assert!(categories.contains(&EvalCategory::Test));
        assert!(categories.contains(&EvalCategory::Lint));
        assert!(categories.contains(&EvalCategory::Typecheck));
        assert!(
            !categories.contains(&EvalCategory::Coverage),
            "coverage is opt-in, got {categories:?}"
        );
    }

    #[test]
    fn is_inactive_for_a_workspace_with_no_tooling() {
        let directory = tempfile::tempdir().expect("temporary directory");

        let evaluator =
            Evaluator::detect_with(directory.path(), enabled(), &NamedPrograms::default());

        assert!(!evaluator.is_active());
        assert!(evaluator.tools().is_empty());
    }

    #[tokio::test]
    async fn an_inactive_evaluator_measures_nothing() {
        let evaluator = Evaluator::with_tools(EvaluationSettings::default(), Vec::new());

        assert!(evaluator.baseline().await.is_empty());
        assert_eq!(
            evaluator.compare(Vec::new()).await.verdict,
            Verdict::NotMeasured
        );
    }

    #[tokio::test]
    async fn reports_a_regression_when_the_agent_breaks_tests() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let results = directory.path().join("results.txt");
        std::fs::write(&results, summary(615, 0)).expect("baseline results written");

        let evaluator = Evaluator::with_tools(
            enabled(),
            vec![replaying_tool(
                directory.path(),
                "cargo test",
                "results.txt",
            )],
        );

        let baseline = evaluator.baseline().await;
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline[0].passed, 615);

        std::fs::write(&results, summary(612, 3)).expect("after results written");
        let report = evaluator.compare(baseline).await;

        assert_eq!(report.verdict, Verdict::Regressed);
        assert!(report.has_regressions());
        assert_eq!(report.regressed_tools(), vec!["cargo test"]);
        assert_eq!(report.deltas[0].failed_delta, 3);
        assert_eq!(report.deltas[0].passed_delta, -3);
    }

    #[tokio::test]
    async fn reports_an_improvement_when_the_agent_fixes_tests() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let results = directory.path().join("results.txt");
        std::fs::write(&results, summary(610, 5)).expect("baseline results written");

        let evaluator = Evaluator::with_tools(
            enabled(),
            vec![replaying_tool(
                directory.path(),
                "cargo test",
                "results.txt",
            )],
        );

        let baseline = evaluator.baseline().await;
        std::fs::write(&results, summary(615, 0)).expect("after results written");
        let report = evaluator.compare(baseline).await;

        assert_eq!(report.verdict, Verdict::Improved);
        assert!(!report.has_regressions());
        assert_eq!(report.deltas[0].failed_delta, -5);
    }

    #[tokio::test]
    async fn reports_no_change_when_the_agent_touches_nothing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("results.txt"), summary(615, 0)).expect("results");

        let evaluator = Evaluator::with_tools(
            enabled(),
            vec![replaying_tool(
                directory.path(),
                "cargo test",
                "results.txt",
            )],
        );

        let baseline = evaluator.baseline().await;
        let report = evaluator.compare(baseline).await;

        assert_eq!(report.verdict, Verdict::Unchanged);
    }

    #[tokio::test]
    async fn measures_every_tool_it_was_given() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("first.txt"), summary(10, 0)).expect("first");
        std::fs::write(directory.path().join("second.txt"), summary(20, 1)).expect("second");

        let evaluator = Evaluator::with_tools(
            enabled(),
            vec![
                replaying_tool(directory.path(), "cargo test", "first.txt"),
                replaying_tool(directory.path(), "bun run test", "second.txt"),
            ],
        );

        let baseline = evaluator.baseline().await;

        assert_eq!(baseline.len(), 2);
        assert_eq!(baseline[0].tool, "cargo test");
        assert_eq!(baseline[0].passed, 10);
        assert_eq!(baseline[1].tool, "bun run test");
        assert_eq!(baseline[1].failed, 1);
    }

    #[tokio::test]
    async fn stops_measuring_once_the_total_budget_is_spent() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("results.txt"), summary(10, 0)).expect("results");

        let evaluator = Evaluator::with_tools(
            EvaluationSettings {
                total_budget: Duration::ZERO,
                ..enabled()
            },
            vec![
                replaying_tool(directory.path(), "cargo test", "results.txt"),
                replaying_tool(directory.path(), "bun run test", "results.txt"),
            ],
        );

        let baseline = evaluator.baseline().await;

        assert_eq!(baseline.len(), 2);
        assert!(
            baseline
                .iter()
                .all(|snapshot| snapshot.outcome == RunOutcome::BudgetExhausted),
            "an exhausted budget still names the tools it skipped"
        );
    }

    #[tokio::test]
    async fn a_tool_that_never_ran_cannot_be_judged() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("results.txt"), summary(10, 0)).expect("results");
        let tools = vec![replaying_tool(
            directory.path(),
            "cargo test",
            "results.txt",
        )];

        let baseline = Evaluator::with_tools(enabled(), tools.clone())
            .baseline()
            .await;
        let starved = Evaluator::with_tools(
            EvaluationSettings {
                total_budget: Duration::ZERO,
                ..enabled()
            },
            tools,
        );

        let report = starved.compare(baseline).await;

        assert_eq!(report.verdict, Verdict::NotMeasured);
        assert!(!report.has_regressions());
    }

    #[tokio::test]
    async fn drops_a_tool_that_disappeared_between_the_two_snapshots() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("results.txt"), summary(10, 0)).expect("results");

        let baseline = Evaluator::with_tools(
            enabled(),
            vec![
                replaying_tool(directory.path(), "cargo test", "results.txt"),
                replaying_tool(directory.path(), "bun run test", "results.txt"),
            ],
        )
        .baseline()
        .await;

        let report = Evaluator::with_tools(
            enabled(),
            vec![replaying_tool(
                directory.path(),
                "cargo test",
                "results.txt",
            )],
        )
        .compare(baseline)
        .await;

        assert_eq!(report.deltas.len(), 1);
        assert_eq!(report.deltas[0].tool, "cargo test");
    }

    #[tokio::test]
    async fn detection_over_a_real_tree_pairs_before_and_after_by_identity() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(
            directory.path().join("package.json"),
            r#"{"packageManager":"bun@1.4.0","scripts":{"test":"bun test"}}"#,
        )
        .expect("manifest written");

        let evaluator = Evaluator::detect_with(directory.path(), enabled(), &EveryProgram);
        let identities: Vec<String> = evaluator
            .tools()
            .iter()
            .map(DetectedTool::identity)
            .collect();

        assert_eq!(identities, vec!["bun run test"]);
    }

    #[tokio::test]
    async fn an_empty_baseline_short_circuits_the_after_run() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("results.txt"), summary(10, 0)).expect("results");

        let report = Evaluator::with_tools(
            enabled(),
            vec![replaying_tool(
                directory.path(),
                "cargo test",
                "results.txt",
            )],
        )
        .compare(Vec::new())
        .await;

        assert_eq!(report.verdict, Verdict::NotMeasured);
        assert!(report.deltas.is_empty());
    }

    #[tokio::test]
    async fn a_workspace_path_that_does_not_exist_is_simply_inactive() {
        let evaluator =
            Evaluator::detect_with(&PathBuf::from("/zone/not/here"), enabled(), &EveryProgram);

        assert!(!evaluator.is_active());
        assert!(evaluator.baseline().await.is_empty());
    }
}
