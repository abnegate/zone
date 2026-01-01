/// API routes for Tasks
/// Handles CRUD operations and task execution
import agents/executor
import cache/queries/tasks as cached_tasks
import gleam/dynamic/decode
import gleam/erlang/process
import gleam/http
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import models/task.{
  type CreateTaskRequest, type TaskStatus, type UpdateTaskRequest,
  CreateTaskRequest, UpdateTaskRequest,
}
import web.{type Context}
import wisp.{type Request, type Response}

/// Handle tasks routes
pub fn handle_tasks_route(
  req: Request,
  path: List(String),
  ctx: Context,
) -> Response {
  case path {
    // GET /api/tasks - List all tasks
    [] -> {
      case req.method {
        http.Get -> list_tasks(req, ctx)
        http.Post -> create_task(req, ctx)
        _ -> wisp.method_not_allowed([http.Get, http.Post])
      }
    }

    // /api/tasks/:id
    [id] -> {
      case req.method {
        http.Get -> get_task(ctx, id)
        http.Patch -> update_task(req, ctx, id)
        http.Delete -> delete_task(ctx, id)
        _ -> wisp.method_not_allowed([http.Get, http.Patch, http.Delete])
      }
    }

    // POST /api/tasks/:id/start - Start task execution
    [id, "start"] -> {
      case req.method {
        http.Post -> start_task(ctx, id)
        _ -> wisp.method_not_allowed([http.Post])
      }
    }

    // POST /api/tasks/:id/stop - Stop task execution
    [id, "stop"] -> {
      case req.method {
        http.Post -> stop_task(ctx, id)
        _ -> wisp.method_not_allowed([http.Post])
      }
    }

    // GET /api/tasks/:id/runs - List task runs
    [id, "runs"] -> {
      case req.method {
        http.Get -> list_task_runs(ctx, id)
        _ -> wisp.method_not_allowed([http.Get])
      }
    }

    // GET /api/tasks/:id/runs/:run_id - Get task run details
    [_id, "runs", run_id] -> {
      case req.method {
        http.Get -> get_task_run(ctx, run_id)
        _ -> wisp.method_not_allowed([http.Get])
      }
    }

    // GET /api/tasks/:id/runs/:run_id/logs - Get task run logs
    [_id, "runs", run_id, "logs"] -> {
      case req.method {
        http.Get -> get_task_run_logs(ctx, run_id)
        _ -> wisp.method_not_allowed([http.Get])
      }
    }

    _ -> wisp.not_found()
  }
}

/// List all tasks
fn list_tasks(req: Request, ctx: Context) -> Response {
  // Parse query parameters for filtering
  let query_params = wisp.get_query(req)

  let project_id =
    list.find(query_params, fn(p) { p.0 == "project_id" })
    |> result.map(fn(p) { p.1 })
    |> option.from_result

  let status_filter =
    list.find(query_params, fn(p) { p.0 == "status" })
    |> result.try(fn(p) { task.status_from_string(p.1) })
    |> option.from_result

  case cached_tasks.list_tasks(ctx.db, ctx.cache, project_id, status_filter) {
    Ok(task_list) -> {
      let json_body =
        json.object([#("tasks", json.array(task_list, task.to_json))])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Error(err) -> error_response(500, err)
  }
}

/// Create a new task
fn create_task(req: Request, ctx: Context) -> Response {
  use body <- wisp.require_string_body(req)

  case decode_create_task_request(body) {
    Ok(create_req) -> {
      case cached_tasks.create_task(ctx.db, ctx.cache, create_req) {
        Ok(created_task) -> {
          let json_body =
            json.object([#("task", task.to_json(created_task))])
            |> json.to_string

          wisp.json_response(json_body, 201)
        }
        Error(err) -> error_response(500, "Failed to create task: " <> err)
      }
    }
    Error(err) -> error_response(400, "Invalid request: " <> err)
  }
}

/// Get a single task
fn get_task(ctx: Context, id: String) -> Response {
  case cached_tasks.get_task(ctx.db, ctx.cache, id) {
    Ok(Some(found_task)) -> {
      let json_body =
        json.object([#("task", task.to_json(found_task))])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Ok(None) -> error_response(404, "Task not found")
    Error(err) -> error_response(500, err)
  }
}

/// Update a task
fn update_task(req: Request, ctx: Context, id: String) -> Response {
  use body <- wisp.require_string_body(req)

  case decode_update_task_request(body) {
    Ok(update_req) -> {
      case cached_tasks.update_task(ctx.db, ctx.cache, id, update_req) {
        Ok(Some(updated_task)) -> {
          let json_body =
            json.object([#("task", task.to_json(updated_task))])
            |> json.to_string

          wisp.json_response(json_body, 200)
        }
        Ok(None) -> error_response(404, "Task not found")
        Error(err) -> error_response(500, "Failed to update task: " <> err)
      }
    }
    Error(err) -> error_response(400, "Invalid request: " <> err)
  }
}

/// Delete a task
fn delete_task(ctx: Context, id: String) -> Response {
  case cached_tasks.delete_task(ctx.db, ctx.cache, id) {
    Ok(True) -> {
      let json_body =
        json.object([#("deleted", json.bool(True))])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Ok(False) -> error_response(404, "Task not found")
    Error(err) -> error_response(500, "Failed to delete task: " <> err)
  }
}

/// Start task execution
fn start_task(ctx: Context, id: String) -> Response {
  // Create a subject for progress updates (not used for REST API response)
  let progress_subject = process.new_subject()

  case executor.start_task_execution(ctx.db, id, progress_subject) {
    Ok(run_id) -> {
      let json_body =
        json.object([
          #("run_id", json.string(run_id)),
          #("status", json.string("running")),
          #("message", json.string("Task execution started")),
        ])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Error(err) -> error_response(400, err)
  }
}

/// Stop task execution
fn stop_task(ctx: Context, id: String) -> Response {
  // Find the latest running task run
  case cached_tasks.list_task_runs(ctx.db, ctx.cache, id) {
    Ok(runs) -> {
      case list.find(runs, fn(r) { r.status == task.Running }) {
        Ok(run) -> {
          case executor.cancel_task_execution(ctx.db, run.id) {
            Ok(_) -> {
              let json_body =
                json.object([
                  #("run_id", json.string(run.id)),
                  #("status", json.string("cancelled")),
                  #("message", json.string("Task execution cancelled")),
                ])
                |> json.to_string

              wisp.json_response(json_body, 200)
            }
            Error(err) -> error_response(400, err)
          }
        }
        Error(_) -> error_response(400, "No running execution found")
      }
    }
    Error(err) -> error_response(500, err)
  }
}

/// List task runs
fn list_task_runs(ctx: Context, task_id: String) -> Response {
  case cached_tasks.list_task_runs(ctx.db, ctx.cache, task_id) {
    Ok(runs) -> {
      let json_body =
        json.object([#("runs", json.array(runs, task.run_to_json))])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Error(err) -> error_response(500, err)
  }
}

/// Get task run details
fn get_task_run(ctx: Context, run_id: String) -> Response {
  case cached_tasks.get_task_run(ctx.db, ctx.cache, run_id) {
    Ok(Some(run)) -> {
      let json_body =
        json.object([#("run", task.run_to_json(run))])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Ok(None) -> error_response(404, "Task run not found")
    Error(err) -> error_response(500, err)
  }
}

/// Get task run logs
fn get_task_run_logs(ctx: Context, run_id: String) -> Response {
  case cached_tasks.list_run_logs(ctx.db, ctx.cache, run_id) {
    Ok(logs) -> {
      let json_body =
        json.object([#("logs", json.array(logs, task.log_to_json))])
        |> json.to_string

      wisp.json_response(json_body, 200)
    }
    Error(err) -> error_response(500, err)
  }
}

/// Create error response
fn error_response(status: Int, message: String) -> Response {
  let json_body =
    json.object([#("error", json.string(message))])
    |> json.to_string

  wisp.json_response(json_body, status)
}

/// Decode create task request
fn decode_create_task_request(body: String) -> Result(CreateTaskRequest, String) {
  let decoder = {
    use project_id <- decode.field("project_id", decode.string)
    use title <- decode.field("title", decode.string)
    use description <- decode.field("description", decode.string)
    use acceptance_criteria <- decode.optional_field(
      "acceptance_criteria",
      None,
      decode.optional(decode.string),
    )
    use priority <- decode.optional_field(
      "priority",
      None,
      decode.optional(decode.int),
    )
    use model_name <- decode.optional_field(
      "model_name",
      None,
      decode.optional(decode.string),
    )
    use dependencies <- decode.optional_field(
      "dependencies",
      None,
      decode.optional(decode.list(decode.string)),
    )
    use is_agentic <- decode.optional_field(
      "is_agentic",
      None,
      decode.optional(decode.bool),
    )
    use github_repo_url <- decode.optional_field(
      "github_repo_url",
      None,
      decode.optional(decode.string),
    )

    decode.success(CreateTaskRequest(
      project_id: project_id,
      title: title,
      description: description,
      acceptance_criteria: acceptance_criteria,
      priority: priority,
      model_name: model_name,
      dependencies: dependencies,
      is_agentic: is_agentic,
      github_repo_url: github_repo_url,
    ))
  }

  case json.parse(body, decoder) {
    Ok(req) -> Ok(req)
    Error(e) -> Error("Failed to parse request: " <> string.inspect(e))
  }
}

/// Decode update task request
fn decode_update_task_request(body: String) -> Result(UpdateTaskRequest, String) {
  let decoder = {
    use title <- decode.optional_field(
      "title",
      None,
      decode.optional(decode.string),
    )
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    use acceptance_criteria <- decode.optional_field(
      "acceptance_criteria",
      None,
      decode.optional(decode.string),
    )
    use status_str <- decode.optional_field(
      "status",
      None,
      decode.optional(decode.string),
    )
    use priority <- decode.optional_field(
      "priority",
      None,
      decode.optional(decode.int),
    )
    use model_name <- decode.optional_field(
      "model_name",
      None,
      decode.optional(decode.string),
    )
    use dependencies <- decode.optional_field(
      "dependencies",
      None,
      decode.optional(decode.list(decode.string)),
    )
    use is_agentic <- decode.optional_field(
      "is_agentic",
      None,
      decode.optional(decode.bool),
    )
    use github_repo_url <- decode.optional_field(
      "github_repo_url",
      None,
      decode.optional(decode.string),
    )

    let status = case status_str {
      Some(s) -> {
        case task.status_from_string(s) {
          Ok(st) -> Some(st)
          Error(_) -> None
        }
      }
      None -> None
    }

    decode.success(UpdateTaskRequest(
      title: title,
      description: description,
      acceptance_criteria: acceptance_criteria,
      status: status,
      priority: priority,
      model_name: model_name,
      dependencies: dependencies,
      is_agentic: is_agentic,
      github_repo_url: github_repo_url,
    ))
  }

  case json.parse(body, decoder) {
    Ok(req) -> Ok(req)
    Error(e) -> Error("Failed to parse request: " <> string.inspect(e))
  }
}
