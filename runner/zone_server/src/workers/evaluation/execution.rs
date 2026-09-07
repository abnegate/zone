//! Running a detected tool under the tool runner's confined executor.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::sync::mpsc;
use tool_runner::{CommandExecutor, ErrorCode, ExecutorConfig, InboundMessage, OutboundMessage};
use uuid::Uuid;

use super::detector::DetectedTool;
use super::parser;
use super::settings::EvaluationSettings;
use super::snapshot::{EvalSnapshot, RunOutcome};

const OUTPUT_CHANNEL_CAPACITY: usize = 256;

/// Wall-clock slack over the tool timeout before the drain loop gives up on the
/// executor, covering its SIGTERM grace period.
const SHUTDOWN_MARGIN: Duration = Duration::from_secs(15);

const TRUNCATION_NOTICE: &str = "...[truncated]\n";

/// Applied to every tool so its output parses the same way on a developer
/// machine, in CI, and inside a task run.
const DETERMINISTIC_ENVIRONMENT: [(&str, &str); 5] = [
    ("CI", "1"),
    ("NO_COLOR", "1"),
    ("TERM", "dumb"),
    ("FORCE_COLOR", "0"),
    ("CARGO_TERM_COLOR", "never"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub stdout: String,
    pub stderr: String,
    pub outcome: RunOutcome,
    pub exit_code: Option<i32>,
}

impl Captured {
    fn failed(outcome: RunOutcome) -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            outcome,
            exit_code: None,
        }
    }
}

pub async fn run(
    tool: &DetectedTool,
    settings: &EvaluationSettings,
    timeout: Duration,
) -> EvalSnapshot {
    let started = Instant::now();
    let captured = capture(tool, settings, timeout).await;

    let mut snapshot = parser::parse(
        tool.format,
        tool.category,
        &captured.stdout,
        &captured.stderr,
    );
    snapshot.tool = tool.identity();
    snapshot.outcome = captured.outcome;
    snapshot.exit_code = captured.exit_code;
    snapshot.duration_milliseconds = started.elapsed().as_millis() as u64;
    snapshot.raw_output = truncate(
        &format!("{}{}", captured.stdout, captured.stderr),
        settings.stored_output_limit,
    );
    snapshot
}

pub async fn capture(
    tool: &DetectedTool,
    settings: &EvaluationSettings,
    timeout: Duration,
) -> Captured {
    let (sender, receiver) = mpsc::channel(OUTPUT_CHANNEL_CAPACITY);
    let executor = CommandExecutor::with_config(
        ExecutorConfig::new()
            .with_timeout(timeout)
            .with_max_output(settings.capture_limit),
    );
    let request = InboundMessage::RunStart {
        job_id: Uuid::new_v4().to_string(),
        workspace: tool.directory.clone(),
        command: tool.program.clone(),
        args: tool.arguments.clone(),
        env: environment(),
        timeout_ms: Some(timeout.as_millis().min(u128::from(u64::MAX)) as u64),
        max_output_bytes: Some(settings.capture_limit),
        working_dir: None,
    };

    if let Err(error) = executor.spawn(&request, sender).await {
        tracing::warn!(tool = %tool.identity(), %error, "evaluation tool could not start");
        return Captured::failed(RunOutcome::SpawnFailed);
    }

    match tokio::time::timeout(timeout + SHUTDOWN_MARGIN, drain(receiver)).await {
        Ok(captured) => captured,
        Err(_) => {
            tracing::warn!(
                tool = %tool.identity(),
                "evaluation tool outlived its timeout and the executor's grace period"
            );
            Captured::failed(RunOutcome::TimedOut)
        }
    }
}

async fn drain(mut receiver: mpsc::Receiver<OutboundMessage>) -> Captured {
    let mut captured = Captured {
        stdout: String::new(),
        stderr: String::new(),
        outcome: RunOutcome::Completed,
        exit_code: None,
    };

    while let Some(message) = receiver.recv().await {
        match message {
            OutboundMessage::RunStdout { data, .. } => append(&mut captured.stdout, &data),
            OutboundMessage::RunStderr { data, .. } => append(&mut captured.stderr, &data),
            OutboundMessage::RunExit { exit_code, .. } => {
                captured.exit_code = exit_code;
                break;
            }
            OutboundMessage::RunError { error_code, .. } => {
                captured.outcome = match error_code {
                    ErrorCode::Timeout => RunOutcome::TimedOut,
                    _ => RunOutcome::SpawnFailed,
                };
                break;
            }
            _ => {}
        }
    }

    captured
}

fn append(buffer: &mut String, encoded: &str) {
    if let Ok(bytes) = BASE64.decode(encoded) {
        buffer.push_str(&String::from_utf8_lossy(&bytes));
    }
}

fn environment() -> HashMap<String, String> {
    DETERMINISTIC_ENVIRONMENT
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect()
}

/// Keeps the tail, because a tool's verdict is the last thing it prints.
pub fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    if limit <= TRUNCATION_NOTICE.len() {
        return String::new();
    }
    let mut start = text.len() - (limit - TRUNCATION_NOTICE.len());
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{TRUNCATION_NOTICE}{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::evaluation::category::EvalCategory;
    use crate::workers::evaluation::detector::{Ecosystem, OutputFormat, ROOT_SCOPE};
    use std::path::PathBuf;

    fn shell_tool(directory: PathBuf, script: &str, format: OutputFormat) -> DetectedTool {
        DetectedTool {
            ecosystem: Ecosystem::Cargo,
            category: EvalCategory::Test,
            name: "shell".to_string(),
            scope: ROOT_SCOPE.to_string(),
            directory,
            program: "/bin/sh".to_string(),
            arguments: vec!["-c".to_string(), script.to_string()],
            format,
        }
    }

    #[tokio::test]
    async fn captures_both_output_streams_and_the_exit_code() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "echo out; echo err >&2; exit 3",
            OutputFormat::Generic,
        );

        let captured = capture(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(captured.outcome, RunOutcome::Completed);
        assert_eq!(captured.exit_code, Some(3));
        assert!(captured.stdout.contains("out"));
        assert!(captured.stderr.contains("err"));
    }

    #[tokio::test]
    async fn runs_the_tool_in_its_own_project_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("marker.txt"), "here").expect("marker written");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "cat marker.txt",
            OutputFormat::Generic,
        );

        let captured = capture(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(captured.stdout.trim(), "here");
    }

    #[tokio::test]
    async fn strips_colour_from_tool_output() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "echo \"$CI/$NO_COLOR/$TERM/$CARGO_TERM_COLOR\"",
            OutputFormat::Generic,
        );

        let captured = capture(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(captured.stdout.trim(), "1/1/dumb/never");
    }

    #[tokio::test]
    async fn kills_a_tool_that_outlives_its_timeout() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "sleep 600",
            OutputFormat::Generic,
        );
        let started = Instant::now();

        let captured = capture(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_millis(300),
        )
        .await;

        assert_eq!(captured.outcome, RunOutcome::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "a hung suite must not hold the task run open, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_timed_out_tool_yields_an_unmeasured_snapshot() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "sleep 600",
            OutputFormat::CargoTest,
        );

        let snapshot = run(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_millis(300),
        )
        .await;

        assert_eq!(snapshot.outcome, RunOutcome::TimedOut);
        assert!(!snapshot.outcome.is_measured());
        assert_eq!(snapshot.tool, "shell");
    }

    #[tokio::test]
    async fn caps_the_output_it_reads_from_a_noisy_tool() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "i=0; while [ $i -lt 20000 ]; do echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; i=$((i+1)); done",
            OutputFormat::Generic,
        );
        let settings = EvaluationSettings {
            capture_limit: 4096,
            ..EvaluationSettings::default()
        };

        let captured = capture(&tool, &settings, Duration::from_secs(60)).await;

        assert!(
            captured.stdout.len() <= settings.capture_limit,
            "captured {} bytes, cap is {}",
            captured.stdout.len(),
            settings.capture_limit
        );
        assert!(!captured.stdout.is_empty());
    }

    #[tokio::test]
    async fn caps_the_output_it_stores_on_the_snapshot() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "i=0; while [ $i -lt 2000 ]; do echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; i=$((i+1)); done",
            OutputFormat::Generic,
        );
        let settings = EvaluationSettings {
            stored_output_limit: 512,
            ..EvaluationSettings::default()
        };

        let snapshot = run(&tool, &settings, Duration::from_secs(60)).await;

        assert!(
            snapshot.raw_output.len() <= settings.stored_output_limit,
            "stored {} bytes, cap is {}",
            snapshot.raw_output.len(),
            settings.stored_output_limit
        );
        assert!(snapshot.raw_output.starts_with(TRUNCATION_NOTICE));
    }

    #[tokio::test]
    async fn reports_a_program_that_cannot_be_started() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut tool = shell_tool(directory.path().to_path_buf(), "", OutputFormat::Generic);
        tool.program = "zone-definitely-not-a-real-program".to_string();
        tool.arguments.clear();

        let snapshot = run(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(snapshot.outcome, RunOutcome::SpawnFailed);
        assert!(!snapshot.outcome.is_measured());
    }

    #[tokio::test]
    async fn reports_a_workspace_that_does_not_exist() {
        let tool = shell_tool(
            PathBuf::from("/zone/not/a/directory"),
            "echo hello",
            OutputFormat::Generic,
        );

        let captured = capture(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(captured.outcome, RunOutcome::SpawnFailed);
    }

    #[tokio::test]
    async fn parses_the_output_of_a_completed_tool() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tool = shell_tool(
            directory.path().to_path_buf(),
            "echo 'test result: ok. 615 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out'",
            OutputFormat::CargoTest,
        );

        let snapshot = run(
            &tool,
            &EvaluationSettings::default(),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(snapshot.outcome, RunOutcome::Completed);
        assert_eq!(snapshot.passed, 615);
        assert_eq!(snapshot.skipped, 2);
        assert_eq!(snapshot.exit_code, Some(0));
        assert_eq!(snapshot.category, EvalCategory::Test);
    }

    #[test]
    fn keeps_short_output_untouched() {
        assert_eq!(truncate("all good", 1024), "all good");
        assert_eq!(truncate("", 1024), "");
    }

    #[test]
    fn keeps_the_tail_of_long_output() {
        let text = format!("{}test result: ok. 615 passed", "noise\n".repeat(1000));

        let truncated = truncate(&text, 64);

        assert!(truncated.len() <= 64);
        assert!(truncated.starts_with(TRUNCATION_NOTICE));
        assert!(
            truncated.ends_with("615 passed"),
            "the verdict is the last thing a tool prints"
        );
    }

    #[test]
    fn never_splits_a_multibyte_character() {
        let text = "\u{1f600}".repeat(500);

        let truncated = truncate(&text, 64);

        assert!(truncated.len() <= 64);
        assert!(
            truncated
                .chars()
                .all(|c| c == '\u{1f600}' || TRUNCATION_NOTICE.contains(c))
        );
    }

    #[test]
    fn gives_up_when_the_limit_cannot_hold_the_notice() {
        assert_eq!(truncate("some long output here", 4), "");
    }

    #[test]
    fn keeps_output_that_is_exactly_at_the_limit() {
        let text = "a".repeat(100);
        assert_eq!(truncate(&text, 100), text);
    }
}
