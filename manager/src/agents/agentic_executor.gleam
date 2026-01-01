/// Agentic Task Executor
/// Implements a tool-calling agent loop for tasks that need to interact with code/KB
import agents/llm.{type LlmError, type Message, Assistant, Message, System, User}
import agents/prompts
import agents/tools
import agents/tools/types.{type ToolCall, type ToolContext, ToolCall, ToolContext}
import config
import database/connection.{type Connection}
import database/queries/sources
import database/queries/tasks
import gleam/dynamic/decode
import gleam/erlang/process.{type Subject}
import gleam/http
import gleam/http/request
import gleam/httpc
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/project.{type Project}
import models/source.{type Source}
import models/task.{type Task, LogInfo}

/// Maximum number of tool-calling iterations to prevent infinite loops
const max_iterations = 50

/// Progress message for WebSocket streaming
pub type ProgressMessage {
  AgenticPhaseStarted(run_id: String, phase: String, message: String)
  AgenticToolCall(run_id: String, tool: String, args: String)
  AgenticToolResult(
    run_id: String,
    tool: String,
    success: Bool,
    message: String,
  )
  AgenticThinking(run_id: String, message: String)
  AgenticComplete(run_id: String, success: Bool, message: String)
  AgenticError(run_id: String, error: String)
}

/// State maintained during agentic execution
pub type AgenticState {
  AgenticState(
    /// Conversation history for context
    messages: List(Message),
    /// Total iterations so far
    iterations: Int,
    /// Files that have been modified
    modified_files: List(String),
    /// Whether the task is complete
    is_complete: Bool,
    /// Final result message
    result_message: Option(String),
  )
}

/// Execute a task agentically with tool access
pub fn execute_agentic_task(
  db: Connection,
  run_id: String,
  task: Task,
  project: Project,
  progress_subject: Subject(ProgressMessage),
) -> Result(String, String) {
  // Get the source for this task from the database
  let source = case sources.get_task_source(db, task.id) {
    Ok(Some(s)) -> Some(s)
    Ok(None) -> None
    Error(_) -> None
  }

  // Create tool context
  let tool_ctx =
    ToolContext(db: db, project: project, task: task, source: source)

  // Get the model to use
  let model = case task.model_name {
    Some(m) -> m
    None -> "gpt-4o"
    // Default to GPT-4o for agentic tasks (better tool use)
  }

  // Build the system prompt for agentic execution
  let system_prompt = build_agentic_system_prompt(task, project)

  // Initial user message
  let user_message = build_initial_user_message(task)

  // Initialize state
  let initial_state =
    AgenticState(
      messages: [
        Message(System, system_prompt),
        Message(User, user_message),
      ],
      iterations: 0,
      modified_files: [],
      is_complete: False,
      result_message: None,
    )

  // Log start
  let _ =
    tasks.add_run_log(
      db,
      run_id,
      "agentic_execution",
      "agent",
      LogInfo,
      "Starting agentic task execution",
    )

  process.send(
    progress_subject,
    AgenticPhaseStarted(
      run_id,
      "agentic_execution",
      "Starting agentic task execution",
    ),
  )

  // Run the agent loop
  case
    run_agent_loop(db, run_id, model, tool_ctx, initial_state, progress_subject)
  {
    Ok(final_state) -> {
      let result_msg =
        option.unwrap(final_state.result_message, "Task completed")

      process.send(progress_subject, AgenticComplete(run_id, True, result_msg))

      // Log modified files if any
      case final_state.modified_files {
        [] -> Nil
        files -> {
          let _ =
            tasks.add_run_log(
              db,
              run_id,
              "agentic_execution",
              "agent",
              LogInfo,
              "Modified files: " <> string.join(files, ", "),
            )
          Nil
        }
      }

      Ok(result_msg)
    }
    Error(err) -> {
      process.send(progress_subject, AgenticError(run_id, err))
      Error(err)
    }
  }
}

/// Run the main agent loop
fn run_agent_loop(
  db: Connection,
  run_id: String,
  model: String,
  tool_ctx: ToolContext,
  state: AgenticState,
  progress_subject: Subject(ProgressMessage),
) -> Result(AgenticState, String) {
  // Check iteration limit
  case state.iterations >= max_iterations {
    True ->
      Error(
        "Maximum iterations reached (" <> int.to_string(max_iterations) <> ")",
      )
    False -> {
      // Check if already complete
      case state.is_complete {
        True -> Ok(state)
        False -> {
          // Call LLM with tools
          case call_llm_with_tools(model, state.messages) {
            Ok(response) -> {
              // Process the response
              process_llm_response(
                db,
                run_id,
                model,
                tool_ctx,
                state,
                response,
                progress_subject,
              )
            }
            Error(err) -> Error(llm.error_to_string(err))
          }
        }
      }
    }
  }
}

/// LLM response that may contain tool calls
pub type LlmResponseWithTools {
  LlmResponseWithTools(
    content: Option(String),
    tool_calls: List(ToolCall),
    finish_reason: String,
  )
}

/// Call LLM with tool definitions
fn call_llm_with_tools(
  model: String,
  messages: List(Message),
) -> Result(LlmResponseWithTools, LlmError) {
  let litellm_host = config.get_litellm_host()
  let litellm_key = config.get_litellm_key()
  let url = litellm_host <> "/v1/chat/completions"

  // Build request body with tools
  let tools_json = tools.tools_to_json(tools.get_available_tools())

  let messages_json =
    json.array(messages, fn(msg) {
      json.object([
        #("role", json.string(role_to_string(msg.role))),
        #("content", json.string(msg.content)),
      ])
    })

  let body =
    json.object([
      #("model", json.string(model)),
      #("messages", messages_json),
      #("tools", tools_json),
      #("tool_choice", json.string("auto")),
      #("temperature", json.float(0.1)),
      // Lower temp for more deterministic tool use
      #("max_tokens", json.int(4096)),
    ])
    |> json.to_string

  case request.to(url) {
    Ok(req) -> {
      let req =
        req
        |> request.set_method(http.Post)
        |> request.set_body(body)
        |> request.set_header("content-type", "application/json")
        |> request.set_header("authorization", "Bearer " <> litellm_key)

      case httpc.send(req) {
        Ok(resp) -> {
          case resp.status {
            200 -> parse_tool_response(resp.body)
            status -> Error(llm.ApiError(status, resp.body))
          }
        }
        Error(_) -> Error(llm.NetworkError("Failed to connect to LiteLLM"))
      }
    }
    Error(_) -> Error(llm.NetworkError("Invalid LiteLLM URL"))
  }
}

/// Parse LLM response that may contain tool calls
fn parse_tool_response(body: String) -> Result(LlmResponseWithTools, LlmError) {
  // Decoder for tool calls
  let tool_call_decoder = {
    use id <- decode.field("id", decode.string)
    use function <- decode.field("function", {
      use name <- decode.field("name", decode.string)
      use arguments <- decode.field("arguments", decode.string)
      decode.success(#(name, arguments))
    })
    decode.success(#(id, function))
  }

  let decoder = {
    use choices <- decode.field(
      "choices",
      decode.list({
        use message <- decode.field("message", {
          use content <- decode.optional_field(
            "content",
            None,
            decode.optional(decode.string),
          )
          use tool_calls <- decode.optional_field(
            "tool_calls",
            [],
            decode.list(tool_call_decoder),
          )
          decode.success(#(content, tool_calls))
        })
        use finish_reason <- decode.field("finish_reason", decode.string)
        decode.success(#(message, finish_reason))
      }),
    )
    decode.success(choices)
  }

  case json.parse(body, decoder) {
    Ok(choices) -> {
      case choices {
        [#(#(content_opt, tool_calls_raw), finish_reason), ..] -> {
          let content = case content_opt {
            Some(c) -> Some(c)
            None -> None
          }
          let tool_calls =
            list.map(tool_calls_raw, fn(tc) {
              let #(id, #(name, args)) = tc
              ToolCall(id, name, args)
            })
          Ok(LlmResponseWithTools(content, tool_calls, finish_reason))
        }
        [] -> Error(llm.ParseError("No choices in response"))
      }
    }
    Error(e) -> Error(llm.ParseError("Failed to parse: " <> string.inspect(e)))
  }
}

/// Process LLM response and continue loop if needed
fn process_llm_response(
  db: Connection,
  run_id: String,
  model: String,
  tool_ctx: ToolContext,
  state: AgenticState,
  response: LlmResponseWithTools,
  progress_subject: Subject(ProgressMessage),
) -> Result(AgenticState, String) {
  // Log thinking if there's content
  case response.content {
    Some(content) -> {
      process.send(progress_subject, AgenticThinking(run_id, content))
      let _ =
        tasks.add_run_log(
          db,
          run_id,
          "agentic_execution",
          "agent",
          LogInfo,
          "Thinking: " <> string.slice(content, 0, 200),
        )
      Nil
    }
    None -> Nil
  }

  // Check if we're done
  case response.finish_reason, response.tool_calls {
    "stop", [] -> {
      // No more tool calls and finish_reason is stop - we're done
      let result_msg =
        option.unwrap(response.content, "Task completed successfully")
      Ok(
        AgenticState(
          ..state,
          is_complete: True,
          result_message: Some(result_msg),
        ),
      )
    }

    _, tool_calls -> {
      // Execute tool calls
      let tool_results =
        list.map(tool_calls, fn(call) {
          process.send(
            progress_subject,
            AgenticToolCall(
              run_id,
              call.name,
              string.slice(call.arguments, 0, 100),
            ),
          )

          let _ =
            tasks.add_run_log(
              db,
              run_id,
              "agentic_execution",
              "agent",
              LogInfo,
              "Tool call: " <> call.name,
            )

          let result = tools.execute_tool(tool_ctx, call)

          process.send(
            progress_subject,
            AgenticToolResult(
              run_id,
              call.name,
              !result.is_error,
              string.slice(result.content, 0, 100),
            ),
          )

          result
        })

      // Track modified files
      let new_modified_files =
        list.fold(tool_calls, state.modified_files, fn(acc, call) {
          case call.name {
            "write_file" -> {
              // Try to extract path from arguments
              case get_path_from_args(call.arguments) {
                Ok(path) -> [path, ..acc]
                Error(_) -> acc
              }
            }
            _ -> acc
          }
        })

      // Build assistant message with tool calls
      let assistant_content = option.unwrap(response.content, "")
      let assistant_msg = Message(Assistant, assistant_content)

      // Build tool result messages
      let tool_result_messages =
        list.map(tool_results, fn(result) {
          // Tool results are sent as user messages in the conversation
          Message(
            User,
            "Tool result for " <> result.tool_call_id <> ":\n" <> result.content,
          )
        })

      // Update state with new messages
      let new_messages =
        list.flatten([
          state.messages,
          [assistant_msg],
          tool_result_messages,
        ])

      let new_state =
        AgenticState(
          messages: new_messages,
          iterations: state.iterations + 1,
          modified_files: new_modified_files,
          is_complete: False,
          result_message: None,
        )

      // Continue the loop
      run_agent_loop(db, run_id, model, tool_ctx, new_state, progress_subject)
    }
  }
}

/// Build the system prompt for agentic task execution
fn build_agentic_system_prompt(task: Task, project: Project) -> String {
  let description = case project.description {
    Some(d) -> d
    None -> "No description provided"
  }

  let acceptance = case task.acceptance_criteria {
    Some(ac) -> ac
    None -> "Complete the task as described"
  }

  string.join(
    [
      "You are an autonomous software development agent working on a coding task.",
      "You have access to tools that let you read and write code, search the codebase, and query the knowledge base.",
      "",
      "PROJECT: " <> project.name,
      description,
      "",
      "TASK: " <> task.title,
      task.description,
      "",
      "ACCEPTANCE CRITERIA:",
      acceptance,
      "",
      "## Instructions",
      "",
      "1. **Understand the codebase**: Use `list_files` and `read_file` to explore the project structure and understand existing patterns.",
      "",
      "2. **Plan your approach**: Before writing code, think about what files need to be created or modified.",
      "",
      "3. **Write code incrementally**: Make changes in small, testable chunks. Use `write_file` to save your changes.",
      "",
      "4. **Search for context**: Use `search_code` to find related code patterns and `query_knowledge_base` for project-specific knowledge.",
      "",
      "5. **Test your changes**: If possible, use `run_command` to run tests or build commands to verify your changes work.",
      "",
      "6. **Complete the task**: Once you've made all necessary changes and verified they work, summarize what you did.",
      "",
      "## Guidelines",
      "",
      "- Follow existing code style and patterns in the project",
      "- Write clean, maintainable code with appropriate comments",
      "- Handle errors appropriately",
      "- Don't make unnecessary changes outside the scope of the task",
      "- If you're unsure about something, explain your reasoning",
      "",
      "When you have completed the task, provide a summary of what you did.",
    ],
    "\n",
  )
}

/// Build the initial user message
fn build_initial_user_message(task: Task) -> String {
  string.join(
    [
      "Please complete the following task:",
      "",
      "**" <> task.title <> "**",
      "",
      task.description,
      "",
      "Start by exploring the codebase to understand its structure, then implement the required changes.",
    ],
    "\n",
  )
}

/// Helper to convert role to string
fn role_to_string(role: llm.Role) -> String {
  case role {
    llm.System -> "system"
    llm.User -> "user"
    llm.Assistant -> "assistant"
  }
}

/// Extract path from write_file arguments
fn get_path_from_args(args_json: String) -> Result(String, String) {
  let decoder = {
    use path <- decode.field("path", decode.string)
    decode.success(path)
  }
  case json.parse(args_json, decoder) {
    Ok(path) -> Ok(path)
    Error(_) -> Error("Could not extract path")
  }
}
