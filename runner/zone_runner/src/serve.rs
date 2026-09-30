//! Daemon mode implementation for the runner.
//!
//! In daemon mode, the runner communicates over stdin/stdout using NDJSON.

use abnegate_exec::{
    CommandExecutor, EnvironmentPolicy, ErrorCode, ExecutorConfig, Hello, InboundMessage,
    JobRegistry, OutboundMessage, RunCancel, RunStart, RunStdin,
};
use base64::prelude::*;
use serde::Serialize;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdout};
use tokio::sync::{Mutex, mpsc};

/// The answer to a `Ping`.
///
/// `OutboundMessage::Pong` cannot be built outside `abnegate-exec`, so the
/// runner writes the same wire shape itself.
#[derive(Serialize)]
#[serde(tag = "type")]
struct Pong {
    id: String,
}

/// Run the daemon, communicating over stdio.
pub async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = tokio::io::stdin();
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));

    let mut reader = BufReader::new(stdin);

    let registry = Arc::new(JobRegistry::new());
    let executor = Arc::new(CommandExecutor::with_config(
        ExecutorConfig::default().with_environment(EnvironmentPolicy::inherit()),
    ));

    let (tx, mut rx) = mpsc::channel::<OutboundMessage>(1000);

    let writer_handle = {
        let stdout = Arc::clone(&stdout);
        let registry = Arc::clone(&registry);
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                registry.observe(&msg);
                if !write_line(&stdout, &msg).await {
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

                    tx.send(OutboundMessage::hello_acknowledged()).await?;
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

        handle_message(msg, &registry, &executor, &stdout, tx.clone()).await?;
    }

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

        if let Err(e) = handle_message(msg, &registry, &executor, &stdout, tx.clone()).await {
            tracing::error!("Error handling message: {}", e);
        }
    }

    tracing::info!("Shutting down, cancelling {} jobs", registry.active_count());
    registry.cancel_all();

    drop(tx);
    writer_handle.await?;

    Ok(())
}

/// Write `message` as one line of JSON, reporting whether stdout still takes
/// writes.
async fn write_line(stdout: &Mutex<Stdout>, message: &impl Serialize) -> bool {
    let json = match serde_json::to_string(message) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!("Failed to serialize message: {}", e);
            return true;
        }
    };

    let mut stdout = stdout.lock().await;
    for bytes in [json.as_bytes(), b"\n"] {
        if let Err(e) = stdout.write_all(bytes).await {
            tracing::error!("Failed to write to stdout: {}", e);
            return false;
        }
    }
    if let Err(e) = stdout.flush().await {
        tracing::error!("Failed to flush stdout: {}", e);
        return false;
    }
    true
}

/// Handle an inbound message.
async fn handle_message(
    msg: InboundMessage,
    registry: &Arc<JobRegistry>,
    executor: &Arc<CommandExecutor>,
    stdout: &Mutex<Stdout>,
    tx: mpsc::Sender<OutboundMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
    match msg {
        InboundMessage::Hello(_) => {
            tx.send(OutboundMessage::error(
                "",
                ErrorCode::InvalidMessage,
                "Unexpected Hello message after handshake",
            ))
            .await?;
        }

        InboundMessage::RunStart(request) => start(request, registry, executor, tx).await?,

        InboundMessage::RunStdin(RunStdin {
            job_id, data, eof, ..
        }) => {
            tracing::debug!("RunStdin: job_id={}, eof={}", job_id, eof);

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

            if let Some(stdin_tx) = registry.get_stdin(&job_id)
                && stdin_tx.send(bytes).await.is_err()
            {
                tracing::warn!("Failed to send stdin data: job_id={}", job_id);
            }

            if eof {
                registry.close_stdin(&job_id);
            }
        }

        InboundMessage::RunCancel(RunCancel { job_id, force, .. }) => {
            tracing::info!("RunCancel: job_id={}, force={}", job_id, force);

            if let Err(e) = registry.cancel(&job_id, force) {
                tx.send(OutboundMessage::error(
                    &job_id,
                    ErrorCode::JobNotFound,
                    e.to_string(),
                ))
                .await?;
            }
        }

        InboundMessage::Ping(ping) => {
            tracing::debug!("Ping: id={}", ping.id);
            write_line(stdout, &Pong { id: ping.id }).await;
        }

        _ => {
            tx.send(OutboundMessage::error(
                "",
                ErrorCode::InvalidMessage,
                "Unsupported message",
            ))
            .await?;
        }
    }

    Ok(())
}

async fn start(
    request: RunStart,
    registry: &Arc<JobRegistry>,
    executor: &Arc<CommandExecutor>,
    tx: mpsc::Sender<OutboundMessage>,
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

    match executor
        .spawn_with_cancellation(&request, tx.clone(), cancellation)
        .await
    {
        Ok(handle) => {
            if let Err(e) = registry.set_process_group(&job_id, handle.process_group.clone()) {
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hello_ack_format() {
        let msg = OutboundMessage::hello_acknowledged();
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains("HelloAck"));
        assert!(json.contains("protocol_version"));
        assert!(json.contains("runner_version"));
        assert!(json.contains("capabilities"));
    }

    #[test]
    fn a_pong_reads_back_as_the_protocols_own() {
        let written = serde_json::to_string(&Pong {
            id: "ping-7".to_string(),
        })
        .unwrap();

        let read: OutboundMessage = serde_json::from_str(&written).unwrap();

        assert!(
            matches!(&read, OutboundMessage::Pong { id, .. } if id == "ping-7"),
            "{written} is not a Pong: {read:?}"
        );
    }
}
