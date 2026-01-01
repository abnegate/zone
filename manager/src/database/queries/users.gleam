import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/user.{
  type User, type UserWithPermissions, User, UserWithPermissions,
}
import youid/uuid

// =============================================================================
// User Queries (using Squirrel-generated SQL)
// =============================================================================

/// Check if any users exist (for first-user-is-admin logic)
pub fn count_users(db: Connection) -> Result(Int, String) {
  sql.count_users(db)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
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
  let display_name_str = option.unwrap(display_name, "")
  sql.create_user(db, email, password_hash, display_name_str, is_admin)
  |> result.map(fn(returned) {
    case list.first(returned.rows) {
      Ok(row) -> create_user_row_to_user(row)
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
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      sql.get_user_by_id(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(user_row_to_user)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get user by email (for login) - returns user with password hash
pub fn get_user_by_email(
  db: Connection,
  email: String,
) -> Result(Option(#(User, String)), String) {
  sql.get_user_by_email(db, email)
  |> result.map(fn(returned) {
    list.first(returned.rows)
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
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      sql.get_user_roles(db, uuid_id)
      |> result.map(fn(returned) {
        list.map(returned.rows, fn(row) { row.name })
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Get user's permission names (aggregated from all roles)
pub fn get_user_permissions(
  db: Connection,
  user_id: String,
) -> Result(List(String), String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      sql.get_user_permissions(db, uuid_id)
      |> result.map(fn(returned) {
        list.map(returned.rows, fn(row) { row.name })
      })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
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
  assigned_by: String,
) -> Result(Bool, String) {
  case uuid.from_string(user_id), uuid.from_string(assigned_by) {
    Ok(uuid_id), Ok(assigned_by_uuid) -> {
      sql.assign_user_role(db, uuid_id, assigned_by_uuid, role_name)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    _, _ -> Error("Invalid UUID format")
  }
}

/// Remove a role from a user
pub fn remove_role(
  db: Connection,
  user_id: String,
  role_name: String,
) -> Result(Bool, String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      sql.remove_user_role(db, uuid_id, role_name)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Update last login time
pub fn update_last_login(db: Connection, user_id: String) -> Result(Nil, String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      sql.update_user_last_login(db, uuid_id)
      |> result.map(fn(_) { Nil })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Update user active status
pub fn set_user_active(
  db: Connection,
  user_id: String,
  is_active: Bool,
) -> Result(Bool, String) {
  case uuid.from_string(user_id) {
    Ok(uuid_id) -> {
      sql.set_user_active(db, is_active, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// List all users
pub fn list_users(db: Connection) -> Result(List(User), String) {
  sql.list_users(db)
  |> result.map(fn(returned) { list.map(returned.rows, list_user_row_to_user) })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Mapping Helpers
// =============================================================================

fn bool_option_to_bool(opt: Option(Bool)) -> Bool {
  option.unwrap(opt, False)
}

fn string_option_to_option(s: String) -> Option(String) {
  case s {
    "" -> None
    _ -> Some(s)
  }
}

fn user_row_to_user(row: sql.GetUserByIdRow) -> User {
  User(
    id: uuid.to_string(row.id),
    email: row.email,
    display_name: row.display_name,
    is_active: bool_option_to_bool(row.is_active),
    is_admin: bool_option_to_bool(row.is_admin),
    created_at: row.created_at,
    updated_at: row.updated_at,
    last_login_at: string_option_to_option(row.last_login_at),
  )
}

fn user_with_hash_to_user(row: sql.GetUserByEmailRow) -> User {
  User(
    id: uuid.to_string(row.id),
    email: row.email,
    display_name: row.display_name,
    is_active: bool_option_to_bool(row.is_active),
    is_admin: bool_option_to_bool(row.is_admin),
    created_at: row.created_at,
    updated_at: row.updated_at,
    last_login_at: string_option_to_option(row.last_login_at),
  )
}

fn create_user_row_to_user(row: sql.CreateUserRow) -> User {
  User(
    id: uuid.to_string(row.id),
    email: row.email,
    display_name: row.display_name,
    is_active: bool_option_to_bool(row.is_active),
    is_admin: bool_option_to_bool(row.is_admin),
    created_at: row.created_at,
    updated_at: row.updated_at,
    last_login_at: string_option_to_option(row.last_login_at),
  )
}

fn list_user_row_to_user(row: sql.ListUsersRow) -> User {
  User(
    id: uuid.to_string(row.id),
    email: row.email,
    display_name: row.display_name,
    is_active: bool_option_to_bool(row.is_active),
    is_admin: bool_option_to_bool(row.is_admin),
    created_at: row.created_at,
    updated_at: row.updated_at,
    last_login_at: string_option_to_option(row.last_login_at),
  )
}
