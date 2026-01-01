import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}

/// Organization entity (top-level resource)
pub type Organization {
  Organization(
    id: String,
    name: String,
    slug: String,
    description: Option(String),
    is_active: Bool,
    created_at: String,
    updated_at: String,
  )
}

/// Request to create a new organization
pub type CreateOrganizationRequest {
  CreateOrganizationRequest(
    name: String,
    slug: String,
    description: Option(String),
  )
}

/// Request to update an organization
pub type UpdateOrganizationRequest {
  UpdateOrganizationRequest(
    name: Option(String),
    slug: Option(String),
    description: Option(String),
    is_active: Option(Bool),
  )
}

/// Decode CreateOrganizationRequest from JSON
pub fn decode_create_request(
  data: String,
) -> Result(CreateOrganizationRequest, json.DecodeError) {
  let decoder = {
    use name <- decode.field("name", decode.string)
    use slug <- decode.field("slug", decode.string)
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )

    decode.success(CreateOrganizationRequest(
      name: name,
      slug: slug,
      description: description,
    ))
  }

  json.parse(data, decoder)
}

/// Decode UpdateOrganizationRequest from JSON
pub fn decode_update_request(
  data: String,
) -> Result(UpdateOrganizationRequest, json.DecodeError) {
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

    decode.success(UpdateOrganizationRequest(
      name: name,
      slug: slug,
      description: description,
      is_active: is_active,
    ))
  }

  json.parse(data, decoder)
}

/// Convert Organization to JSON
pub fn to_json(org: Organization) -> json.Json {
  json.object([
    #("id", json.string(org.id)),
    #("name", json.string(org.name)),
    #("slug", json.string(org.slug)),
    #("description", option_to_json(org.description)),
    #("is_active", json.bool(org.is_active)),
    #("created_at", json.string(org.created_at)),
    #("updated_at", json.string(org.updated_at)),
  ])
}

fn option_to_json(opt: Option(String)) -> json.Json {
  case opt {
    Some(s) -> json.string(s)
    None -> json.null()
  }
}
