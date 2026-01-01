import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None}

/// User entity
pub type User {
  User(
    id: String,
    email: String,
    display_name: Option(String),
    is_active: Bool,
    is_admin: Bool,
    created_at: String,
    updated_at: String,
    last_login_at: Option(String),
  )
}

/// User with roles and permissions for JWT claims
pub type UserWithPermissions {
  UserWithPermissions(
    user: User,
    roles: List(String),
    permissions: List(String),
  )
}

/// Registration request
pub type RegisterRequest {
  RegisterRequest(email: String, password: String, display_name: Option(String))
}

/// Login request
pub type LoginRequest {
  LoginRequest(email: String, password: String)
}

/// Refresh token request
pub type RefreshRequest {
  RefreshRequest(refresh_token: String)
}

/// Auth response with tokens
pub type AuthResponse {
  AuthResponse(
    access_token: String,
    refresh_token: String,
    expires_in: Int,
    user: User,
    roles: List(String),
    permissions: List(String),
  )
}

/// Decode RegisterRequest from JSON
pub fn decode_register_request(
  data: String,
) -> Result(RegisterRequest, json.DecodeError) {
  let decoder = {
    use email <- decode.field("email", decode.string)
    use password <- decode.field("password", decode.string)
    use display_name <- decode.optional_field(
      "display_name",
      None,
      decode.optional(decode.string),
    )
    decode.success(RegisterRequest(
      email: email,
      password: password,
      display_name: display_name,
    ))
  }

  json.parse(data, decoder)
}

/// Decode LoginRequest from JSON
pub fn decode_login_request(
  data: String,
) -> Result(LoginRequest, json.DecodeError) {
  let decoder = {
    use email <- decode.field("email", decode.string)
    use password <- decode.field("password", decode.string)
    decode.success(LoginRequest(email: email, password: password))
  }

  json.parse(data, decoder)
}

/// Decode RefreshRequest from JSON
pub fn decode_refresh_request(
  data: String,
) -> Result(RefreshRequest, json.DecodeError) {
  let decoder = {
    use refresh_token <- decode.field("refresh_token", decode.string)
    decode.success(RefreshRequest(refresh_token: refresh_token))
  }

  json.parse(data, decoder)
}

/// Convert User to JSON
pub fn to_json(user: User) -> json.Json {
  json.object([
    #("id", json.string(user.id)),
    #("email", json.string(user.email)),
    #("display_name", case user.display_name {
      option.Some(name) -> json.string(name)
      option.None -> json.null()
    }),
    #("is_active", json.bool(user.is_active)),
    #("is_admin", json.bool(user.is_admin)),
    #("created_at", json.string(user.created_at)),
    #("updated_at", json.string(user.updated_at)),
    #("last_login_at", case user.last_login_at {
      option.Some(ts) -> json.string(ts)
      option.None -> json.null()
    }),
  ])
}

/// Convert AuthResponse to JSON
pub fn auth_response_to_json(resp: AuthResponse) -> json.Json {
  json.object([
    #("access_token", json.string(resp.access_token)),
    #("refresh_token", json.string(resp.refresh_token)),
    #("expires_in", json.int(resp.expires_in)),
    #("user", to_json(resp.user)),
    #("roles", json.array(resp.roles, json.string)),
    #("permissions", json.array(resp.permissions, json.string)),
  ])
}

/// Row decoder for User from database
pub fn user_decoder() -> decode.Decoder(User) {
  use id <- decode.field(0, decode.string)
  use email <- decode.field(1, decode.string)
  use display_name <- decode.field(2, decode.optional(decode.string))
  use is_active <- decode.field(3, decode.bool)
  use is_admin <- decode.field(4, decode.bool)
  use created_at <- decode.field(5, decode.string)
  use updated_at <- decode.field(6, decode.string)
  use last_login_at <- decode.field(7, decode.optional(decode.string))
  decode.success(User(
    id: id,
    email: email,
    display_name: display_name,
    is_active: is_active,
    is_admin: is_admin,
    created_at: created_at,
    updated_at: updated_at,
    last_login_at: last_login_at,
  ))
}

/// Row decoder for User with password hash from database
pub fn user_with_hash_decoder() -> decode.Decoder(#(User, String)) {
  use id <- decode.field(0, decode.string)
  use email <- decode.field(1, decode.string)
  use display_name <- decode.field(2, decode.optional(decode.string))
  use is_active <- decode.field(3, decode.bool)
  use is_admin <- decode.field(4, decode.bool)
  use created_at <- decode.field(5, decode.string)
  use updated_at <- decode.field(6, decode.string)
  use last_login_at <- decode.field(7, decode.optional(decode.string))
  use password_hash <- decode.field(8, decode.string)
  decode.success(#(
    User(
      id: id,
      email: email,
      display_name: display_name,
      is_active: is_active,
      is_admin: is_admin,
      created_at: created_at,
      updated_at: updated_at,
      last_login_at: last_login_at,
    ),
    password_hash,
  ))
}
