/// Permission constants for type-safe checking
/// Format: {resource}:{action}
// Projects permissions

pub const projects_create = "projects:create"

pub const projects_read = "projects:read"

pub const projects_update = "projects:update"

pub const projects_delete = "projects:delete"

// Tasks permissions
pub const tasks_create = "tasks:create"

pub const tasks_read = "tasks:read"

pub const tasks_update = "tasks:update"

pub const tasks_delete = "tasks:delete"

// Chats permissions
pub const chats_create = "chats:create"

pub const chats_read = "chats:read"

pub const chats_update = "chats:update"

pub const chats_delete = "chats:delete"

// Sources permissions
pub const sources_create = "sources:create"

pub const sources_read = "sources:read"

pub const sources_update = "sources:update"

pub const sources_delete = "sources:delete"

// Models permissions
pub const models_create = "models:create"

pub const models_read = "models:read"

pub const models_update = "models:update"

pub const models_delete = "models:delete"

// Wiki permissions
pub const wiki_create = "wiki:create"

pub const wiki_read = "wiki:read"

pub const wiki_update = "wiki:update"

pub const wiki_delete = "wiki:delete"

// Users permissions (admin only)
pub const users_create = "users:create"

pub const users_read = "users:read"

pub const users_update = "users:update"

pub const users_delete = "users:delete"

/// Build permission string from resource and action
pub fn for_resource(resource: String, action: String) -> String {
  resource <> ":" <> action
}

/// Get the permission required for a given HTTP method on a resource
pub fn for_method(resource: String, method: String) -> String {
  let action = case method {
    "GET" | "HEAD" -> "read"
    "POST" -> "create"
    "PUT" | "PATCH" -> "update"
    "DELETE" -> "delete"
    _ -> "read"
  }
  for_resource(resource, action)
}

/// Default role ID for admin
pub const admin_role_id = "00000000-0000-0000-0000-000000000001"

/// Default role ID for standard user
pub const user_role_id = "00000000-0000-0000-0000-000000000002"

/// Default role ID for viewer
pub const viewer_role_id = "00000000-0000-0000-0000-000000000003"

/// Default role names
pub const admin_role = "admin"

pub const user_role = "user"

pub const viewer_role = "viewer"
