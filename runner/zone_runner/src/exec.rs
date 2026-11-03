//! One-shot execution mode (for future standalone usage).
//!
//! This mode allows running a single command without the daemon protocol.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use abnegate_exec::{
    CommandExecutor, EnvironmentPolicy, ExecutorConfig, OutboundMessage, RunStart,
};
use base64::prelude::*;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Run a single command and exit.
pub async fn run_once(
    workspace: PathBuf,
    command: String,
    arguments: Vec<String>,
    timeout: Option<Duration>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut config = ExecutorConfig::default()
        .with_environment(EnvironmentPolicy::inherit())
        .with_proxy(zone_routing::proxy());
    if let Some(timeout) = timeout {
        config = config.with_timeout(timeout);
    }

    let executor = CommandExecutor::with_config(config);
    let (sender, mut receiver) = mpsc::channel::<OutboundMessage>(100);
    let request = RunStart::new("exec", workspace, command).with_arguments(arguments);

    executor.spawn(&request, sender).await?;

    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut exit_code = ExitCode::SUCCESS;

    while let Some(message) = receiver.recv().await {
        match message {
            OutboundMessage::RunStdout { data, .. } => forward(&mut stdout, &data).await,
            OutboundMessage::RunStderr { data, .. } => forward(&mut stderr, &data).await,
            OutboundMessage::RunExit {
                exit_code: code, ..
            } => {
                exit_code = match code {
                    Some(0) => ExitCode::SUCCESS,
                    Some(code) => ExitCode::from(code as u8),
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

async fn forward(output: &mut (impl AsyncWrite + Unpin), encoded: &str) {
    if let Ok(bytes) = BASE64_STANDARD.decode(encoded) {
        let _ = output.write_all(&bytes).await;
    }
}
