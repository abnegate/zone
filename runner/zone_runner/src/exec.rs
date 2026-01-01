//! One-shot execution mode (for future standalone usage).
//!
//! This mode allows running a single command without the daemon protocol.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tokio::sync::mpsc;

use tool_runner::{CommandExecutor, ExecutorConfig, InboundMessage, OutboundMessage};

/// Run a single command and exit.
pub async fn run_once(
    workspace: PathBuf,
    command: String,
    args: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let config = if let Some(secs) = timeout_secs {
        ExecutorConfig::default().with_timeout(Duration::from_secs(secs))
    } else {
        ExecutorConfig::default()
    };

    let executor = CommandExecutor::with_config(config);
    let (tx, mut rx) = mpsc::channel::<OutboundMessage>(100);

    let request = InboundMessage::RunStart {
        job_id: "exec".to_string(),
        workspace,
        command,
        args,
        env: HashMap::new(),
        timeout_ms: timeout_secs.map(|s| s * 1000),
        max_output_bytes: None,
        working_dir: None,
    };

    // Spawn the command
    let _handle = executor.spawn(&request, tx).await?;

    // Collect output and wait for exit
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
