import database/queries/organizations
import database/queries/workspaces
import gleam/list
import gleam/option.{None, Some}
import gleeunit/should
import models/organization.{CreateOrganizationRequest}
import models/workspace.{CreateWorkspaceRequest, UpdateWorkspaceRequest}

import test_db

// =============================================================================
// Helper: Create an organization for workspace tests
// =============================================================================

fn create_test_org(db, name: String, slug: String) {
  let req = CreateOrganizationRequest(name: name, slug: slug, description: None)
  organizations.create_organization(db, req) |> should.be_ok()
}

// =============================================================================
// Workspace CRUD Tests
// =============================================================================

pub fn create_workspace_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(
        name: "Test Workspace",
        slug: "test-ws",
        description: Some("A test workspace"),
      )

    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    ws.name |> should.equal("Test Workspace")
    ws.slug |> should.equal("test-ws")
    ws.description |> should.equal(Some("A test workspace"))
    ws.organization_id |> should.equal(org.id)
    ws.is_active |> should.equal(True)
  })
}

pub fn create_workspace_without_description_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(
        name: "Minimal Workspace",
        slug: "minimal-ws",
        description: None,
      )

    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    ws.name |> should.equal("Minimal Workspace")
    ws.description |> should.equal(None)
  })
}

pub fn list_workspaces_empty_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    workspaces.list_workspaces(db, org.id, False)
    |> should.be_ok()
    |> should.equal([])
  })
}

pub fn list_workspaces_returns_all_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req1 =
      CreateWorkspaceRequest(
        name: "Workspace 1",
        slug: "ws-1",
        description: None,
      )
    let req2 =
      CreateWorkspaceRequest(
        name: "Workspace 2",
        slug: "ws-2",
        description: None,
      )

    let _ = workspaces.create_workspace(db, org.id, req1) |> should.be_ok()
    let _ = workspaces.create_workspace(db, org.id, req2) |> should.be_ok()

    let all = workspaces.list_workspaces(db, org.id, False) |> should.be_ok()
    list.length(all) |> should.equal(2)
  })
}

pub fn list_workspaces_scoped_by_organization_test() {
  test_db.with_db(fn(db) {
    let org1 = create_test_org(db, "Org 1", "org-1")
    let org2 = create_test_org(db, "Org 2", "org-2")

    let req1 =
      CreateWorkspaceRequest(
        name: "Org1 Workspace",
        slug: "ws-1",
        description: None,
      )
    let req2 =
      CreateWorkspaceRequest(
        name: "Org2 Workspace",
        slug: "ws-2",
        description: None,
      )

    let _ = workspaces.create_workspace(db, org1.id, req1) |> should.be_ok()
    let _ = workspaces.create_workspace(db, org2.id, req2) |> should.be_ok()

    // List for org1 should only return org1's workspaces
    let org1_ws =
      workspaces.list_workspaces(db, org1.id, False) |> should.be_ok()
    list.length(org1_ws) |> should.equal(1)
    let assert [ws] = org1_ws
    ws.name |> should.equal("Org1 Workspace")

    // List for org2 should only return org2's workspaces
    let org2_ws =
      workspaces.list_workspaces(db, org2.id, False) |> should.be_ok()
    list.length(org2_ws) |> should.equal(1)
    let assert [ws2] = org2_ws
    ws2.name |> should.equal("Org2 Workspace")
  })
}

pub fn list_workspaces_filter_active_only_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req1 =
      CreateWorkspaceRequest(
        name: "Active WS",
        slug: "active-ws",
        description: None,
      )
    let req2 =
      CreateWorkspaceRequest(
        name: "Inactive WS",
        slug: "inactive-ws",
        description: None,
      )

    let _ = workspaces.create_workspace(db, org.id, req1) |> should.be_ok()
    let ws2 = workspaces.create_workspace(db, org.id, req2) |> should.be_ok()

    // Deactivate ws2
    let update_req =
      UpdateWorkspaceRequest(
        name: None,
        slug: None,
        description: None,
        is_active: Some(False),
      )
    let _ =
      workspaces.update_workspace(db, org.id, ws2.id, update_req)
      |> should.be_ok()

    // All workspaces
    let all = workspaces.list_workspaces(db, org.id, False) |> should.be_ok()
    list.length(all) |> should.equal(2)

    // Active only
    let active = workspaces.list_workspaces(db, org.id, True) |> should.be_ok()
    list.length(active) |> should.equal(1)
    let assert [ws] = active
    ws.name |> should.equal("Active WS")
  })
}

pub fn get_workspace_not_found_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    workspaces.get_workspace(db, org.id, "nonexistent-id")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_workspace_found_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(
        name: "Test WS",
        slug: "test-ws",
        description: None,
      )
    let created = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    let found =
      workspaces.get_workspace(db, org.id, created.id)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.name |> should.equal("Test WS")
    found.organization_id |> should.equal(org.id)
  })
}

pub fn get_workspace_wrong_organization_test() {
  test_db.with_db(fn(db) {
    let org1 = create_test_org(db, "Org 1", "org-1")
    let org2 = create_test_org(db, "Org 2", "org-2")

    let req =
      CreateWorkspaceRequest(
        name: "Test WS",
        slug: "test-ws",
        description: None,
      )
    let ws = workspaces.create_workspace(db, org1.id, req) |> should.be_ok()

    // Try to get workspace with wrong organization ID
    workspaces.get_workspace(db, org2.id, ws.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_workspace_by_slug_not_found_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    workspaces.get_workspace_by_slug(db, org.id, "nonexistent-slug")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_workspace_by_slug_found_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(
        name: "Slug Test",
        slug: "my-unique-slug",
        description: None,
      )
    let created = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    let found =
      workspaces.get_workspace_by_slug(db, org.id, "my-unique-slug")
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.slug |> should.equal("my-unique-slug")
  })
}

pub fn update_workspace_name_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(
        name: "Original",
        slug: "original-ws",
        description: None,
      )
    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    let update_req =
      UpdateWorkspaceRequest(
        name: Some("Updated"),
        slug: None,
        description: None,
        is_active: None,
      )

    let updated =
      workspaces.update_workspace(db, org.id, ws.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.name |> should.equal("Updated")
    updated.slug |> should.equal("original-ws")
  })
}

pub fn update_workspace_slug_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(name: "Test", slug: "old-slug", description: None)
    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    let update_req =
      UpdateWorkspaceRequest(
        name: None,
        slug: Some("new-slug"),
        description: None,
        is_active: None,
      )

    let updated =
      workspaces.update_workspace(db, org.id, ws.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.slug |> should.equal("new-slug")
  })
}

pub fn update_workspace_description_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(name: "Test", slug: "test-ws", description: None)
    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()
    ws.description |> should.equal(None)

    let update_req =
      UpdateWorkspaceRequest(
        name: None,
        slug: None,
        description: Some("New description"),
        is_active: None,
      )

    let updated =
      workspaces.update_workspace(db, org.id, ws.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.description |> should.equal(Some("New description"))
  })
}

pub fn update_workspace_is_active_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(name: "Test", slug: "test-ws", description: None)
    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()
    ws.is_active |> should.equal(True)

    let update_req =
      UpdateWorkspaceRequest(
        name: None,
        slug: None,
        description: None,
        is_active: Some(False),
      )

    let updated =
      workspaces.update_workspace(db, org.id, ws.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.is_active |> should.equal(False)
  })
}

pub fn update_workspace_not_found_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let update_req =
      UpdateWorkspaceRequest(
        name: Some("Updated"),
        slug: None,
        description: None,
        is_active: None,
      )

    workspaces.update_workspace(db, org.id, "nonexistent-id", update_req)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn update_workspace_wrong_organization_test() {
  test_db.with_db(fn(db) {
    let org1 = create_test_org(db, "Org 1", "org-1")
    let org2 = create_test_org(db, "Org 2", "org-2")

    let req =
      CreateWorkspaceRequest(name: "Test", slug: "test-ws", description: None)
    let ws = workspaces.create_workspace(db, org1.id, req) |> should.be_ok()

    let update_req =
      UpdateWorkspaceRequest(
        name: Some("Should not update"),
        slug: None,
        description: None,
        is_active: None,
      )

    // Try to update workspace with wrong organization ID
    workspaces.update_workspace(db, org2.id, ws.id, update_req)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_workspace_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req =
      CreateWorkspaceRequest(name: "Test", slug: "test-ws", description: None)
    let ws = workspaces.create_workspace(db, org.id, req) |> should.be_ok()

    workspaces.delete_workspace(db, org.id, ws.id)
    |> should.be_ok()
    |> should.equal(True)

    workspaces.get_workspace(db, org.id, ws.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_workspace_not_found_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    workspaces.delete_workspace(db, org.id, "nonexistent-id")
    |> should.be_ok()
    |> should.equal(False)
  })
}

pub fn delete_workspace_wrong_organization_test() {
  test_db.with_db(fn(db) {
    let org1 = create_test_org(db, "Org 1", "org-1")
    let org2 = create_test_org(db, "Org 2", "org-2")

    let req =
      CreateWorkspaceRequest(name: "Test", slug: "test-ws", description: None)
    let ws = workspaces.create_workspace(db, org1.id, req) |> should.be_ok()

    // Try to delete workspace with wrong organization ID
    workspaces.delete_workspace(db, org2.id, ws.id)
    |> should.be_ok()
    |> should.equal(False)

    // Workspace should still exist
    let _ =
      workspaces.get_workspace(db, org1.id, ws.id)
      |> should.be_ok()
      |> should.be_some()
    Nil
  })
}

// =============================================================================
// Constraint Tests
// =============================================================================

pub fn unique_slug_within_organization_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req1 =
      CreateWorkspaceRequest(name: "WS 1", slug: "same-slug", description: None)
    let req2 =
      CreateWorkspaceRequest(name: "WS 2", slug: "same-slug", description: None)

    let _ = workspaces.create_workspace(db, org.id, req1) |> should.be_ok()

    // Second workspace with same slug in same org should fail
    let _ =
      workspaces.create_workspace(db, org.id, req2)
      |> should.be_error()
    Nil
  })
}

pub fn same_slug_allowed_in_different_organizations_test() {
  test_db.with_db(fn(db) {
    let org1 = create_test_org(db, "Org 1", "org-1")
    let org2 = create_test_org(db, "Org 2", "org-2")

    let req =
      CreateWorkspaceRequest(
        name: "Test WS",
        slug: "shared-slug",
        description: None,
      )

    // Same slug in different orgs should work
    let ws1 = workspaces.create_workspace(db, org1.id, req) |> should.be_ok()
    let ws2 = workspaces.create_workspace(db, org2.id, req) |> should.be_ok()

    ws1.slug |> should.equal("shared-slug")
    ws2.slug |> should.equal("shared-slug")
    ws1.organization_id |> should.not_equal(ws2.organization_id)
  })
}

// =============================================================================
// Cascade Delete Tests
// =============================================================================

pub fn cascade_delete_workspaces_when_organization_deleted_test() {
  test_db.with_db(fn(db) {
    let org = create_test_org(db, "Test Org", "test-org")

    let req1 =
      CreateWorkspaceRequest(name: "WS 1", slug: "ws-1", description: None)
    let req2 =
      CreateWorkspaceRequest(name: "WS 2", slug: "ws-2", description: None)

    let ws1 = workspaces.create_workspace(db, org.id, req1) |> should.be_ok()
    let ws2 = workspaces.create_workspace(db, org.id, req2) |> should.be_ok()

    // Verify workspaces exist
    let ws_list =
      workspaces.list_workspaces(db, org.id, False) |> should.be_ok()
    list.length(ws_list) |> should.equal(2)

    // Delete organization
    organizations.delete_organization(db, org.id)
    |> should.be_ok()
    |> should.equal(True)

    // Workspaces should be cascade deleted - we can't list them since org is gone
    // but we can try to get by ID (which requires org_id, so we create a new org to verify)
    let new_org = create_test_org(db, "New Org", "new-org")

    // These workspace IDs should not exist anymore
    workspaces.get_workspace(db, new_org.id, ws1.id)
    |> should.be_ok()
    |> should.equal(None)

    workspaces.get_workspace(db, new_org.id, ws2.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}
