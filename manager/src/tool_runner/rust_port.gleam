/// Rust Port Tool Runner
/// Implementation that communicates with the zone-runner binary via Erlang ports
import gleam/dict.{type Dict}
import gleam/dynamic/decode
import gleam/erlang/process.{type Subject}
import gleam/json
import gleam/list
import gleam/option.{type Option}
import gleam/otp/actor
import gleam/result
import gleam/string
import tool_runner.{
  type RunProgress, type RunResult, type RunnerConfig, type RunnerResponse,
}
import youid/uuid

/// External port type (opaque Erlang port reference)
pub type Port

/// State of the runner process
pub type RunnerState {
  RunnerState(
    port: Port,
    config: RunnerConfig,
    /// Map of job_id to progress subject
    subscribers: Dict(String, Subject(RunProgress)),
    /// Whether handshake is complete
    connected: Bool,
  )
}

/// Messages to the runner actor
pub type RunnerMessage {
  /// Execute a command
  Execute(
    workspace: String,
    command: String,
    args: List(String),
    env: List(#(String, String)),
    timeout_ms: Option(Int),
    reply_to: Subject(Result(tool_runner.RunResult, String)),
    progress_subject: Subject(RunProgress),
  )
  /// Cancel a job
  Cancel(job_id: String, force: Bool)
  /// Port data received
  PortData(data: String)
  /// Port exited
  PortExit(status: Int)
  /// Shutdown the runner
  Shutdown
}

// =============================================================================
// FFI Functions
// =============================================================================

@external(erlang, "tool_runner_port_ffi", "open_port")
fn ffi_open_port(
  binary_path: String,
  args: List(String),
) -> Result(Port, String)

@external(erlang, "tool_runner_port_ffi", "send_to_port")
fn ffi_send_to_port(port: Port, data: String) -> Result(Nil, String)

@external(erlang, "tool_runner_port_ffi", "receive_from_port")
fn ffi_receive_from_port(port: Port, timeout_ms: Int) -> Result(String, Nil)

@external(erlang, "tool_runner_port_ffi", "close_port")
fn ffi_close_port(port: Port) -> Result(Nil, String)

// =============================================================================
// Public API
// =============================================================================

/// Start the runner process
pub fn start(config: RunnerConfig) -> Result(Subject(RunnerMessage), String) {
  // Open port to runner binary
  case ffi_open_port(config.binary_path, ["serve", "--stdio"]) {
    Ok(port) -> {
      // Perform handshake
      let hello = encode_hello()
      case ffi_send_to_port(port, hello <> "\n") {
        Ok(_) -> {
          // Wait for HelloAck
          case ffi_receive_from_port(port, 5000) {
            Ok(response) -> {
              case parse_hello_ack(response) {
                Ok(_ack) -> {
                  // Start actor to manage the port
                  let state =
                    RunnerState(
                      port: port,
                      config: config,
                      subscribers: dict.new(),
                      connected: True,
                    )
                  case start_actor(state) {
                    Ok(subject) -> Ok(subject)
                    Error(_) -> {
                      let _ = ffi_close_port(port)
                      Error("Failed to start runner actor")
                    }
                  }
                }
                Error(e) -> {
                  let _ = ffi_close_port(port)
                  Error("HelloAck parse failed: " <> e)
                }
              }
            }
            Error(_) -> {
              let _ = ffi_close_port(port)
              Error("Timeout waiting for HelloAck")
            }
          }
        }
        Error(e) -> {
          let _ = ffi_close_port(port)
          Error("Failed to send Hello: " <> e)
        }
      }
    }
    Error(e) -> Error("Failed to start runner: " <> e)
  }
}

fn start_actor(
  state: RunnerState,
) -> Result(Subject(RunnerMessage), actor.StartError) {
  actor.new(state)
  |> actor.on_message(handle_message)
  |> actor.start
  |> result.map(fn(started) { started.data })
}

/// Run a command through the runner
pub fn run_command(
  runner: Subject(RunnerMessage),
  workspace: String,
  command: String,
  args: List(String),
  env: List(#(String, String)),
  timeout_ms: Option(Int),
  progress_subject: Subject(RunProgress),
) -> Result(tool_runner.RunResult, String) {
  // Create reply subject for this request
  let reply_subject = process.new_subject()

  // Send execute message
  process.send(
    runner,
    Execute(
      workspace: workspace,
      command: command,
      args: args,
      env: env,
      timeout_ms: timeout_ms,
      reply_to: reply_subject,
      progress_subject: progress_subject,
    ),
  )

  // Wait for result with timeout
  let effective_timeout = option.unwrap(timeout_ms, 300_000) + 5000
  case process.receive(reply_subject, effective_timeout) {
    Ok(result) -> result
    Error(_) -> Error("Timeout waiting for command start")
  }
}

/// Cancel a running job
pub fn cancel(runner: Subject(RunnerMessage), job_id: String, force: Bool) {
  process.send(runner, Cancel(job_id, force))
}

/// Shutdown the runner
pub fn shutdown(runner: Subject(RunnerMessage)) {
  process.send(runner, Shutdown)
}

// =============================================================================
// Actor Message Handler
// =============================================================================

fn handle_message(
  state: RunnerState,
  message: RunnerMessage,
) -> actor.Next(RunnerState, RunnerMessage) {
  case message {
    Execute(workspace, command, args, env, timeout_ms, reply_to, progress) -> {
      let job_id = uuid.v4_string()

      // Register subscriber
      let new_subscribers = dict.insert(state.subscribers, job_id, progress)

      // Build and send RunStart message
      let msg =
        encode_run_start(
          job_id,
          workspace,
          command,
          args,
          env,
          timeout_ms,
          option.Some(state.config.max_output_bytes),
        )

      case ffi_send_to_port(state.port, msg <> "\n") {
        Ok(_) -> {
          // Wait for RunStarted response
          case ffi_receive_from_port(state.port, 10_000) {
            Ok(response) -> {
              case parse_response(response) {
                Ok(tool_runner.RunStarted(id, pid)) if id == job_id -> {
                  process.send(
                    reply_to,
                    Ok(tool_runner.RunResult(job_id, pid)),
                  )

                  // Start background task to collect output
                  let port = state.port
                  let subs = new_subscribers
                  let _ = process.spawn(fn() { collect_output(port, subs) })

                  actor.continue(
                    RunnerState(..state, subscribers: new_subscribers),
                  )
                }
                Ok(tool_runner.RunError(id, code, message)) if id == job_id -> {
                  process.send(reply_to, Error(code <> ": " <> message))
                  actor.continue(state)
                }
                Ok(_) -> {
                  process.send(reply_to, Error("Unexpected response"))
                  actor.continue(state)
                }
                Error(e) -> {
                  process.send(reply_to, Error("Parse error: " <> e))
                  actor.continue(state)
                }
              }
            }
            Error(_) -> {
              process.send(reply_to, Error("Timeout waiting for RunStarted"))
              actor.continue(state)
            }
          }
        }
        Error(e) -> {
          process.send(reply_to, Error("Failed to send RunStart: " <> e))
          actor.continue(state)
        }
      }
    }

    Cancel(job_id, force) -> {
      let msg = encode_cancel(job_id, force)
      let _ = ffi_send_to_port(state.port, msg <> "\n")
      actor.continue(state)
    }

    PortData(data) -> {
      // Parse and dispatch to subscriber
      case parse_response(data) {
        Ok(response) -> {
          let progress = response_to_progress(response)
          case progress {
            option.Some(#(job_id, msg)) -> {
              case dict.get(state.subscribers, job_id) {
                Ok(subject) -> process.send(subject, msg)
                Error(_) -> Nil
              }
            }
            option.None -> Nil
          }
        }
        Error(_) -> Nil
      }
      actor.continue(state)
    }

    PortExit(status) -> {
      // Notify all subscribers of error
      dict.each(state.subscribers, fn(job_id, subject) {
        process.send(
          subject,
          tool_runner.Error(
            job_id,
            "runner_exit",
            "Runner process exited with status " <> string.inspect(status),
          ),
        )
      })
      actor.stop()
    }

    Shutdown -> {
      let _ = ffi_close_port(state.port)
      actor.stop()
    }
  }
}

// =============================================================================
// Background Output Collection
// =============================================================================

fn collect_output(port: Port, subscribers: Dict(String, Subject(RunProgress))) {
  // Continuously read from port and dispatch to subscribers
  case ffi_receive_from_port(port, 60_000) {
    Ok(line) -> {
      case parse_response(line) {
        Ok(response) -> {
          case response_to_progress(response) {
            option.Some(#(job_id, progress)) -> {
              case dict.get(subscribers, job_id) {
                Ok(subject) -> {
                  process.send(subject, progress)
                  // Continue unless terminal
                  case progress {
                    tool_runner.Exit(_, _, _, _)
                    | tool_runner.Error(_, _, _) -> Nil
                    _ -> collect_output(port, subscribers)
                  }
                }
                Error(_) -> collect_output(port, subscribers)
              }
            }
            option.None -> collect_output(port, subscribers)
          }
        }
        Error(_) -> collect_output(port, subscribers)
      }
    }
    Error(_) -> Nil
    // Port closed or timeout
  }
}

// =============================================================================
// Protocol Encoding
// =============================================================================

fn encode_hello() -> String {
  json.to_string(
    json.object([
      #("type", json.string("Hello")),
      #("protocol_version", json.string("1.0")),
      #("capabilities", json.array(["cancel", "stdin", "logs"], json.string)),
    ]),
  )
}

fn encode_run_start(
  job_id: String,
  workspace: String,
  command: String,
  args: List(String),
  env: List(#(String, String)),
  timeout_ms: Option(Int),
  max_output_bytes: Option(Int),
) -> String {
  let env_object =
    json.object(list.map(env, fn(kv) {
      let #(k, v) = kv
      #(k, json.string(v))
    }))

  json.to_string(
    json.object([
      #("type", json.string("RunStart")),
      #("job_id", json.string(job_id)),
      #("workspace", json.string(workspace)),
      #("command", json.string(command)),
      #("args", json.array(args, json.string)),
      #("env", env_object),
      #(
        "timeout_ms",
        case timeout_ms {
          option.Some(t) -> json.int(t)
          option.None -> json.null()
        },
      ),
      #(
        "max_output_bytes",
        case max_output_bytes {
          option.Some(m) -> json.int(m)
          option.None -> json.null()
        },
      ),
    ]),
  )
}

fn encode_cancel(job_id: String, force: Bool) -> String {
  json.to_string(
    json.object([
      #("type", json.string("RunCancel")),
      #("job_id", json.string(job_id)),
      #("force", json.bool(force)),
    ]),
  )
}

// =============================================================================
// Protocol Parsing
// =============================================================================

fn parse_hello_ack(
  line: String,
) -> Result(
  #(String, String, List(String)),
  // protocol_version, runner_version, capabilities
  String,
) {
  let decoder = {
    use msg_type <- decode.field("type", decode.string)
    use protocol_version <- decode.field("protocol_version", decode.string)
    use runner_version <- decode.field("runner_version", decode.string)
    use capabilities <- decode.field("capabilities", decode.list(decode.string))

    case msg_type {
      "HelloAck" ->
        decode.success(#(protocol_version, runner_version, capabilities))
      _ -> decode.failure(#("", "", []), "Expected HelloAck")
    }
  }

  case json.parse(line, decoder) {
    Ok(res) -> Ok(res)
    Error(_) -> Error("Failed to parse HelloAck")
  }
}

fn parse_response(line: String) -> Result(RunnerResponse, String) {
  let type_decoder = {
    use msg_type <- decode.field("type", decode.string)
    decode.success(msg_type)
  }

  case json.parse(line, type_decoder) {
    Ok(msg_type) -> parse_response_by_type(line, msg_type)
    Error(_) -> Error("Failed to parse message type")
  }
}

fn parse_response_by_type(
  line: String,
  msg_type: String,
) -> Result(RunnerResponse, String) {
  case msg_type {
    "RunStarted" -> {
      let decoder = {
        use job_id <- decode.field("job_id", decode.string)
        use pid <- decode.field("pid", decode.int)
        decode.success(tool_runner.RunStarted(job_id, pid))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse RunStarted" })
    }

    "RunStdout" -> {
      let decoder = {
        use job_id <- decode.field("job_id", decode.string)
        use data <- decode.field("data", decode.string)
        use sequence <- decode.field("sequence", decode.int)
        decode.success(tool_runner.RunStdout(job_id, data, sequence))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse RunStdout" })
    }

    "RunStderr" -> {
      let decoder = {
        use job_id <- decode.field("job_id", decode.string)
        use data <- decode.field("data", decode.string)
        use sequence <- decode.field("sequence", decode.int)
        decode.success(tool_runner.RunStderr(job_id, data, sequence))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse RunStderr" })
    }

    "RunLog" -> {
      let decoder = {
        use job_id <- decode.field("job_id", decode.string)
        use level <- decode.field("level", decode.string)
        use message <- decode.field("message", decode.string)
        decode.success(tool_runner.RunLog(job_id, level, message))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse RunLog" })
    }

    "RunExit" -> {
      let decoder = {
        use job_id <- decode.field("job_id", decode.string)
        use exit_code <- decode.optional_field(
          "exit_code",
          option.None,
          decode.optional(decode.int),
        )
        use signal <- decode.optional_field(
          "signal",
          option.None,
          decode.optional(decode.int),
        )
        use duration_ms <- decode.field("duration_ms", decode.int)
        decode.success(tool_runner.RunExit(
          job_id,
          exit_code,
          signal,
          duration_ms,
        ))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse RunExit" })
    }

    "RunError" -> {
      let decoder = {
        use job_id <- decode.field("job_id", decode.string)
        use error_code <- decode.field("error_code", decode.string)
        use message <- decode.field("message", decode.string)
        decode.success(tool_runner.RunError(job_id, error_code, message))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse RunError" })
    }

    "Pong" -> {
      let decoder = {
        use id <- decode.field("id", decode.string)
        decode.success(tool_runner.Pong(id))
      }
      json.parse(line, decoder)
      |> result.map_error(fn(_) { "Failed to parse Pong" })
    }

    _ -> Error("Unknown message type: " <> msg_type)
  }
}

fn response_to_progress(
  response: RunnerResponse,
) -> Option(#(String, RunProgress)) {
  case response {
    tool_runner.RunStdout(job_id, data, seq) -> {
      case decode_base64(data) {
        Ok(bytes) ->
          option.Some(#(job_id, tool_runner.Stdout(job_id, bytes, seq)))
        Error(_) -> option.None
      }
    }

    tool_runner.RunStderr(job_id, data, seq) -> {
      case decode_base64(data) {
        Ok(bytes) ->
          option.Some(#(job_id, tool_runner.Stderr(job_id, bytes, seq)))
        Error(_) -> option.None
      }
    }

    tool_runner.RunLog(job_id, level, message) ->
      option.Some(#(job_id, tool_runner.Log(job_id, level, message)))

    tool_runner.RunExit(job_id, exit_code, signal, duration_ms) ->
      option.Some(#(
        job_id,
        tool_runner.Exit(job_id, exit_code, signal, duration_ms),
      ))

    tool_runner.RunError(job_id, error_code, message) ->
      option.Some(#(job_id, tool_runner.Error(job_id, error_code, message)))

    _ -> option.None
  }
}

// =============================================================================
// Base64 Decoding
// =============================================================================

@external(erlang, "base64", "decode")
fn erlang_base64_decode(data: String) -> BitArray

fn decode_base64(data: String) -> Result(BitArray, String) {
  // Use Erlang's base64 module
  Ok(erlang_base64_decode(data))
}
