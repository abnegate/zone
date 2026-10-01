//! One-shot execution mode (for future standalone usage).
//!
//! This mode allows running a single command without the daemon protocol.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tokio::sync::mpsc;

use abnegate_exec::{
    CommandExecutor, EnvironmentPolicy, ExecutorConfig, OutboundMessage, RunStart,
};

/// Run a single command and exit.
pub async fn run_once(
    workspace: PathBuf,
    command: String,
    args: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let config = ExecutorConfig::default().with_environment(EnvironmentPolicy::inherit());
    let config = match timeout_secs {
        Some(secs) => config.with_timeout(Duration::from_secs(secs)),
        None => config,
    };

    let executor = CommandExecutor::with_config(config);
    let (tx, mut rx) = mpsc::channel::<OutboundMessage>(100);

    let request = RunStart::new("exec", workspace, command).with_arguments(args);

    let _handle = executor.spawn(&request, tx).await?;

    let mut exit_code = ExitCode::SUCCESS;
    let stdout = tokio::io::stdout();
    let stderr = tokio::io::stderr();

    use base64::prelude::*;
    use tokio::io::AsyncWriteExt;

    let mut stdout = stdout;
    let mut stderr = stderr;

    while let Some(msg) = rx.recv().await {
        match msg {
            OutboundMessage::RunStdout { data, .. } => {
                if let Ok(bytes) = BASE64_STANDARD.decode(&data) {
                    let _ = stdout.write_all(&bytes).await;
                }
            }
            OutboundMessage::RunStderr { data, .. } => {
                if let Ok(bytes) = BASE64_STANDARD.decode(&data) {
                    let _ = stderr.write_all(&bytes).await;
                }
            }
            OutboundMessage::RunExit {
                exit_code: code, ..
            } => {
                exit_code = match code {
                    Some(0) => ExitCode::SUCCESS,
                    Some(c) => ExitCode::from(c as u8),
                    None => ExitCode::FAILURE,
                };
                break;
            }
            OutboundMessage::RunError { message, .. } => {
                eprintln!("Error: {}", message);
                exit_code = ExitCode::FAILURE;
                break;
            }
            _ => {}
        }
    }

    let _ = stdout.flush().await;
    let _ = stderr.flush().await;

    Ok(exit_code)
}
