import gleam/json
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import models/project

// =============================================================================
// ProjectStatus conversion tests
// =============================================================================

pub fn status_to_string_active_test() {
  project.status_to_string(project.Active)
  |> should.equal("active")
}

pub fn status_to_string_on_hold_test() {
  project.status_to_string(project.OnHold)
  |> should.equal("on_hold")
}

pub fn status_to_string_cancelled_test() {
  project.status_to_string(project.Cancelled)
  |> should.equal("cancelled")
}

pub fn status_from_string_active_test() {
  project.status_from_string("active")
  |> should.be_ok()
  |> should.equal(project.Active)
}

pub fn status_from_string_on_hold_test() {
  project.status_from_string("on_hold")
  |> should.be_ok()
  |> should.equal(project.OnHold)
}

pub fn status_from_string_cancelled_test() {
  project.status_from_string("cancelled")
  |> should.be_ok()
  |> should.equal(project.Cancelled)
}

pub fn status_from_string_invalid_test() {
  project.status_from_string("invalid")
  |> should.be_error()
}

pub fn status_from_string_empty_test() {
  project.status_from_string("")
  |> should.be_error()
}

// =============================================================================
// CreateProjectRequest decoding tests
// =============================================================================

pub fn decode_create_request_minimal_test() {
  let json_data = "{\"name\": \"Test Project\"}"

  project.decode_create_request(json_data)
  |> should.be_ok()
  |> fn(req: project.CreateProjectRequest) {
    should.equal(req.name, "Test Project")
    should.equal(req.description, None)
    should.equal(req.status, None)
    should.equal(req.github_repo_url, None)
  }
}

pub fn decode_create_request_full_test() {
  let json_data =
    "{\"name\": \"Test Project\", \"description\": \"A test\", \"status\": \"on_hold\", \"github_repo_url\": \"https://github.com/test/repo\"}"

  project.decode_create_request(json_data)
  |> should.be_ok()
  |> fn(req: project.CreateProjectRequest) {
    should.equal(req.name, "Test Project")
    should.equal(req.description, Some("A test"))
    should.equal(req.status, Some(project.OnHold))
    should.equal(req.github_repo_url, Some("https://github.com/test/repo"))
  }
}

pub fn decode_create_request_missing_name_test() {
  let json_data = "{\"description\": \"A test\"}"

  project.decode_create_request(json_data)
  |> should.be_error()
}

pub fn decode_create_request_invalid_json_test() {
  project.decode_create_request("not json")
  |> should.be_error()
}

pub fn decode_create_request_invalid_status_test() {
  let json_data = "{\"name\": \"Test\", \"status\": \"invalid_status\"}"

  project.decode_create_request(json_data)
  |> should.be_ok()
  |> fn(req: project.CreateProjectRequest) {
    // Invalid status should result in None
    should.equal(req.status, None)
  }
}

// =============================================================================
// UpdateProjectRequest decoding tests
// =============================================================================

pub fn decode_update_request_empty_test() {
  let json_data = "{}"

  project.decode_update_request(json_data)
  |> should.be_ok()
  |> fn(req: project.UpdateProjectRequest) {
    should.equal(req.name, None)
    should.equal(req.description, None)
    should.equal(req.status, None)
    should.equal(req.github_repo_url, None)
  }
}

pub fn decode_update_request_partial_test() {
  let json_data = "{\"name\": \"Updated Name\", \"status\": \"cancelled\"}"

  project.decode_update_request(json_data)
  |> should.be_ok()
  |> fn(req: project.UpdateProjectRequest) {
    should.equal(req.name, Some("Updated Name"))
    should.equal(req.status, Some(project.Cancelled))
    should.equal(req.description, None)
  }
}

// =============================================================================
// Project to_json tests
// =============================================================================

pub fn project_to_json_test() {
  let proj =
    project.Project(
      id: "test-id",
      name: "Test Project",
      description: Some("A description"),
      status: project.Active,
      github_repo_url: None,
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    project.to_json(proj)
    |> json.to_string()

  // Check that required fields are present
  should.be_true(string.contains(json_str, "\"id\":\"test-id\""))
  should.be_true(string.contains(json_str, "\"name\":\"Test Project\""))
  should.be_true(string.contains(json_str, "\"status\":\"active\""))
}

pub fn project_to_json_null_fields_test() {
  let proj =
    project.Project(
      id: "test-id",
      name: "Test",
      description: None,
      status: project.Active,
      github_repo_url: None,
      created_at: "2025-01-01T00:00:00Z",
      updated_at: "2025-01-01T00:00:00Z",
    )

  let json_str =
    project.to_json(proj)
    |> json.to_string()

  // Null fields should be present as null
  should.be_true(string.contains(json_str, "\"description\":null"))
  should.be_true(string.contains(json_str, "\"github_repo_url\":null"))
}
