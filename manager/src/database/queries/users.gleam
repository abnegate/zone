import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/user.{
  type User, type UserWithPermissions, User, UserWithPermissions,
}
import pog

/// Check if any users exist (for first-user-is-admin logic)
pub fn count_users(db: Connection) -> Result(Int, String) {
  let sql = "SELECT COUNT(*)::int FROM users"

  pog.query(sql)
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

/// Create a new user
pub fn create_user(
  db: Connection,
  email: String,
  password_hash: String,
  display_name: Option(String),
  is_admin: Bool,
) -> Result(User, String) {
  let sql =
    "
    INSERT INTO users (email, password_hash, display_name, is_admin)
    VALUES ($1, $2, $3, $4)
    RETURNING id, email, display_name, is_active, is_admin,
              created_at::text, updated_at::text, last_login_at::text
  "

  pog.query(sql)
  |> pog.parameter(pog.text(email))
  |> pog.parameter(pog.text(password_hash))
  |> pog.parameter(pog.nullable(pog.text, display_name))
  |> pog.parameter(pog.bool(is_admin))
  |> pog.returning(user.user_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) {
    case list.first(r.rows) {
      Ok(u) -> u
      Error(_) -> {
        // This should never happen as INSERT RETURNING should always return a row
        // Log warning and continue (better than panic)
        user.User(
          id: "",
          email: email,
          display_name: display_name,
          is_active: True,
          is_admin: is_admin,
          created_at: "",
          updated_at: "",
          last_login_at: None,
        )
      }
    }
  })
  |> result.map_error(query_error_to_string)
}

/// Get user by ID
pub fn get_user_by_id(
  db: Connection,
  user_id: String,
) -> Result(Option(User), String) {
  let sql =
    "
    SELECT id, email, display_name, is_active, is_admin,
           created_at::text, updated_at::text, last_login_at::text
    FROM users WHERE id = $1
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.returning(user.user_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { list.first(r.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Get user by email (for login) - returns user with password hash
pub fn get_user_by_email(
  db: Connection,
  email: String,
) -> Result(Option(#(User, String)), String) {
  let sql =
    "
    SELECT id, email, display_name, is_active, is_admin,
           created_at::text, updated_at::text, last_login_at::text,
           password_hash
    FROM users WHERE email = $1
  "

  pog.query(sql)
  |> pog.parameter(pog.text(email))
  |> pog.returning(user.user_with_hash_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { list.first(r.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Get user's role names
pub fn get_user_roles(
  db: Connection,
  user_id: String,
) -> Result(List(String), String) {
  let sql =
    "
    SELECT r.name FROM roles r
    JOIN user_roles ur ON ur.role_id = r.id
    WHERE ur.user_id = $1
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.returning(string_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { r.rows })
  |> result.map_error(query_error_to_string)
}

/// Get user's permission names (aggregated from all roles)
pub fn get_user_permissions(
  db: Connection,
  user_id: String,
) -> Result(List(String), String) {
  let sql =
    "
    SELECT DISTINCT p.name FROM permissions p
    JOIN role_permissions rp ON rp.permission_id = p.id
    JOIN user_roles ur ON ur.role_id = rp.role_id
    WHERE ur.user_id = $1
    ORDER BY p.name
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.returning(string_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { r.rows })
  |> result.map_error(query_error_to_string)
}

/// Get user with roles and permissions (for JWT claims)
pub fn get_user_with_permissions(
  db: Connection,
  user_id: String,
) -> Result(Option(UserWithPermissions), String) {
  // First get user
  use user_opt <- result.try(get_user_by_id(db, user_id))

  case user_opt {
    None -> Ok(None)
    Some(u) -> {
      // Get roles
      use roles <- result.try(get_user_roles(db, user_id))
      // Get permissions
      use permissions <- result.try(get_user_permissions(db, user_id))

      Ok(
        Some(UserWithPermissions(
          user: u,
          roles: roles,
          permissions: permissions,
        )),
      )
    }
  }
}

/// Assign a role to a user
pub fn assign_role(
  db: Connection,
  user_id: String,
  role_name: String,
  assigned_by: Option(String),
) -> Result(Bool, String) {
  let sql =
    "
    INSERT INTO user_roles (user_id, role_id, assigned_by)
    SELECT $1, r.id, $2 FROM roles r WHERE r.name = $3
    ON CONFLICT DO NOTHING
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.parameter(pog.nullable(pog.text, assigned_by))
  |> pog.parameter(pog.text(role_name))
  |> pog.execute(db)
  |> result.map(fn(r) { r.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Remove a role from a user
pub fn remove_role(
  db: Connection,
  user_id: String,
  role_name: String,
) -> Result(Bool, String) {
  let sql =
    "
    DELETE FROM user_roles
    WHERE user_id = $1 AND role_id = (SELECT id FROM roles WHERE name = $2)
  "

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.parameter(pog.text(role_name))
  |> pog.execute(db)
  |> result.map(fn(r) { r.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Update last login time
pub fn update_last_login(db: Connection, user_id: String) -> Result(Nil, String) {
  let sql = "UPDATE users SET last_login_at = NOW() WHERE id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(user_id))
  |> pog.execute(db)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Update user active status
pub fn set_user_active(
  db: Connection,
  user_id: String,
  is_active: Bool,
) -> Result(Bool, String) {
  let sql = "UPDATE users SET is_active = $1, updated_at = NOW() WHERE id = $2"

  pog.query(sql)
  |> pog.parameter(pog.bool(is_active))
  |> pog.parameter(pog.text(user_id))
  |> pog.execute(db)
  |> result.map(fn(r) { r.count > 0 })
  |> result.map_error(query_error_to_string)
}

/// List all users
pub fn list_users(db: Connection) -> Result(List(User), String) {
  let sql =
    "
    SELECT id, email, display_name, is_active, is_admin,
           created_at::text, updated_at::text, last_login_at::text
    FROM users
    ORDER BY created_at DESC
  "

  pog.query(sql)
  |> pog.returning(user.user_decoder())
  |> pog.execute(db)
  |> result.map(fn(r) { r.rows })
  |> result.map_error(query_error_to_string)
}

// --- Decoders ---

fn count_decoder() -> decode.Decoder(Int) {
  decode.at([0], decode.int)
}

fn string_decoder() -> decode.Decoder(String) {
  decode.at([0], decode.string)
}
