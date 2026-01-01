/// Task Worker Progress
/// Progress message serialization for WebSocket streaming
import agents/task_worker/types.{
  type ProgressMessage, ExecutionCompleted, ExecutionFailed, LogEntry,
  PhaseCompleted, PhaseStarted,
}
import gleam/json

/// Convert progress message to JSON for WebSocket
pub fn to_json(msg: ProgressMessage) -> String {
  case msg {
    PhaseStarted(run_id, phase, progress, message) ->
      json.object([
        #("type", json.string("phase_started")),
        #("run_id", json.string(run_id)),
        #("phase", json.string(phase)),
        #("progress_percent", json.int(progress)),
        #("message", json.string(message)),
      ])
      |> json.to_string

    PhaseCompleted(run_id, phase, progress, message) ->
      json.object([
        #("type", json.string("phase_completed")),
        #("run_id", json.string(run_id)),
        #("phase", json.string(phase)),
        #("progress_percent", json.int(progress)),
        #("message", json.string(message)),
      ])
      |> json.to_string

    LogEntry(run_id, phase, agent, level, message) ->
      json.object([
        #("type", json.string("log")),
        #("run_id", json.string(run_id)),
        #("phase", json.string(phase)),
        #("agent_type", json.string(agent)),
        #("log_level", json.string(level)),
        #("message", json.string(message)),
      ])
      |> json.to_string

    ExecutionCompleted(run_id, success, message) ->
      json.object([
        #("type", json.string("complete")),
        #("run_id", json.string(run_id)),
        #("success", json.bool(success)),
        #("message", json.string(message)),
      ])
      |> json.to_string

    ExecutionFailed(run_id, error) ->
      json.object([
        #("type", json.string("error")),
        #("run_id", json.string(run_id)),
        #("error", json.string(error)),
      ])
      |> json.to_string
  }
}
