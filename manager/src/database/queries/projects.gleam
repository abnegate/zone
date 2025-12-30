import database/connection.{type Connection}
import gleam/option.{type Option}
import models/project.{
  type CreateProjectRequest, type Project, type ProjectStatus,
  type UpdateProjectRequest,
}

/// List all projects, optionally filtered by status
/// (Placeholder - returns empty list until DB implementation)
pub fn list_projects(
  _db: Connection,
  _status_filter: Option(ProjectStatus),
) -> Result(List(Project), String) {
  // TODO: Implement with actual DB query
  Ok([])
}

/// Get a single project by ID
/// (Placeholder - returns None until DB implementation)
pub fn get_project(
  _db: Connection,
  _id: String,
) -> Result(Option(Project), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Create a new project
/// (Placeholder - returns error until DB implementation)
pub fn create_project(
  _db: Connection,
  _req: CreateProjectRequest,
) -> Result(Project, String) {
  // TODO: Implement with actual DB query
  Error("Database not yet implemented")
}

/// Update an existing project
/// (Placeholder - returns None until DB implementation)
pub fn update_project(
  _db: Connection,
  _id: String,
  _req: UpdateProjectRequest,
) -> Result(Option(Project), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Delete a project by ID
/// (Placeholder - returns false until DB implementation)
pub fn delete_project(_db: Connection, _id: String) -> Result(Bool, String) {
  // TODO: Implement with actual DB query
  Ok(False)
}

/// Link a GitHub repository to a project
/// (Placeholder - returns None until DB implementation)
pub fn link_github(
  _db: Connection,
  _id: String,
  _repo_url: String,
  _access_token: Option(String),
) -> Result(Option(Project), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}

/// Unlink GitHub repository from a project
/// (Placeholder - returns None until DB implementation)
pub fn unlink_github(
  _db: Connection,
  _id: String,
) -> Result(Option(Project), String) {
  // TODO: Implement with actual DB query
  Ok(option.None)
}
