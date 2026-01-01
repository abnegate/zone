/// Task Worker Types
/// Shared types for task worker actors
import database/connection.{type Connection}
import gleam/erlang/process.{type Subject}
import gleam/option.{type Option}

/// Actor state for task execution
pub type State {
  State(
    db: Connection,
    progress_subject: Subject(ProgressMessage),
    current_run_id: Option(String),
    /// When true, actor will stop after finishing current work and reject new tasks
    shutting_down: Bool,
  )
}

/// Messages the actor can receive
pub type Message {
  /// Execute a task
  Execute(task_id: String, reply_to: Subject(Result(String, String)))
  /// Cancel current execution
  Cancel(reply_to: Subject(Result(Nil, String)))
  /// Shutdown the actor gracefully
  Shutdown
}

/// Progress update message for WebSocket streaming
pub type ProgressMessage {
  PhaseStarted(run_id: String, phase: String, progress: Int, message: String)
  PhaseCompleted(run_id: String, phase: String, progress: Int, message: String)
  LogEntry(
    run_id: String,
    phase: String,
    agent: String,
    level: String,
    message: String,
  )
  ExecutionCompleted(run_id: String, success: Bool, message: String)
  ExecutionFailed(run_id: String, error: String)
}

/// Artifacts collected during execution
pub type ExecutionArtifacts {
  ExecutionArtifacts(
    plan: Option(String),
    tests: Option(String),
    implementation: Option(String),
    review: Option(String),
    final_code: Option(String),
  )
}
