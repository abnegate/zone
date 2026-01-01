import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}

/// Workspace entity (nested under Organization)
pub type Workspace {
  Workspace(
    id: String,
    organization_id: String,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Bool,
    created_at: String,
    updated_at: String,
  )
}

/// Request to create a new workspace
pub type CreateWorkspaceRequest {
  CreateWorkspaceRequest(
    name: String,
    slug: String,
    description: Option(String),
  )
}

/// Request to update a workspace
pub type UpdateWorkspaceRequest {
  UpdateWorkspaceRequest(
    name: Option(String),
    slug: Option(String),
    description: Option(String),
    is_active: Option(Bool),
  )
}

/// Decode CreateWorkspaceRequest from JSON
pub fn decode_create_request(
  data: String,
) -> Result(CreateWorkspaceRequest, json.DecodeError) {
  let decoder = {
    use name <- decode.field("name", decode.string)
    use slug <- decode.field("slug", decode.string)
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )

    decode.success(CreateWorkspaceRequest(
      name: name,
      slug: slug,
      description: description,
    ))
  }

  json.parse(data, decoder)
}

/// Decode UpdateWorkspaceRequest from JSON
pub fn decode_update_request(
  data: String,
) -> Result(UpdateWorkspaceRequest, json.DecodeError) {
  let decoder = {
    use name <- decode.optional_field(
      "name",
      None,
      decode.optional(decode.string),
    )
    use slug <- decode.optional_field(
      "slug",
      None,
      decode.optional(decode.string),
    )
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    use is_active <- decode.optional_field(
      "is_active",
      None,
      decode.optional(decode.bool),
    )

    decode.success(UpdateWorkspaceRequest(
      name: name,
      slug: slug,
      description: description,
      is_active: is_active,
    ))
  }

  json.parse(data, decoder)
}

/// Convert Workspace to JSON
pub fn to_json(ws: Workspace) -> json.Json {
  json.object([
    #("id", json.string(ws.id)),
    #("organization_id", json.string(ws.organization_id)),
    #("name", json.string(ws.name)),
    #("slug", json.string(ws.slug)),
    #("description", option_to_json(ws.description)),
    #("is_active", json.bool(ws.is_active)),
    #("created_at", json.string(ws.created_at)),
    #("updated_at", json.string(ws.updated_at)),
  ])
}

fn option_to_json(opt: Option(String)) -> json.Json {
  case opt {
    Some(s) -> json.string(s)
    None -> json.null()
  }
}
