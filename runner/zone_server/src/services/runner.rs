//! Runner service for executing commands via zone_runner subprocess
//!
//! This service manages a zone_runner subprocess running in daemon mode,
//! communicating via NDJSON protocol on stdin/stdout.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

/// Runner service error
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("Failed to spawn runner process: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("Runner process exited unexpectedly")]
    ProcessExited,
    #[error("Failed to send command: {0}")]
    SendFailed(String),
    #[error("Failed to receive response: {0}")]
    ReceiveFailed(String),
    #[error("Runner returned error: {0}")]
    RunnerError(String),
    #[error("Invalid JSON: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Timeout waiting for response")]
    Timeout,
}

/// Inbound message from zone_runner (responses)
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundMessage {
    /// Runner is ready and advertising capabilities
    Ready {
        version: String,
        capabilities: Vec<String>,
    },
    /// Log message from runner or job
    Log {
        job_id: Option<String>,
        level: String,
        message: String,
    },
    /// Job started
    JobStarted { job_id: String },
    /// Output from a running job
    Output {
        job_id: String,
        stream: String, // "stdout" or "stderr"
        data: String,
    },
    /// Job completed successfully
    JobCompleted { job_id: String, exit_code: i32 },
    /// Job failed with error
    JobFailed { job_id: String, error: String },
    /// Error response
    Error { code: String, message: String },
}

/// Outbound message to zone_runner (commands)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundMessage {
    /// Spawn a new job
    Spawn {
        job_id: String,
        workspace: String,
        command: String,
        args: Vec<String>,
        env: Option<std::collections::HashMap<String, String>>,
        timeout_secs: Option<u64>,
    },
    /// Cancel a running job
    Cancel { job_id: String },
    /// Shutdown the runner
    Shutdown,
}

/// Job execution result
#[derive(Debug, Clone)]
pub struct JobResult {
    pub job_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Runner service manages a zone_runner subprocess
pub struct RunnerService {
    inner: Arc<Mutex<RunnerServiceInner>>,
}

struct RunnerServiceInner {
    child: Option<Child>,
    tx: Option<mpsc::UnboundedSender<OutboundMessage>>,
    jobs: std::collections::HashMap<String, JobHandle>,
}

#[derive(Clone)]
struct JobHandle {
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    exit_code: Arc<Mutex<Option<i32>>>,
    error: Arc<Mutex<Option<String>>>,
    completed: Arc<tokio::sync::Notify>,
}

impl RunnerService {
    /// Create a new runner service and spawn the zone_runner process
    pub async fn new() -> Result<Self, RunnerError> {
        let mut child = Command::new("zone-runner")
            .arg("serve")
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| {
            RunnerError::SpawnFailed(std::io::Error::other("Failed to get stdin"))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            RunnerError::SpawnFailed(std::io::Error::other("Failed to get stdout"))
        })?;

        let (tx, mut rx) = mpsc::unbounded_channel::<OutboundMessage>();

        let inner = Arc::new(Mutex::new(RunnerServiceInner {
            child: Some(child),
            tx: Some(tx.clone()),
            jobs: std::collections::HashMap::new(),
        }));

        // Spawn task to write messages to stdin
        let mut stdin = stdin;
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Ok(json) = serde_json::to_string(&msg) {
                    if stdin.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdin.write_all(b"\n").await.is_err() {
                        break;
                    }
                    if stdin.flush().await.is_err() {
                        break;
                    }
                }
            }
        });

        // Spawn task to read messages from stdout
        let inner_clone = inner.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(msg) = serde_json::from_str::<InboundMessage>(&line) {
                    Self::handle_message(inner_clone.clone(), msg).await;
                }
            }
        });

        Ok(Self { inner })
    }

    /// Handle an inbound message from the runner
    async fn handle_message(inner: Arc<Mutex<RunnerServiceInner>>, msg: InboundMessage) {
        match msg {
            InboundMessage::Ready { version, .. } => {
                tracing::info!("Runner ready: version={}", version);
            }
            InboundMessage::Log {
                job_id,
                level,
                message,
            } => {
                tracing::debug!(
                    "Runner log: job_id={:?}, level={}, message={}",
                    job_id,
                    level,
                    message
                );
            }
            InboundMessage::JobStarted { job_id } => {
                tracing::debug!("Job started: {}", job_id);
            }
            InboundMessage::Output {
                job_id,
                stream,
                data,
            } => {
                let guard = inner.lock().await;
                if let Some(job) = guard.jobs.get(&job_id) {
                    if stream == "stdout" {
                        let mut stdout = job.stdout.lock().await;
                        stdout.push_str(&data);
                        stdout.push('\n');
                    } else if stream == "stderr" {
                        let mut stderr = job.stderr.lock().await;
                        stderr.push_str(&data);
                        stderr.push('\n');
                    }
                }
            }
            InboundMessage::JobCompleted { job_id, exit_code } => {
                let guard = inner.lock().await;
                if let Some(job) = guard.jobs.get(&job_id) {
                    *job.exit_code.lock().await = Some(exit_code);
                    job.completed.notify_waiters();
                }
            }
            InboundMessage::JobFailed { job_id, error } => {
                let guard = inner.lock().await;
                if let Some(job) = guard.jobs.get(&job_id) {
                    *job.error.lock().await = Some(error);
                    job.completed.notify_waiters();
                }
            }
            InboundMessage::Error { code, message } => {
                tracing::error!("Runner error: code={}, message={}", code, message);
            }
        }
    }

    /// Execute a command and wait for completion
    pub async fn execute(
        &self,
        workspace: PathBuf,
        command: String,
        args: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<JobResult, RunnerError> {
        let job_id = Uuid::new_v4().to_string();

        // Create job handle
        let handle = JobHandle {
            stdout: Arc::new(Mutex::new(String::new())),
            stderr: Arc::new(Mutex::new(String::new())),
            exit_code: Arc::new(Mutex::new(None)),
            error: Arc::new(Mutex::new(None)),
            completed: Arc::new(tokio::sync::Notify::new()),
        };

        // Register job
        {
            let mut guard = self.inner.lock().await;
            guard.jobs.insert(job_id.clone(), handle.clone());
        }

        // Send spawn message
        let msg = OutboundMessage::Spawn {
            job_id: job_id.clone(),
            workspace: workspace.to_string_lossy().to_string(),
            command,
            args,
            env: None,
            timeout_secs,
        };

        {
            let guard = self.inner.lock().await;
            if let Some(tx) = &guard.tx {
                tx.send(msg)
                    .map_err(|e| RunnerError::SendFailed(e.to_string()))?;
            } else {
                return Err(RunnerError::ProcessExited);
            }
        }

        // Wait for completion with timeout
        let timeout_duration = std::time::Duration::from_secs(timeout_secs.unwrap_or(300));
        tokio::time::timeout(timeout_duration, handle.completed.notified())
            .await
            .map_err(|_| RunnerError::Timeout)?;

        // Get results
        let exit_code = handle.exit_code.lock().await.unwrap_or(-1);
        let error = handle.error.lock().await.clone();
        let stdout = handle.stdout.lock().await.clone();
        let stderr = handle.stderr.lock().await.clone();

        // Clean up job handle
        {
            let mut guard = self.inner.lock().await;
            guard.jobs.remove(&job_id);
        }

        if let Some(error_msg) = error {
            return Err(RunnerError::RunnerError(error_msg));
        }

        Ok(JobResult {
            job_id,
            exit_code,
            stdout,
            stderr,
            success: exit_code == 0,
        })
    }

    /// Shutdown the runner process
    pub async fn shutdown(&self) -> Result<(), RunnerError> {
        let mut guard = self.inner.lock().await;

        if let Some(tx) = &guard.tx {
            let _ = tx.send(OutboundMessage::Shutdown);
        }

        if let Some(mut child) = guard.child.take() {
            let _ = child.kill().await;
        }

        guard.tx = None;
        guard.jobs.clear();

        Ok(())
    }
}

impl Drop for RunnerService {
    fn drop(&mut self) {
        // Best effort cleanup - spawn blocking task
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Runtime::new() {
                runtime.block_on(async {
                    let mut guard = inner.lock().await;
                    if let Some(mut child) = guard.child.take() {
                        let _ = child.kill().await;
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires zone-runner binary
    async fn test_runner_service_spawn() {
        let service = RunnerService::new().await;
        assert!(service.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires zone-runner binary
    async fn test_runner_service_execute_echo() {
        let service = RunnerService::new().await.unwrap();
        let workspace = std::env::temp_dir();

        let result = service
            .execute(
                workspace,
                "echo".to_string(),
                vec!["hello world".to_string()],
                Some(5),
            )
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    #[ignore] // Requires zone-runner binary
    async fn test_runner_service_execute_timeout() {
        let service = RunnerService::new().await.unwrap();
        let workspace = std::env::temp_dir();

        let result = service
            .execute(
                workspace,
                "sleep".to_string(),
                vec!["10".to_string()],
                Some(1),
            )
            .await;

        assert!(matches!(result, Err(RunnerError::Timeout)));
    }
}
