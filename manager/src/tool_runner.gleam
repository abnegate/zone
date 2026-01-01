/// Tool Runner
/// Abstraction for executing commands via the Rust tool runner process
import gleam/erlang/process.{type Subject}
import gleam/option.{type Option}

/// Result of starting a command
pub type RunResult {
  RunResult(job_id: String, pid: Int)
}

/// Progress messages from the runner
pub type RunProgress {
  /// Stdout output chunk (base64 decoded to BitArray)
  Stdout(job_id: String, data: BitArray, sequence: Int)
  /// Stderr output chunk (base64 decoded to BitArray)
  Stderr(job_id: String, data: BitArray, sequence: Int)
  /// Structured log message
  Log(job_id: String, level: String, message: String)
  /// Command exited normally
  Exit(
    job_id: String,
    exit_code: Option(Int),
    signal: Option(Int),
    duration_ms: Int,
  )
  /// Command encountered an error
  Error(job_id: String, error_code: String, message: String)
}

/// Protocol messages sent to the runner
pub type RunnerMessage {
  /// Handshake message
  Hello(protocol_version: String, capabilities: List(String))
  /// Start a command
  RunStart(
    job_id: String,
    workspace: String,
    command: String,
    args: List(String),
    env: List(#(String, String)),
    timeout_ms: Option(Int),
    max_output_bytes: Option(Int),
  )
  /// Send data to stdin
  RunStdin(job_id: String, data: BitArray, eof: Bool)
  /// Cancel a running command
  RunCancel(job_id: String, force: Bool)
  /// Health check
  Ping(id: String)
}

/// Protocol messages received from the runner
pub type RunnerResponse {
  /// Handshake acknowledgment
  HelloAck(
    protocol_version: String,
    runner_version: String,
    capabilities: List(String),
  )
  /// Command started
  RunStarted(job_id: String, pid: Int)
  /// Stdout chunk
  RunStdout(job_id: String, data: String, sequence: Int)
  /// Stderr chunk
  RunStderr(job_id: String, data: String, sequence: Int)
  /// Structured log
  RunLog(job_id: String, level: String, message: String)
  /// Command exited
  RunExit(
    job_id: String,
    exit_code: Option(Int),
    signal: Option(Int),
    duration_ms: Int,
  )
  /// Command error
  RunError(job_id: String, error_code: String, message: String)
  /// Pong response
  Pong(id: String)
}

/// Tool runner configuration
pub type RunnerConfig {
  RunnerConfig(
    /// Path to the zone-runner binary
    binary_path: String,
    /// Default timeout in milliseconds
    default_timeout_ms: Int,
    /// Maximum output bytes before truncation
    max_output_bytes: Int,
  )
}

/// Default runner configuration
pub fn default_config() -> RunnerConfig {
  RunnerConfig(
    // Will be in priv/bin relative to the application
    binary_path: "priv/bin/zone-runner",
    default_timeout_ms: 300_000,
    // 5 minutes
    max_output_bytes: 10_485_760,
  )
  // 10 MB
}
