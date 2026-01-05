//! Daemon mode implementation for the runner.
//!
//! In daemon mode, the runner communicates over stdin/stdout using NDJSON.

use base64::prelude::*;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use tool_runner::{CommandExecutor, ErrorCode, InboundMessage, JobRegistry, OutboundMessage};

/// Run the daemon, communicating over stdio.
pub async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let mut reader = BufReader::new(stdin);

    // Set up message channel for outbound messages
    let (tx, mut rx) = mpsc::channel::<OutboundMessage>(1000);

    // Spawn output writer task
    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    tracing::error!("Failed to serialize message: {}", e);
                    continue;
                }
            };

            if let Err(e) = stdout.write_all(json.as_bytes()).await {
                tracing::error!("Failed to write to stdout: {}", e);
                break;
            }

            if let Err(e) = stdout.write_all(b"\n").await {
                tracing::error!("Failed to write newline: {}", e);
                break;
            }

            if let Err(e) = stdout.flush().await {
                tracing::error!("Failed to flush stdout: {}", e);
                break;
            }
        }
    });

    // Set up job registry and executor
    let registry = Arc::new(JobRegistry::new());
    let executor = Arc::new(CommandExecutor::new());

    // Wait for Hello message
    let mut hello_received = false;
    let mut line = String::new();

    // Read first line (Hello)
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            // EOF
            tracing::info!("EOF on stdin, shutting down");
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: InboundMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Failed to parse message: {} (line: {})", e, line);
                tx.send(OutboundMessage::error(
                    "",
                    ErrorCode::InvalidMessage,
                    format!("Failed to parse message: {}", e),
                ))
                .await?;
                continue;
            }
        };

        if !hello_received {
            match msg {
                InboundMessage::Hello {
                    protocol_version,
                    capabilities,
                } => {
                    tracing::info!(
                        "Received Hello: version={}, capabilities={:?}",
                        protocol_version,
                        capabilities
                    );

                    // Check protocol version compatibility
                    if !protocol_version.starts_with("1.") {
                        tx.send(OutboundMessage::error(
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

                    tx.send(OutboundMessage::hello_ack()).await?;
                    hello_received = true;
                    tracing::info!("Handshake complete");
                }
                _ => {
                    tx.send(OutboundMessage::error(
                        "",
                        ErrorCode::InvalidMessage,
                        "Expected Hello message",
                    ))
                    .await?;
                }
            }
            continue;
        }

        // Handle other messages
        handle_message(msg, &registry, &executor, tx.clone()).await?;
    }

    // Read remaining messages
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            break;
        }

        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            continue;
        }

        let msg: InboundMessage = match serde_json::from_str(line_trimmed) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Failed to parse message: {}", e);
                continue;
            }
        };

        if let Err(e) = handle_message(msg, &registry, &executor, tx.clone()).await {
            tracing::error!("Error handling message: {}", e);
        }
    }

    // Cleanup: cancel all running jobs
    tracing::info!("Shutting down, cancelling {} jobs", registry.active_count());
    registry.cancel_all();

    // Close the output channel
    drop(tx);
    writer_handle.await?;

    Ok(())
}

/// Handle an inbound message.
async fn handle_message(
    msg: InboundMessage,
    registry: &Arc<JobRegistry>,
    executor: &Arc<CommandExecutor>,
    tx: mpsc::Sender<OutboundMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    match msg {
        InboundMessage::Hello { .. } => {
            // Already handled in main loop
            tx.send(OutboundMessage::error(
                "",
                ErrorCode::InvalidMessage,
                "Unexpected Hello message after handshake",
            ))
            .await?;
        }

        InboundMessage::RunStart {
            job_id,
            workspace,
            command,
            args,
            env,
            timeout_ms,
            max_output_bytes,
            working_dir,
        } => {
            tracing::info!(
                "RunStart: job_id={}, command={}, workspace={}",
                job_id,
                command,
                workspace.display()
            );

            // Register the job
            let _cancel_token = match registry.register(job_id.clone()) {
                Ok(token) => token,
                Err(e) => {
                    tx.send(OutboundMessage::error(
                        &job_id,
                        ErrorCode::InternalError,
                        format!("Failed to register job: {}", e),
                    ))
                    .await?;
                    return Ok(());
                }
            };

            // Create the request message
            let request = InboundMessage::RunStart {
                job_id: job_id.clone(),
                workspace,
                command,
                args,
                env,
                timeout_ms,
                max_output_bytes,
                working_dir,
            };

            // Spawn the command
            match executor.spawn(&request, tx.clone()).await {
                Ok(handle) => {
                    // Store process group in registry
                    if let Err(e) =
                        registry.set_process_group(&job_id, handle.process_group.clone())
                    {
                        tracing::warn!("Failed to store process group: {}", e);
                    }
                    tracing::debug!("Command spawned: job_id={}", job_id);
                }
                Err(e) => {
                    tracing::error!("Failed to spawn command: {}", e);
                    registry.remove(&job_id);
                    tx.send(OutboundMessage::error(
                        &job_id,
                        e.to_error_code(),
                        e.to_string(),
                    ))
                    .await?;
                }
            }
        }

        InboundMessage::RunStdin { job_id, data, eof } => {
            tracing::debug!("RunStdin: job_id={}, eof={}", job_id, eof);

            // Decode base64 data
            let bytes = match BASE64_STANDARD.decode(&data) {
                Ok(b) => b,
                Err(e) => {
                    tx.send(OutboundMessage::error(
                        &job_id,
                        ErrorCode::InvalidMessage,
                        format!("Invalid base64 data: {}", e),
                    ))
                    .await?;
                    return Ok(());
                }
            };

            // Send to job's stdin
            if let Some(stdin_tx) = registry.get_stdin(&job_id) {
                if stdin_tx.send(bytes).await.is_err() {
                    tracing::warn!("Failed to send stdin data: job_id={}", job_id);
                }
            }

            if eof {
                registry.close_stdin(&job_id);
            }
        }

        InboundMessage::RunCancel { job_id, force } => {
            tracing::info!("RunCancel: job_id={}, force={}", job_id, force);

            match registry.cancel(&job_id, force) {
                Ok(_) => {
                    tracing::debug!("Cancelled job: {}", job_id);
                }
                Err(e) => {
                    tx.send(OutboundMessage::error(
                        &job_id,
                        ErrorCode::JobNotFound,
                        e.to_string(),
                    ))
                    .await?;
                }
            }
        }

        InboundMessage::Ping { id } => {
            tracing::debug!("Ping: id={}", id);
            tx.send(OutboundMessage::Pong { id }).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hello_ack_format() {
        let msg = OutboundMessage::hello_ack();
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains("HelloAck"));
        assert!(json.contains("protocol_version"));
        assert!(json.contains("runner_version"));
        assert!(json.contains("capabilities"));
    }
}
