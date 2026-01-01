/// Task execution orchestrator
/// Public API for task execution that delegates to the OTP supervisor
import agents/task_supervisor.{type SupervisorMessage}
import agents/task_worker.{type ProgressMessage}
import agents/task_worker/types.{Execute, Shutdown}
import database/connection.{type Connection}
import database/queries/tasks
import gleam/erlang/process.{type Subject}
import gleam/option.{None, Some}
import models/task

// Re-export types from task_worker for backward compatibility
pub type Progress =
  task_worker.ProgressMessage

pub type ExecutionArtifacts =
  task_worker.ExecutionArtifacts

// Re-export the progress_to_json function
pub const progress_to_json = task_worker.progress_to_json

/// Global supervisor subject (set at application startup)
type SupervisorRef =
  Subject(SupervisorMessage)

/// Start task execution through the supervisor
/// Returns the run ID immediately, execution happens asynchronously
pub fn start_task_execution(
  db: Connection,
  task_id: String,
  progress_subject: Subject(ProgressMessage),
) -> Result(String, String) {
  // Get the global supervisor
  case get_supervisor() {
    Some(supervisor) -> {
      task_supervisor.start_task(supervisor, task_id, progress_subject)
    }
    None -> {
      // Fallback: create a one-off worker (for cases where supervisor isn't running)
      start_task_direct(db, task_id, progress_subject)
    }
  }
}

/// Cancel a running task execution
pub fn cancel_task_execution(
  db: Connection,
  run_id: String,
) -> Result(Nil, String) {
  // Try supervisor first
  case get_supervisor() {
    Some(supervisor) -> task_supervisor.cancel_task(supervisor, run_id)
    None -> {
      // Fallback: direct cancellation
      cancel_task_direct(db, run_id)
    }
  }
}

/// Direct task start (fallback when supervisor not available)
fn start_task_direct(
  db: Connection,
  task_id: String,
  progress_subject: Subject(ProgressMessage),
) -> Result(String, String) {
  case task_worker.start(db, progress_subject) {
    Ok(worker) -> {
      let result_subject = process.new_subject()
      process.send(worker, Execute(task_id, result_subject))

      case process.receive(result_subject, 60_000) {
        Ok(result) -> result
        Error(_) -> {
          process.send(worker, Shutdown)
          Error("Task execution timeout")
        }
      }
    }
    Error(_) -> Error("Failed to start task worker")
  }
}

/// Direct task cancellation (fallback when supervisor not available)
fn cancel_task_direct(db: Connection, run_id: String) -> Result(Nil, String) {
  case tasks.get_task_run(db, run_id) {
    Ok(Some(run)) -> {
      case run.status {
        task.Running -> {
          let _ =
            tasks.complete_task_run(
              db,
              run_id,
              task.Cancelled,
              Some("Cancelled by user"),
            )
          Ok(Nil)
        }
        _ -> Error("Task run is not running")
      }
    }
    Ok(None) -> Error("Task run not found")
    Error(err) -> Error(err)
  }
}

/// Get the global supervisor (stored in process dictionary or ETS)
/// Returns None if supervisor hasn't been started
fn get_supervisor() -> option.Option(Subject(SupervisorMessage)) {
  // The supervisor reference is set by the application startup
  // For now, return None and use fallback mode
  // In production, this would read from a named process or ETS table
  None
}
