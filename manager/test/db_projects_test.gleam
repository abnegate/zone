import database/queries/projects
import gleam/list
import gleam/option.{None, Some}
import gleeunit/should
import models/project.{
  Active, Cancelled, CreateProjectRequest, OnHold, UpdateProjectRequest,
}

import test_db

// =============================================================================
// Project CRUD Tests
// =============================================================================

pub fn create_project_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test Project",
        description: Some("A test project"),
        status: None,
        github_repo_url: None,
      )

    let proj = projects.create_project(db, req) |> should.be_ok()

    proj.name |> should.equal("Test Project")
    proj.description |> should.equal(Some("A test project"))
    proj.status |> should.equal(Active)
    proj.github_repo_url |> should.equal(None)
  })
}

pub fn create_project_with_status_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "On Hold Project",
        description: None,
        status: Some(OnHold),
        github_repo_url: None,
      )

    let proj = projects.create_project(db, req) |> should.be_ok()

    proj.status |> should.equal(OnHold)
  })
}

pub fn create_project_with_github_url_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "GitHub Project",
        description: None,
        status: None,
        github_repo_url: Some("https://github.com/user/repo"),
      )

    let proj = projects.create_project(db, req) |> should.be_ok()

    proj.github_repo_url |> should.equal(Some("https://github.com/user/repo"))
  })
}

pub fn list_projects_works_test() {
  test_db.with_db(fn(db) {
    projects.list_projects(db, None)
    |> should.be_ok()
    Nil
  })
}

pub fn list_projects_returns_all_test() {
  test_db.with_db(fn(db) {
    let req1 =
      CreateProjectRequest(
        name: "Project 1",
        description: None,
        status: None,
        github_repo_url: None,
      )
    let req2 =
      CreateProjectRequest(
        name: "Project 2",
        description: None,
        status: None,
        github_repo_url: None,
      )

    let proj1 = projects.create_project(db, req1) |> should.be_ok()
    let proj2 = projects.create_project(db, req2) |> should.be_ok()

    let all = projects.list_projects(db, None) |> should.be_ok()
    // Check that both created projects are in the list (there may be more from parallel tests)
    let ids = list.map(all, fn(p) { p.id })
    list.contains(ids, proj1.id) |> should.be_true()
    list.contains(ids, proj2.id) |> should.be_true()
  })
}

pub fn list_projects_filter_by_status_test() {
  test_db.with_db(fn(db) {
    let req1 =
      CreateProjectRequest(
        name: "Active",
        description: None,
        status: Some(Active),
        github_repo_url: None,
      )
    let req2 =
      CreateProjectRequest(
        name: "On Hold",
        description: None,
        status: Some(OnHold),
        github_repo_url: None,
      )
    let req3 =
      CreateProjectRequest(
        name: "Cancelled",
        description: None,
        status: Some(Cancelled),
        github_repo_url: None,
      )

    let proj1 = projects.create_project(db, req1) |> should.be_ok()
    let proj2 = projects.create_project(db, req2) |> should.be_ok()
    let proj3 = projects.create_project(db, req3) |> should.be_ok()

    // Filter by Active - should include proj1
    let active = projects.list_projects(db, Some(Active)) |> should.be_ok()
    let active_ids = list.map(active, fn(p) { p.id })
    list.contains(active_ids, proj1.id) |> should.be_true()
    list.contains(active_ids, proj2.id) |> should.be_false()
    list.contains(active_ids, proj3.id) |> should.be_false()

    // Filter by OnHold - should include proj2
    let on_hold = projects.list_projects(db, Some(OnHold)) |> should.be_ok()
    let on_hold_ids = list.map(on_hold, fn(p) { p.id })
    list.contains(on_hold_ids, proj2.id) |> should.be_true()

    // Filter by Cancelled - should include proj3
    let cancelled =
      projects.list_projects(db, Some(Cancelled)) |> should.be_ok()
    let cancelled_ids = list.map(cancelled, fn(p) { p.id })
    list.contains(cancelled_ids, proj3.id) |> should.be_true()
  })
}

pub fn get_project_not_found_test() {
  test_db.with_db(fn(db) {
    projects.get_project(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_project_found_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test",
        description: None,
        status: None,
        github_repo_url: None,
      )
    let created = projects.create_project(db, req) |> should.be_ok()

    let found =
      projects.get_project(db, created.id)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.name |> should.equal("Test")
  })
}

pub fn update_project_name_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Original",
        description: None,
        status: None,
        github_repo_url: None,
      )
    let proj = projects.create_project(db, req) |> should.be_ok()

    let update_req =
      UpdateProjectRequest(
        name: Some("Updated"),
        description: None,
        status: None,
        github_repo_url: None,
      )

    let updated =
      projects.update_project(db, proj.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.name |> should.equal("Updated")
  })
}

pub fn update_project_description_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test",
        description: None,
        status: None,
        github_repo_url: None,
      )
    let proj = projects.create_project(db, req) |> should.be_ok()
    proj.description |> should.equal(None)

    let update_req =
      UpdateProjectRequest(
        name: None,
        description: Some("New description"),
        status: None,
        github_repo_url: None,
      )

    let updated =
      projects.update_project(db, proj.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.description |> should.equal(Some("New description"))
  })
}

pub fn update_project_status_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test",
        description: None,
        status: Some(Active),
        github_repo_url: None,
      )
    let proj = projects.create_project(db, req) |> should.be_ok()
    proj.status |> should.equal(Active)

    let update_req =
      UpdateProjectRequest(
        name: None,
        description: None,
        status: Some(Cancelled),
        github_repo_url: None,
      )

    let updated =
      projects.update_project(db, proj.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.status |> should.equal(Cancelled)
  })
}

pub fn update_project_not_found_test() {
  test_db.with_db(fn(db) {
    let update_req =
      UpdateProjectRequest(
        name: Some("Updated"),
        description: None,
        status: None,
        github_repo_url: None,
      )

    projects.update_project(
      db,
      "00000000-0000-0000-0000-000000000000",
      update_req,
    )
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_project_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test",
        description: None,
        status: None,
        github_repo_url: None,
      )
    let proj = projects.create_project(db, req) |> should.be_ok()

    projects.delete_project(db, proj.id)
    |> should.be_ok()
    |> should.equal(True)

    projects.get_project(db, proj.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_project_not_found_test() {
  test_db.with_db(fn(db) {
    projects.delete_project(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(False)
  })
}

// =============================================================================
// GitHub Link Tests
// =============================================================================

pub fn link_github_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test",
        description: None,
        status: None,
        github_repo_url: None,
      )
    let proj = projects.create_project(db, req) |> should.be_ok()
    proj.github_repo_url |> should.equal(None)

    let linked =
      projects.link_github(db, proj.id, "https://github.com/user/repo")
      |> should.be_ok()
      |> should.be_some()

    linked.github_repo_url |> should.equal(Some("https://github.com/user/repo"))
  })
}

pub fn link_github_not_found_test() {
  test_db.with_db(fn(db) {
    projects.link_github(
      db,
      "00000000-0000-0000-0000-000000000000",
      "https://github.com/user/repo",
    )
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn unlink_github_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateProjectRequest(
        name: "Test",
        description: None,
        status: None,
        github_repo_url: Some("https://github.com/user/repo"),
      )
    let proj = projects.create_project(db, req) |> should.be_ok()
    proj.github_repo_url |> should.equal(Some("https://github.com/user/repo"))

    let unlinked =
      projects.unlink_github(db, proj.id)
      |> should.be_ok()
      |> should.be_some()

    unlinked.github_repo_url |> should.equal(None)
  })
}

pub fn unlink_github_not_found_test() {
  test_db.with_db(fn(db) {
    projects.unlink_github(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(None)
  })
}
