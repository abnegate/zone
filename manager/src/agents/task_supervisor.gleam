/// Task Supervisor
/// OTP supervisor for managing task worker actors
import agents/task_worker.{type Message, type ProgressMessage}
import agents/task_worker/types.{Cancel, Execute, Shutdown}
import database/connection.{type Connection}
import gleam/dict.{type Dict}
import gleam/erlang/process.{type Subject}
import gleam/otp/actor
import gleam/result

/// Supervisor state
pub type SupervisorState {
  SupervisorState(
    db: Connection,
    workers: Dict(String, Subject(Message)),
    max_workers: Int,
  )
}

/// Messages for the supervisor
pub type SupervisorMessage {
  /// Start a new task execution
  StartTask(
    task_id: String,
    progress_subject: Subject(ProgressMessage),
    reply_to: Subject(Result(String, String)),
  )
  /// Cancel a task by run_id
  CancelTask(run_id: String, reply_to: Subject(Result(Nil, String)))
  /// Worker completed/failed, remove it
  WorkerDone(worker_id: String)
  /// Get active worker count
  GetWorkerCount(reply_to: Subject(Int))
  /// Shutdown all workers
  ShutdownAll
}

/// Start the supervisor
pub fn start(
  db: Connection,
  max_workers: Int,
) -> Result(Subject(SupervisorMessage), actor.StartError) {
  let initial_state = SupervisorState(db, dict.new(), max_workers)

  actor.new(initial_state)
  |> actor.on_message(handle_message)
  |> actor.start
  |> result.map(fn(started) { started.data })
}

/// Handle supervisor messages (note: actor.on_message expects fn(state, message))
fn handle_message(
  state: SupervisorState,
  message: SupervisorMessage,
) -> actor.Next(SupervisorState, SupervisorMessage) {
  case message {
    StartTask(task_id, progress_subject, reply_to) -> {
      // Check if we have capacity
      case dict.size(state.workers) >= state.max_workers {
        True -> {
          process.send(reply_to, Error("Maximum concurrent tasks reached"))
          actor.continue(state)
        }
        False -> {
          // Start a new worker
          case task_worker.start(state.db, progress_subject) {
            Ok(worker) -> {
              // Generate worker ID
              let worker_id = task_id

              // Execute the task
              let result_subject = process.new_subject()
              process.send(worker, Execute(task_id, result_subject))

              // Wait for result (with timeout)
              case process.receive(result_subject, 30_000) {
                Ok(task_result) -> {
                  process.send(reply_to, task_result)
                  // Add worker to tracking
                  let workers = dict.insert(state.workers, worker_id, worker)
                  actor.continue(SupervisorState(..state, workers: workers))
                }
                Error(_) -> {
                  process.send(reply_to, Error("Task start timeout"))
                  // Shutdown the stuck worker
                  process.send(worker, Shutdown)
                  actor.continue(state)
                }
              }
            }
            Error(_) -> {
              process.send(reply_to, Error("Failed to start worker"))
              actor.continue(state)
            }
          }
        }
      }
    }

    CancelTask(run_id, reply_to) -> {
      case dict.get(state.workers, run_id) {
        Ok(worker) -> {
          let result_subject = process.new_subject()
          process.send(worker, Cancel(result_subject))
          case process.receive(result_subject, 5000) {
            Ok(cancel_result) -> process.send(reply_to, cancel_result)
            Error(_) -> process.send(reply_to, Error("Cancel timeout"))
          }
          actor.continue(state)
        }
        Error(_) -> {
          process.send(reply_to, Error("Worker not found"))
          actor.continue(state)
        }
      }
    }

    WorkerDone(worker_id) -> {
      let workers = dict.delete(state.workers, worker_id)
      actor.continue(SupervisorState(..state, workers: workers))
    }

    GetWorkerCount(reply_to) -> {
      process.send(reply_to, dict.size(state.workers))
      actor.continue(state)
    }

    ShutdownAll -> {
      // Send shutdown to all workers
      dict.each(state.workers, fn(_id, worker) {
        process.send(worker, Shutdown)
      })
      actor.stop()
    }
  }
}

/// Start a task through the supervisor
pub fn start_task(
  supervisor: Subject(SupervisorMessage),
  task_id: String,
  progress_subject: Subject(ProgressMessage),
) -> Result(String, String) {
  let reply_subject = process.new_subject()
  process.send(supervisor, StartTask(task_id, progress_subject, reply_subject))
  // Wait for response with 60 second timeout
  case process.receive(reply_subject, 60_000) {
    Ok(task_result) -> task_result
    Error(_) -> Error("Supervisor timeout")
  }
}

/// Cancel a task through the supervisor
pub fn cancel_task(
  supervisor: Subject(SupervisorMessage),
  run_id: String,
) -> Result(Nil, String) {
  let reply_subject = process.new_subject()
  process.send(supervisor, CancelTask(run_id, reply_subject))
  case process.receive(reply_subject, 10_000) {
    Ok(cancel_result) -> cancel_result
    Error(_) -> Error("Cancel timeout")
  }
}

/// Get number of active workers
pub fn worker_count(supervisor: Subject(SupervisorMessage)) -> Int {
  let reply_subject = process.new_subject()
  process.send(supervisor, GetWorkerCount(reply_subject))
  case process.receive(reply_subject, 5000) {
    Ok(count) -> count
    Error(_) -> 0
  }
}

/// Shutdown all workers gracefully
pub fn shutdown(supervisor: Subject(SupervisorMessage)) -> Nil {
  process.send(supervisor, ShutdownAll)
  Nil
}
