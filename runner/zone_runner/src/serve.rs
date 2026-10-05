//! Daemon mode implementation for the runner.
//!
//! In daemon mode, the runner communicates over stdin/stdout using NDJSON.

use abnegate_exec::{
    CommandExecutor, EnvironmentPolicy, ErrorCode, ExecutorConfig, Hello, InboundMessage,
    JobRegistry, OutboundMessage, RunCancel, RunStart, RunStdin,
};
use base64::prelude::*;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdout};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

/// Run the daemon, communicating over stdio.
pub async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(tokio::io::stdin());

    let registry = Arc::new(JobRegistry::new());
    let executor = Arc::new(CommandExecutor::with_config(
        ExecutorConfig::default()
            .with_environment(EnvironmentPolicy::inherit())
            .with_proxy(zone_routing::proxy()),
    ));

    let (sender, mut receiver) = mpsc::channel::<OutboundMessage>(1000);

    let writer = {
        let registry = Arc::clone(&registry);
        tokio::spawn(async move {
            let mut stdout = tokio::io::stdout();
            while let Some(message) = receiver.recv().await {
                registry.observe(&message);
                if !write_line(&mut stdout, &message).await {
                    break;
                }
            }
        })
    };

    let mut hello_received = false;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            tracing::info!("EOF on stdin, shutting down");
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let message: InboundMessage = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => {
                tracing::error!("Failed to parse message: {} (line: {})", error, line);
                sender
                    .send(OutboundMessage::error(
                        "",
                        ErrorCode::InvalidMessage,
                        format!("Failed to parse message: {}", error),
                    ))
                    .await?;
                continue;
            }
        };

        if !hello_received {
            match message {
                InboundMessage::Hello(Hello {
                    protocol_version,
                    capabilities,
                    ..
                }) => {
                    tracing::info!(
                        "Received Hello: version={}, capabilities={:?}",
                        protocol_version,
                        capabilities
                    );

                    if !protocol_version.starts_with("1.") {
                        sender
                            .send(OutboundMessage::error(
                                "",
                                ErrorCode::InvalidMessage,
                                format!(
                                    "Unsupported protocol version: {} (expected 1.x)",
                                    protocol_version
                                ),
                            ))
                            .await?;
                        break;
                    }

                    sender.send(OutboundMessage::hello_acknowledged()).await?;
                    hello_received = true;
                    tracing::info!("Handshake complete");
                }
                _ => {
                    sender
                        .send(OutboundMessage::error(
                            "",
                            ErrorCode::InvalidMessage,
                            "Expected Hello message",
                        ))
                        .await?;
                }
            }
            continue;
        }

        handle_message(message, &registry, &executor, sender.clone()).await?;
    }

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let message: InboundMessage = match serde_json::from_str(trimmed) {
            Ok(message) => message,
            Err(error) => {
                tracing::error!("Failed to parse message: {}", error);
                continue;
            }
        };

        if let Err(error) = handle_message(message, &registry, &executor, sender.clone()).await {
            tracing::error!("Error handling message: {}", error);
        }
    }

    tracing::info!("Shutting down, cancelling {} jobs", registry.active_count());
    registry.cancel_all();

    drop(sender);
    writer.await?;

    Ok(())
}

/// Write `message` as one line of JSON, reporting whether stdout still takes
/// writes.
async fn write_line(stdout: &mut Stdout, message: &OutboundMessage) -> bool {
    let json = match serde_json::to_string(message) {
        Ok(json) => json,
        Err(error) => {
            tracing::error!("Failed to serialize message: {}", error);
            return true;
        }
    };

    for bytes in [json.as_bytes(), b"\n"] {
        if let Err(error) = stdout.write_all(bytes).await {
            tracing::error!("Failed to write to stdout: {}", error);
            return false;
        }
    }
    if let Err(error) = stdout.flush().await {
        tracing::error!("Failed to flush stdout: {}", error);
        return false;
    }
    true
}

async fn handle_message(
    message: InboundMessage,
    registry: &Arc<JobRegistry>,
    executor: &Arc<CommandExecutor>,
    sender: mpsc::Sender<OutboundMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    match message {
        InboundMessage::Hello(_) => {
            sender
                .send(OutboundMessage::error(
                    "",
                    ErrorCode::InvalidMessage,
                    "Unexpected Hello message after handshake",
                ))
                .await?;
        }

        InboundMessage::RunStart(request) => start(request, registry, executor, sender).await?,

        InboundMessage::RunStdin(RunStdin {
            job_id, data, eof, ..
        }) => {
            tracing::debug!("RunStdin: job_id={}, eof={}", job_id, eof);

            let bytes = match BASE64_STANDARD.decode(&data) {
                Ok(bytes) => bytes,
                Err(error) => {
                    sender
                        .send(protocol_error(format!(
                            "Invalid base64 stdin for job {job_id}: {error}"
                        )))
                        .await?;
                    return Ok(());
                }
            };

            if let Some(stdin) = registry.get_stdin(&job_id)
                && !bytes.is_empty()
            {
                match stdin.try_send(bytes) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        sender
                            .send(protocol_error(format!("stdin for job {job_id} is full")))
                            .await?;
                        return Ok(());
                    }
                    Err(TrySendError::Closed(_)) => {
                        tracing::warn!("Failed to send stdin data: job_id={}", job_id);
                    }
                }
            }

            if eof {
                registry.close_stdin(&job_id);
            }
        }

        InboundMessage::RunCancel(RunCancel { job_id, force, .. }) => {
            tracing::info!("RunCancel: job_id={}, force={}", job_id, force);

            if let Err(error) = registry.cancel(&job_id, force) {
                sender
                    .send(OutboundMessage::error(
                        &job_id,
                        ErrorCode::JobNotFound,
                        error.to_string(),
                    ))
                    .await?;
            }
        }

        InboundMessage::Ping(ping) => {
            tracing::debug!("Ping: id={}", ping.id);
            sender.send(OutboundMessage::pong(ping.id)).await?;
        }

        _ => {
            sender
                .send(OutboundMessage::error(
                    "",
                    ErrorCode::InvalidMessage,
                    "Unsupported message",
                ))
                .await?;
        }
    }

    Ok(())
}

/// An error about the message itself rather than a job. It names no job, so
/// the registry never mistakes it for the end of a running one.
fn protocol_error(message: impl Into<String>) -> OutboundMessage {
    OutboundMessage::error("", ErrorCode::InvalidMessage, message)
}

async fn start(
    request: RunStart,
    registry: &Arc<JobRegistry>,
    executor: &Arc<CommandExecutor>,
    sender: mpsc::Sender<OutboundMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    let job_id = request.job_id.clone();
    tracing::info!(
        "RunStart: job_id={}, command={}, workspace={}",
        job_id,
        request.command,
        request.workspace.display()
    );

    let cancellation = match registry.register(job_id.clone()) {
        Ok(token) => token,
        Err(error) => {
            sender
                .send(protocol_error(format!(
                    "Failed to register job {job_id}: {error}"
                )))
                .await?;
            return Ok(());
        }
    };

    match executor
        .spawn_with_cancellation(&request, sender.clone(), cancellation)
        .await
    {
        Ok(handle) => {
            if let Err(error) = registry.set_process_group(&job_id, handle.process_group.clone()) {
                tracing::warn!("Failed to store process group: {}", error);
            }
            if let Some(stdin) = handle.stdin
                && let Err(error) = registry.set_stdin_handle(&job_id, stdin)
            {
                tracing::debug!("Job finished before its stdin was stored: {}", error);
            }
            tracing::debug!("Command spawned: job_id={}", job_id);
        }
        Err(error) => {
            tracing::error!("Failed to spawn command: {}", error);
            registry.remove(&job_id);
            sender
                .send(OutboundMessage::error(
                    &job_id,
                    error.to_error_code(),
                    error.to_string(),
                ))
                .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hello_ack_format() {
        let message = OutboundMessage::hello_acknowledged();
        let json = serde_json::to_string(&message).unwrap();

        assert!(json.contains("HelloAck"));
        assert!(json.contains("protocol_version"));
        assert!(json.contains("runner_version"));
        assert!(json.contains("capabilities"));
    }
}
