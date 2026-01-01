import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}

/// Project status enum
pub type ProjectStatus {
  Active
  OnHold
  Cancelled
}

/// Project entity
pub type Project {
  Project(
    id: String,
    name: String,
    description: Option(String),
    status: ProjectStatus,
    github_repo_url: Option(String),
    created_at: String,
    updated_at: String,
  )
}

/// Request to create a new project
pub type CreateProjectRequest {
  CreateProjectRequest(
    name: String,
    description: Option(String),
    status: Option(ProjectStatus),
    github_repo_url: Option(String),
  )
}

/// Request to update a project
pub type UpdateProjectRequest {
  UpdateProjectRequest(
    name: Option(String),
    description: Option(String),
    status: Option(ProjectStatus),
    github_repo_url: Option(String),
  )
}

/// Convert ProjectStatus to string for database
pub fn status_to_string(status: ProjectStatus) -> String {
  case status {
    Active -> "active"
    OnHold -> "on_hold"
    Cancelled -> "cancelled"
  }
}

/// Parse string to ProjectStatus
pub fn status_from_string(s: String) -> Result(ProjectStatus, Nil) {
  case s {
    "active" -> Ok(Active)
    "on_hold" -> Ok(OnHold)
    "cancelled" -> Ok(Cancelled)
    _ -> Error(Nil)
  }
}

/// Decoder for ProjectStatus from database string
pub fn status_decoder() -> decode.Decoder(ProjectStatus) {
  decode.string
  |> decode.then(fn(s) {
    case status_from_string(s) {
      Ok(status) -> decode.success(status)
      Error(_) -> decode.failure(Active, "ProjectStatus")
    }
  })
}

/// Decode CreateProjectRequest from JSON
pub fn decode_create_request(
  data: String,
) -> Result(CreateProjectRequest, json.DecodeError) {
  let decoder = {
    use name <- decode.field("name", decode.string)
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    use status_str <- decode.optional_field(
      "status",
      None,
      decode.optional(decode.string),
    )
    use github_repo_url <- decode.optional_field(
      "github_repo_url",
      None,
      decode.optional(decode.string),
    )

    let status = case status_str {
      Some(s) -> {
        case status_from_string(s) {
          Ok(st) -> Some(st)
          Error(_) -> None
        }
      }
      None -> None
    }

    decode.success(CreateProjectRequest(
      name: name,
      description: description,
      status: status,
      github_repo_url: github_repo_url,
    ))
  }

  json.parse(data, decoder)
}

/// Decode UpdateProjectRequest from JSON
pub fn decode_update_request(
  data: String,
) -> Result(UpdateProjectRequest, json.DecodeError) {
  let decoder = {
    use name <- decode.optional_field(
      "name",
      None,
      decode.optional(decode.string),
    )
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    use status_str <- decode.optional_field(
      "status",
      None,
      decode.optional(decode.string),
    )
    use github_repo_url <- decode.optional_field(
      "github_repo_url",
      None,
      decode.optional(decode.string),
    )

    let status = case status_str {
      Some(s) -> {
        case status_from_string(s) {
          Ok(st) -> Some(st)
          Error(_) -> None
        }
      }
      None -> None
    }

    decode.success(UpdateProjectRequest(
      name: name,
      description: description,
      status: status,
      github_repo_url: github_repo_url,
    ))
  }

  json.parse(data, decoder)
}

/// Convert Project to JSON
pub fn to_json(project: Project) -> json.Json {
  json.object([
    #("id", json.string(project.id)),
    #("name", json.string(project.name)),
    #("description", option_to_json(project.description)),
    #("status", json.string(status_to_string(project.status))),
    #("github_repo_url", option_to_json(project.github_repo_url)),
    #("created_at", json.string(project.created_at)),
    #("updated_at", json.string(project.updated_at)),
  ])
}

fn option_to_json(opt: Option(String)) -> json.Json {
  case opt {
    Some(s) -> json.string(s)
    None -> json.null()
  }
}
