/// Agent Tool Executor
/// Executes tool calls from the LLM
import agents/content_source
import agents/file_source
import agents/tools/types.{
  type ToolCall, type ToolContext, type ToolResult, ToolResult,
}
import database/queries/projects
import database/queries/sources as sources_db
import database/queries/tasks
import gleam/dynamic/decode
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/content
import models/project
import models/source
import models/task

/// Execute a tool call
pub fn execute_tool(ctx: ToolContext, call: ToolCall) -> ToolResult {
  let result = case call.name {
    // File tools (legacy)
    "read_file" -> execute_read_file(ctx, call.arguments)
    "write_file" -> execute_write_file(ctx, call.arguments)
    "list_files" -> execute_list_files(ctx, call.arguments)
    "search_code" -> execute_search_code(ctx, call.arguments)
    // Content tools (unified)
    "list_content" -> execute_list_content(ctx, call.arguments)
    "get_content" -> execute_get_content(ctx, call.arguments)
    "search_content" -> execute_search_content(ctx, call.arguments)
    // Other tools
    "query_knowledge_base" -> execute_query_kb(ctx, call.arguments)
    "list_projects" -> execute_list_projects(ctx, call.arguments)
    "get_task_history" -> execute_get_task_history(ctx, call.arguments)
    "run_command" -> execute_run_command(ctx, call.arguments)
    _ -> Error("Unknown tool: " <> call.name)
  }

  case result {
    Ok(output) -> ToolResult(call.id, output, False)
    Error(err) -> ToolResult(call.id, "Error: " <> err, True)
  }
}

// =============================================================================
// Tool Implementations
// =============================================================================

fn execute_read_file(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use path <- result.try(get_string_arg(args, "path"))

  case ctx.source {
    Some(source) -> {
      case file_source.read_file(source, path) {
        Ok(content) -> Ok(content.content)
        Error(err) -> Error(file_source.error_to_string(err))
      }
    }
    None -> Error("No source configured for this task")
  }
}

fn execute_write_file(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use path <- result.try(get_string_arg(args, "path"))
  use content <- result.try(get_string_arg(args, "content"))

  case ctx.source {
    Some(source) -> {
      let message = "Update " <> path <> " via Zone Agent"
      case file_source.write_file(source, path, content, message) {
        Ok(res) -> Ok(res.message)
        Error(err) -> Error(file_source.error_to_string(err))
      }
    }
    None -> Error("No source configured for this task")
  }
}

fn execute_list_files(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  let path = get_string_arg(args, "path") |> result.unwrap("")
  let _recursive = get_bool_arg(args, "recursive") |> result.unwrap(False)

  case ctx.source {
    Some(source) -> {
      case file_source.list_files(source, path) {
        Ok(entries) -> {
          let res =
            list.map(entries, fn(entry) {
              json.object([
                #("name", json.string(entry.name)),
                #("path", json.string(entry.path)),
                #(
                  "type",
                  json.string(case entry.is_directory {
                    True -> "dir"
                    False -> "file"
                  }),
                ),
              ])
            })
          Ok(json.to_string(json.array(res, fn(x) { x })))
        }
        Error(err) -> Error(file_source.error_to_string(err))
      }
    }
    None -> Error("No source configured for this task")
  }
}

fn execute_search_code(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use pattern <- result.try(get_string_arg(args, "pattern"))
  let file_pattern = get_string_arg(args, "file_pattern") |> result.unwrap("")
  let _max_results = get_int_arg(args, "max_results") |> result.unwrap(50)

  case ctx.source {
    Some(src) -> {
      let path_filter = case file_pattern {
        "" -> None
        p -> Some(p)
      }
      case file_source.search_files(src, pattern, path_filter) {
        Ok(matches) -> {
          let res =
            list.map(matches, fn(match) {
              let #(path, name) = match
              json.object([
                #("name", json.string(name)),
                #("path", json.string(path)),
              ])
            })
          Ok(json.to_string(json.array(res, fn(x) { x })))
        }
        Error(err) -> Error(file_source.error_to_string(err))
      }
    }
    None -> Error("No source configured for this task")
  }
}

// =============================================================================
// Content Source Tools
// =============================================================================

fn execute_list_content(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  let category_str =
    get_string_arg(args, "source_category") |> option.from_result
  let path = get_string_arg(args, "path") |> option.from_result
  let start_date = get_string_arg(args, "start_date") |> option.from_result
  let end_date = get_string_arg(args, "end_date") |> option.from_result
  let limit = get_int_arg(args, "limit") |> result.unwrap(50)
  let offset = get_int_arg(args, "offset") |> result.unwrap(0)

  let query =
    content.ListQuery(
      path: path,
      start_date: start_date,
      end_date: end_date,
      channel: None,
      folder: path,
      limit: limit,
      offset: offset,
    )

  // Get sources from task
  case sources_db.get_task_sources(ctx.db, ctx.task.id) {
    Ok(sources) -> {
      // Filter by category if specified
      let filtered_sources = case category_str {
        Some(cat_str) -> {
          case source.source_category_from_string(cat_str) {
            Ok(category) ->
              list.filter(sources, fn(s) {
                source.source_type_category(s.source_type) == category
              })
            Error(_) -> sources
          }
        }
        None -> sources
      }

      // Collect content from all sources
      let results =
        list.flat_map(filtered_sources, fn(src) {
          case content_source.list_content(src, query) {
            Ok(list_result) -> list_result.items
            Error(_) -> []
          }
        })

      let limited = list.take(results, limit)
      Ok(json.to_string(json.array(limited, content.content_item_to_json)))
    }
    Error(err) -> Error("Failed to get sources: " <> err)
  }
}

fn execute_get_content(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use item_id <- result.try(get_string_arg(args, "item_id"))
  let source_id = get_string_arg(args, "source_id") |> option.from_result

  // Get specific source or all task sources
  let sources_result = case source_id {
    Some(id) -> {
      case sources_db.get_source(ctx.db, id) {
        Ok(Some(src)) -> Ok([src])
        Ok(None) -> Error("Source not found: " <> id)
        Error(err) -> Error(err)
      }
    }
    None -> sources_db.get_task_sources(ctx.db, ctx.task.id)
  }

  case sources_result {
    Ok(sources) -> {
      // Try to get content from each source until we find it
      let found =
        list.find_map(sources, fn(src) {
          case content_source.get_content(src, item_id) {
            Ok(item) -> Ok(item)
            Error(_) -> Error(Nil)
          }
        })

      case found {
        Ok(item) -> Ok(json.to_string(content.content_item_to_json(item)))
        Error(_) -> Error("Content not found: " <> item_id)
      }
    }
    Error(err) -> Error("Failed to get sources: " <> err)
  }
}

fn execute_search_content(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use query_text <- result.try(get_string_arg(args, "query"))
  let category_str =
    get_string_arg(args, "source_category") |> option.from_result
  let start_date = get_string_arg(args, "start_date") |> option.from_result
  let end_date = get_string_arg(args, "end_date") |> option.from_result
  let limit = get_int_arg(args, "limit") |> result.unwrap(50)

  let query =
    content.SearchQuery(
      query: query_text,
      path: None,
      start_date: start_date,
      end_date: end_date,
      limit: limit,
    )

  case sources_db.get_task_sources(ctx.db, ctx.task.id) {
    Ok(sources) -> {
      // Filter by category if specified
      let filtered_sources = case category_str {
        Some(cat_str) -> {
          case source.source_category_from_string(cat_str) {
            Ok(category) ->
              list.filter(sources, fn(s) {
                source.source_type_category(s.source_type) == category
              })
            Error(_) -> sources
          }
        }
        None -> sources
      }

      // Search across all sources
      let results =
        list.flat_map(filtered_sources, fn(src) {
          case content_source.search_content(src, query) {
            Ok(items) -> items
            Error(_) -> []
          }
        })

      let limited = list.take(results, limit)
      Ok(json.to_string(json.array(limited, content.content_item_to_json)))
    }
    Error(err) -> Error("Failed to get sources: " <> err)
  }
}

fn execute_query_kb(
  _ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use _query <- result.try(get_string_arg(args, "query"))
  let _limit = get_int_arg(args, "limit") |> result.unwrap(5)

  Error("Knowledge base search is not yet implemented")
}

fn execute_list_projects(
  ctx: ToolContext,
  _args_json: String,
) -> Result(String, String) {
  case projects.list_projects(ctx.db, None) {
    Ok(projs) -> {
      let results =
        list.map(projs, fn(p) {
          json.object([
            #("id", json.string(p.id)),
            #("name", json.string(p.name)),
            #("description", case p.description {
              Some(d) -> json.string(d)
              None -> json.null()
            }),
            #("status", json.string(project.status_to_string(p.status))),
            #("github_repo_url", case p.github_repo_url {
              Some(url) -> json.string(url)
              None -> json.null()
            }),
          ])
        })
      Ok(json.to_string(json.array(results, fn(x) { x })))
    }
    Error(err) -> Error("Failed to list projects: " <> err)
  }
}

fn execute_get_task_history(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  let project_id =
    get_string_arg(args, "project_id") |> result.unwrap(ctx.project.id)
  let status_filter = get_string_arg(args, "status") |> option.from_result
  let limit = get_int_arg(args, "limit") |> result.unwrap(20)

  let status = case status_filter {
    Some(s) -> task.status_from_string(s) |> option.from_result
    None -> None
  }

  case tasks.list_tasks(ctx.db, Some(project_id), status) {
    Ok(task_list) -> {
      let limited = list.take(task_list, limit)
      let results =
        list.map(limited, fn(t) {
          json.object([
            #("id", json.string(t.id)),
            #("title", json.string(t.title)),
            #("description", json.string(string.slice(t.description, 0, 200))),
            #("status", json.string(task.status_to_string(t.status))),
            #("priority", json.int(t.priority)),
            #("is_agentic", json.bool(t.is_agentic)),
          ])
        })
      Ok(json.to_string(json.array(results, fn(x) { x })))
    }
    Error(err) -> Error("Failed to get task history: " <> err)
  }
}

fn execute_run_command(
  ctx: ToolContext,
  args_json: String,
) -> Result(String, String) {
  use args <- result.try(parse_args(args_json))
  use command <- result.try(get_string_arg(args, "command"))
  let timeout_seconds = get_int_arg(args, "timeout_seconds") |> result.unwrap(60)

  // For security, we'll only allow certain safe commands
  let allowed_prefixes = [
    "npm ", "yarn ", "pnpm ", "gleam ", "cargo ", "go ", "python ", "pip ",
    "make ", "cmake ", "git status", "git diff", "git log", "ls ", "cat ",
    "head ", "tail ", "grep ", "find ", "tree ", "wc ", "du ",
  ]

  let is_allowed =
    list.any(allowed_prefixes, fn(prefix) {
      string.starts_with(command, prefix)
    })

  case is_allowed {
    True -> {
      // Get workspace path from source
      case get_workspace_path(ctx.source) {
        Ok(workspace_path) -> {
          // Parse command into executable and args
          let #(executable, cmd_args) = parse_command_line(command)
          execute_command_with_runner(
            workspace_path,
            executable,
            cmd_args,
            timeout_seconds,
          )
        }
        Error(e) -> Error(e)
      }
    }
    False ->
      Error(
        "Command not allowed for security reasons. Allowed commands: npm, yarn, gleam, cargo, go, python, make, git (read-only), ls, cat, grep, find, tree",
      )
  }
}

/// Get workspace path from source configuration
fn get_workspace_path(src: Option(source.Source)) -> Result(String, String) {
  case src {
    Some(s) -> {
      case s.config {
        source.FilesystemSourceConfig(cfg) -> Ok(cfg.base_path)
        // For GitHub/GitLab, we'd need to clone to a local path first
        // For now, return error if not filesystem
        _ ->
          Error(
            "Command execution requires a filesystem source. Clone the repository first.",
          )
      }
    }
    None -> Error("No source configured for this task")
  }
}

/// Parse a command string into executable and arguments
fn parse_command_line(command: String) -> #(String, List(String)) {
  case string.split(command, " ") {
    [executable, ..args] -> #(executable, args)
    _ -> #(command, [])
  }
}

/// Execute command using the Rust tool runner
fn execute_command_with_runner(
  workspace: String,
  executable: String,
  args: List(String),
  timeout_seconds: Int,
) -> Result(String, String) {
  // Import tool_runner modules
  // Note: This requires the runner to be started and available
  // In production, this would use a shared runner instance

  // For now, we provide a synchronous implementation that spawns the runner
  // per-command. In the future, this should use a long-lived runner process.

  // TODO: Implement full integration when runner is available in runtime
  // For now, return a placeholder message with command info
  let timeout_ms = timeout_seconds * 1000
  Ok(
    json.to_string(
      json.object([
        #("status", json.string("pending")),
        #(
          "message",
          json.string(
            "Command execution via Rust runner is configured. Waiting for runner binary.",
          ),
        ),
        #("executable", json.string(executable)),
        #("args", json.array(args, json.string)),
        #("workspace", json.string(workspace)),
        #("timeout_ms", json.int(timeout_ms)),
      ]),
    ),
  )
}

// =============================================================================
// Argument Parsing Helpers
// =============================================================================

fn parse_args(args_json: String) -> Result(String, String) {
  // Just validate it's valid JSON and return the string for later parsing
  case json.parse(args_json, decode.dynamic) {
    Ok(_) -> Ok(args_json)
    Error(_) -> Error("Invalid JSON arguments")
  }
}

fn get_string_arg(args: String, key: String) -> Result(String, String) {
  let decoder = {
    use value <- decode.field(key, decode.string)
    decode.success(value)
  }
  case json.parse(args, decoder) {
    Ok(value) -> Ok(value)
    Error(_) -> Error("Missing or invalid argument: " <> key)
  }
}

fn get_int_arg(args: String, key: String) -> Result(Int, String) {
  let decoder = {
    use value <- decode.field(key, decode.int)
    decode.success(value)
  }
  case json.parse(args, decoder) {
    Ok(value) -> Ok(value)
    Error(_) -> Error("Missing or invalid argument: " <> key)
  }
}

fn get_bool_arg(args: String, key: String) -> Result(Bool, String) {
  let decoder = {
    use value <- decode.field(key, decode.bool)
    decode.success(value)
  }
  case json.parse(args, decoder) {
    Ok(value) -> Ok(value)
    Error(_) -> Error("Missing or invalid argument: " <> key)
  }
}
