import auth/jwt
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/timestamp
import pog

/// Store a refresh token (hashed)
pub fn create_refresh_token(
  db: Connection,
  user_id: String,
  token: String,
  expires_at: Int,
  user_agent: Option(String),
  ip_address: Option(String),
) -> Result(Nil, String) {
  let token_hash = jwt.hash_token(token)
  // Convert Unix timestamp (seconds) to gleam timestamp
  let expires_ts = timestamp.from_unix_seconds(expires_at)

  let sql =
    "
    INSERT INTO refresh_tokens (user_id, token_hash, expires_at, user_agent, ip_address)
    VALUES ($1::uuid, $2, $3, $4, $5)
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.parameter(pog.text(token_hash))
  |> pog.parameter(pog.timestamp(expires_ts))
  |> pog.parameter(pog.nullable(pog.text, user_agent))
  |> pog.parameter(pog.nullable(pog.text, ip_address))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Validate a refresh token and get user_id
pub fn validate_refresh_token(
  db: Connection,
  token: String,
) -> Result(Option(String), String) {
  let token_hash = jwt.hash_token(token)

  let sql =
    "
    SELECT user_id FROM refresh_tokens
    WHERE token_hash = $1
      AND expires_at > NOW()
      AND revoked_at IS NULL
  "

  pog.query(sql)
  |> pog.parameter(pog.text(token_hash))
  |> pog.returning(user_id_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { list.first(r.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Revoke a refresh token
pub fn revoke_refresh_token(
  db: Connection,
  token: String,
) -> Result(Bool, String) {
  let token_hash = jwt.hash_token(token)

  let sql =
    "
    UPDATE refresh_tokens
    SET revoked_at = NOW()
    WHERE token_hash = $1 AND revoked_at IS NULL
  "

  pog.query(sql)
  |> pog.parameter(pog.text(token_hash))
  |> pog.execute(db)
  |> result.map(fn(r) { r.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Revoke all refresh tokens for a user (logout everywhere)
pub fn revoke_all_user_tokens(
  db: Connection,
  user_id: String,
) -> Result(Int, String) {
  let sql =
    "
    UPDATE refresh_tokens
    SET revoked_at = NOW()
    WHERE user_id = $1 AND revoked_at IS NULL
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.execute(db)
  |> result.map(fn(r) { r.count })
  |> result.map_error(query_error_to_string)
}

/// Clean up expired tokens (run periodically)
pub fn cleanup_expired_tokens(db: Connection) -> Result(Int, String) {
  let sql = "DELETE FROM refresh_tokens WHERE expires_at < NOW()"

  pog.query(sql)
  |> pog.execute(db)
  |> result.map(fn(r) { r.count })
  |> result.map_error(query_error_to_string)
}

/// Get count of active tokens for a user
pub fn count_user_tokens(db: Connection, user_id: String) -> Result(Int, String) {
  let sql =
    "
    SELECT COUNT(*)::int FROM refresh_tokens
    WHERE user_id = $1 AND expires_at > NOW() AND revoked_at IS NULL
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.returning(count_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) {
    case list.first(r.rows) {
      Ok(count) -> count
      Error(_) -> 0
    }
  })
  |> result.map_error(query_error_to_string)
}

// --- Decoders ---

fn user_id_decoder() -> decode.Decoder(String) {
  decode.at([0], decode.string)
}

fn count_decoder() -> decode.Decoder(Int) {
  decode.at([0], decode.int)
}
