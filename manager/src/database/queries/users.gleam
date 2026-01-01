import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/user.{
  type User, type UserWithPermissions, User, UserWithPermissions,
}

// =============================================================================
// User Queries (using Squirrel-generated SQL)
// =============================================================================

/// Check if any users exist (for first-user-is-admin logic)
pub fn count_users(db: Connection) -> Result(Int, String) {
  sql.count_users(db)
  |> result.map(fn(rows) {
    case list.first(rows) {
      Ok(row) -> row.count
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
  sql.create_user(db, email, password_hash, display_name, is_admin)
  |> result.map(fn(rows) {
    case list.first(rows) {
      Ok(row) -> user_row_to_user(row)
      Error(_) ->
        User(
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
  })
  |> result.map_error(query_error_to_string)
}

/// Get user by ID
pub fn get_user_by_id(
  db: Connection,
  user_id: String,
) -> Result(Option(User), String) {
  sql.get_user_by_id(db, user_id)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(user_row_to_user)
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Get user by email (for login) - returns user with password hash
pub fn get_user_by_email(
  db: Connection,
  email: String,
) -> Result(Option(#(User, String)), String) {
  sql.get_user_by_email(db, email)
  |> result.map(fn(rows) {
    list.first(rows)
    |> result.map(fn(row) { #(user_with_hash_to_user(row), row.password_hash) })
    |> option.from_result
  })
  |> result.map_error(query_error_to_string)
}

/// Get user's role names
pub fn get_user_roles(
  db: Connection,
  user_id: String,
) -> Result(List(String), String) {
  sql.get_user_roles(db, user_id)
  |> result.map(fn(rows) { list.map(rows, fn(row) { row.name }) })
  |> result.map_error(query_error_to_string)
}

/// Get user's permission names (aggregated from all roles)
pub fn get_user_permissions(
  db: Connection,
  user_id: String,
) -> Result(List(String), String) {
  sql.get_user_permissions(db, user_id)
  |> result.map(fn(rows) { list.map(rows, fn(row) { row.name }) })
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
  sql.assign_user_role(db, user_id, assigned_by, role_name)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Remove a role from a user
pub fn remove_role(
  db: Connection,
  user_id: String,
  role_name: String,
) -> Result(Bool, String) {
  sql.remove_user_role(db, user_id, role_name)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

/// Update last login time
pub fn update_last_login(db: Connection, user_id: String) -> Result(Nil, String) {
  sql.update_user_last_login(db, user_id)
  |> result.map(fn(_) { Nil })
  |> result.map_error(query_error_to_string)
}

/// Update user active status
pub fn set_user_active(
  db: Connection,
  user_id: String,
  is_active: Bool,
) -> Result(Bool, String) {
  sql.set_user_active(db, is_active, user_id)
  |> result.map(fn(count) { count > 0 })
  |> result.map_error(query_error_to_string)
}

/// List all users
pub fn list_users(db: Connection) -> Result(List(User), String) {
  sql.list_users(db)
  |> result.map(fn(rows) { list.map(rows, user_row_to_user) })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Mapping
// =============================================================================

fn user_row_to_user(row: sql.GetUserByIdRow) -> User {
  User(
    id: row.id,
    email: row.email,
    display_name: row.display_name,
    is_active: row.is_active,
    is_admin: row.is_admin,
    created_at: row.created_at,
    updated_at: row.updated_at,
    last_login_at: row.last_login_at,
  )
}

fn user_with_hash_to_user(row: sql.GetUserByEmailRow) -> User {
  User(
    id: row.id,
    email: row.email,
    display_name: row.display_name,
    is_active: row.is_active,
    is_admin: row.is_admin,
    created_at: row.created_at,
    updated_at: row.updated_at,
    last_login_at: row.last_login_at,
  )
}
