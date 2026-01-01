import auth/jwt
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/timestamp
import youid/uuid

// =============================================================================
// Refresh Token Queries (using Squirrel-generated SQL)
// =============================================================================

/// Store a refresh token (hashed)
pub fn create_refresh_token(
  db: Connection,
  user_id: String,
  token: String,
  expires_at: Int,
  user_agent: Option(String),
  ip_address: Option(String),
) -> Result(Nil, String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      let token_hash = jwt.hash_token(token)
      // Convert Unix timestamp (seconds) to gleam timestamp
      let expires_ts = timestamp.from_unix_seconds(expires_at)
      let user_agent_str = option.unwrap(user_agent, "")
      let ip_address_str = option.unwrap(ip_address, "")

      sql.create_refresh_token(
        db,
        uuid_id,
        token_hash,
        expires_ts,
        user_agent_str,
        ip_address_str,
      )
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Validate a refresh token and get user_id
pub fn validate_refresh_token(
  db: Connection,
  token: String,
) -> Result(Option(String), String) {
  let token_hash = jwt.hash_token(token)

  sql.validate_refresh_token(db, token_hash)
  |> result.map(fn(returned) {
    list.first(returned.rows)
    |> result.map(fn(row) { uuid.to_string(row.user_id) })
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Revoke a refresh token
pub fn revoke_refresh_token(
  db: Connection,
  token: String,
) -> Result(Bool, String) {
  let token_hash = jwt.hash_token(token)

  sql.revoke_refresh_token(db, token_hash)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Revoke all refresh tokens for a user (logout everywhere)
pub fn revoke_all_user_tokens(
  db: Connection,
  user_id: String,
) -> Result(Int, String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) ->
      sql.revoke_all_user_tokens(db, uuid_id)
      |> result.map(fn(returned) { returned.count })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Clean up expired tokens (run periodically)
pub fn cleanup_expired_tokens(db: Connection) -> Result(Int, String) {
  sql.cleanup_expired_tokens(db)
  |> result.map(fn(returned) { returned.count })
  |> result.map_error(query_error_to_string)
}

/// Get count of active tokens for a user
pub fn count_user_tokens(db: Connection, user_id: String) -> Result(Int, String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) ->
      sql.count_user_tokens(db, uuid_id)
      |> result.map(fn(returned) {
        case list.first(returned.rows) {
          Ok(row) -> row.count
          Error(_) -> 0
        }
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}
