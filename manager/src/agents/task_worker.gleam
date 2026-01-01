/// Task Worker Actor
/// OTP-compliant actor for executing individual tasks with supervision support
import agents/agentic_executor
import agents/llm
import agents/prompts.{
  type AgentPhase, ArchitectPlanning, ArchitectReview, DeveloperFinal,
  DeveloperFixes, DeveloperImplementation, DeveloperTests, GrillerReview,
}
import agents/task_worker/progress
import agents/task_worker/types.{
  Cancel, Execute, ExecutionArtifacts, ExecutionCompleted, ExecutionFailed,
  LogEntry, PhaseCompleted, PhaseStarted, Shutdown, State,
}
import database/connection.{type Connection}
import database/queries/projects
import database/queries/tasks
import gleam/erlang/process.{type Subject}
import gleam/option.{type Option, None, Some}
import gleam/otp/actor
import gleam/result
import models/project.{type Project}
import models/task.{type Task, Completed, Failed, InProgress}

// Re-export types for external use
pub type State =
  types.State

pub type Message =
  types.Message

pub type ProgressMessage =
  types.ProgressMessage

pub type ExecutionArtifacts =
  types.ExecutionArtifacts

/// Convert progress message to JSON
pub fn progress_to_json(msg: ProgressMessage) -> String {
  progress.to_json(msg)
}

/// Start a new task worker actor
pub fn start(
  db: Connection,
  progress_subject: Subject(ProgressMessage),
) -> Result(Subject(Message), actor.StartError) {
  let initial_state =
    State(
      db: db,
      progress_subject: progress_subject,
      current_run_id: None,
      shutting_down: False,
    )

  actor.new(initial_state)
  |> actor.on_message(handle_message)
  |> actor.start
  |> result.map(fn(started) { started.data })
}

/// Handle incoming messages
fn handle_message(state: State, message: Message) -> actor.Next(State, Message) {
  case message {
    Execute(task_id, reply_to) -> {
      case state.shutting_down {
        True -> {
          process.send(reply_to, Error("Worker is shutting down"))
          actor.continue(state)
        }
        False -> {
          let exec_result = do_execute_task(state, task_id)
          process.send(reply_to, exec_result)

          case exec_result {
            Ok(run_id) -> {
              let new_state = State(..state, current_run_id: Some(run_id))
              maybe_shutdown_after_task(new_state)
            }
            Error(_) -> {
              maybe_shutdown_after_task(state)
            }
          }
        }
      }
    }

    Cancel(reply_to) -> {
      let cancel_result = do_cancel(state)
      process.send(reply_to, cancel_result)
      let new_state = State(..state, current_run_id: None)
      maybe_shutdown_after_task(new_state)
    }

    Shutdown -> {
      case state.current_run_id {
        None -> actor.stop()
        Some(_) -> {
          actor.continue(State(..state, shutting_down: True))
        }
      }
    }
  }
}

fn maybe_shutdown_after_task(state: State) -> actor.Next(State, Message) {
  case state.shutting_down {
    True -> actor.stop()
    False -> actor.continue(state)
  }
}

fn do_execute_task(state: State, task_id: String) -> Result(String, String) {
  case tasks.get_task(state.db, task_id) {
    Ok(Some(task)) -> {
      case projects.get_project(state.db, task.project_id) {
        Ok(Some(project)) -> {
          case validate_task_can_start(task) {
            Ok(_) -> {
              case tasks.create_task_run(state.db, task_id) {
                Ok(run) -> {
                  execute_task_phases(
                    state.db,
                    run.id,
                    task,
                    project,
                    state.progress_subject,
                  )
                  Ok(run.id)
                }
                Error(err) -> Error("Failed to create task run: " <> err)
              }
            }
            Error(err) -> Error(err)
          }
        }
        Ok(None) -> Error("Project not found")
        Error(err) -> Error("Failed to get project: " <> err)
      }
    }
    Ok(None) -> Error("Task not found")
    Error(err) -> Error("Failed to get task: " <> err)
  }
}

fn do_cancel(state: State) -> Result(Nil, String) {
  case state.current_run_id {
    Some(run_id) -> {
      case tasks.get_task_run(state.db, run_id) {
        Ok(Some(run)) -> {
          case run.status {
            task.Running -> {
              let _ =
                tasks.complete_task_run(
                  state.db,
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
    None -> Error("No task currently running")
  }
}

fn validate_task_can_start(task: Task) -> Result(Nil, String) {
  case task.status {
    task.Created -> Ok(Nil)
    task.Queued -> Ok(Nil)
    task.Blocked -> Ok(Nil)
    task.InProgress -> Error("Task is already in progress")
    task.Review -> Error("Task is under review")
    task.Complete -> Error("Task is already complete")
  }
}

fn execute_task_phases(
  db: Connection,
  run_id: String,
  task: Task,
  project: Project,
  progress_subject: Subject(ProgressMessage),
) -> Nil {
  let _ = tasks.update_task_status(db, task.id, InProgress)

  let result = case task.is_agentic {
    True -> {
      let agentic_subject = create_agentic_progress_adapter(progress_subject)
      agentic_executor.execute_agentic_task(
        db,
        run_id,
        task,
        project,
        agentic_subject,
      )
      |> result.map(fn(_) {
        ExecutionArtifacts(
          plan: None,
          tests: None,
          implementation: None,
          review: None,
          final_code: None,
        )
      })
    }
    False -> {
      let model = case task.model_name {
        Some(m) -> m
        None -> "llama3.2"
      }
      execute_phases(db, run_id, task, project, model, progress_subject)
    }
  }

  case result {
    Ok(_) -> {
      let _ = tasks.complete_task_run(db, run_id, Completed, None)
      let _ = tasks.update_task_status(db, task.id, task.Complete)
      process.send(
        progress_subject,
        ExecutionCompleted(run_id, True, "Task completed successfully"),
      )
      Nil
    }
    Error(err) -> {
      let _ = tasks.complete_task_run(db, run_id, Failed, Some(err))
      let _ = tasks.update_task_status(db, task.id, task.Blocked)
      process.send(progress_subject, ExecutionFailed(run_id, err))
      Nil
    }
  }
}

fn create_agentic_progress_adapter(
  main_subject: Subject(ProgressMessage),
) -> Subject(agentic_executor.ProgressMessage) {
  let handler = fn(msg: agentic_executor.ProgressMessage) {
    let converted = case msg {
      agentic_executor.AgenticPhaseStarted(run_id, phase, message) ->
        PhaseStarted(run_id, phase, 0, message)
      agentic_executor.AgenticToolCall(run_id, tool, args) ->
        LogEntry(
          run_id,
          "agentic",
          "agent",
          "info",
          "Tool: " <> tool <> " - " <> args,
        )
      agentic_executor.AgenticToolResult(run_id, tool, success, message) ->
        LogEntry(
          run_id,
          "agentic",
          "agent",
          case success {
            True -> "info"
            False -> "error"
          },
          tool <> ": " <> message,
        )
      agentic_executor.AgenticThinking(run_id, message) ->
        LogEntry(run_id, "agentic", "agent", "info", message)
      agentic_executor.AgenticComplete(run_id, success, message) ->
        ExecutionCompleted(run_id, success, message)
      agentic_executor.AgenticError(run_id, error) ->
        ExecutionFailed(run_id, error)
    }
    process.send(main_subject, converted)
  }

  let subject = process.new_subject()

  let _ = process.spawn(fn() { forward_agentic_messages(subject, handler) })

  subject
}

fn forward_agentic_messages(
  subject: Subject(agentic_executor.ProgressMessage),
  handler: fn(agentic_executor.ProgressMessage) -> Nil,
) -> Nil {
  case process.receive(subject, 60_000) {
    Ok(msg) -> {
      handler(msg)
      forward_agentic_messages(subject, handler)
    }
    Error(_) -> {
      forward_agentic_messages(subject, handler)
    }
  }
}

fn execute_phases(
  db: Connection,
  run_id: String,
  task: Task,
  project: Project,
  model: String,
  progress_subject: Subject(ProgressMessage),
) -> Result(ExecutionArtifacts, String) {
  let artifacts =
    ExecutionArtifacts(
      plan: None,
      tests: None,
      implementation: None,
      review: None,
      final_code: None,
    )

  use plan <- result.try(
    execute_phase(db, run_id, ArchitectPlanning, progress_subject, fn() {
      let system_prompt = prompts.architect_planning_prompt(task, project)
      let user_message = "Create an implementation plan for this task."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  let artifacts = ExecutionArtifacts(..artifacts, plan: Some(plan))

  use tests <- result.try(
    execute_phase(db, run_id, DeveloperTests, progress_subject, fn() {
      let system_prompt = prompts.developer_tests_prompt(task, project, plan)
      let user_message = "Write comprehensive tests based on the plan."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  let artifacts = ExecutionArtifacts(..artifacts, tests: Some(tests))

  use implementation <- result.try(
    execute_phase(db, run_id, DeveloperImplementation, progress_subject, fn() {
      let system_prompt =
        prompts.developer_implementation_prompt(task, project, plan, tests)
      let user_message = "Implement the feature to pass all tests."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  let artifacts =
    ExecutionArtifacts(..artifacts, implementation: Some(implementation))

  use review <- result.try(
    execute_phase(db, run_id, GrillerReview, progress_subject, fn() {
      let system_prompt =
        prompts.griller_review_prompt(task, project, plan, implementation)
      let user_message = "Review this implementation thoroughly."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  let artifacts = ExecutionArtifacts(..artifacts, review: Some(review))

  use fixed_impl <- result.try(
    execute_phase(db, run_id, DeveloperFixes, progress_subject, fn() {
      let system_prompt =
        prompts.developer_fixes_prompt(task, project, implementation, review)
      let user_message = "Address the code review feedback."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  use architect_feedback <- result.try(
    execute_phase(db, run_id, ArchitectReview, progress_subject, fn() {
      let system_prompt =
        prompts.architect_review_prompt(task, project, plan, fixed_impl)
      let user_message = "Perform final architectural review."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  use final_code <- result.try(
    execute_phase(db, run_id, DeveloperFinal, progress_subject, fn() {
      let system_prompt =
        prompts.developer_final_prompt(
          task,
          project,
          fixed_impl,
          architect_feedback,
        )
      let user_message = "Finalize the implementation."
      llm.complete(model, system_prompt, user_message)
    }),
  )

  let artifacts = ExecutionArtifacts(..artifacts, final_code: Some(final_code))

  Ok(artifacts)
}

fn execute_phase(
  db: Connection,
  run_id: String,
  phase: AgentPhase,
  progress_subject: Subject(ProgressMessage),
  agent_fn: fn() -> Result(String, llm.LlmError),
) -> Result(String, String) {
  let phase_str = prompts.phase_to_string(phase)
  let phase_display = prompts.phase_display_name(phase)
  let agent_type = prompts.phase_agent_type(phase)
  let progress_val = prompts.phase_progress(phase)

  process.send(
    progress_subject,
    PhaseStarted(
      run_id,
      phase_str,
      progress_val - 10,
      "Starting " <> phase_display,
    ),
  )

  let _ =
    tasks.add_run_log(
      db,
      run_id,
      phase_str,
      agent_type,
      task.LogInfo,
      "Starting phase: " <> phase_display,
    )

  let _ = tasks.update_run_progress(db, run_id, phase_str, progress_val - 10)

  case agent_fn() {
    Ok(agent_result) -> {
      let _ =
        tasks.add_run_log(
          db,
          run_id,
          phase_str,
          agent_type,
          task.LogInfo,
          "Completed phase: " <> phase_display,
        )

      let _ = tasks.update_run_progress(db, run_id, phase_str, progress_val)

      process.send(
        progress_subject,
        PhaseCompleted(
          run_id,
          phase_str,
          progress_val,
          "Completed " <> phase_display,
        ),
      )

      Ok(agent_result)
    }
    Error(err) -> {
      let error_msg = llm.error_to_string(err)

      let _ =
        tasks.add_run_log(
          db,
          run_id,
          phase_str,
          agent_type,
          task.LogError,
          "Phase failed: " <> error_msg,
        )

      process.send(
        progress_subject,
        LogEntry(run_id, phase_str, agent_type, "error", error_msg),
      )

      Error("Phase " <> phase_display <> " failed: " <> error_msg)
    }
  }
}
