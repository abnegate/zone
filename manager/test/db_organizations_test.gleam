import database/queries/organizations
import gleam/list
import gleam/option.{None, Some}
import gleeunit/should
import models/organization.{CreateOrganizationRequest, UpdateOrganizationRequest}

import test_db

// =============================================================================
// Organization CRUD Tests
// =============================================================================

pub fn create_organization_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Test Organization",
        slug: "test-org",
        description: Some("A test organization"),
      )

    let org = organizations.create_organization(db, req) |> should.be_ok()

    org.name |> should.equal("Test Organization")
    org.slug |> should.equal("test-org")
    org.description |> should.equal(Some("A test organization"))
    org.is_active |> should.equal(True)
  })
}

pub fn create_organization_without_description_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Minimal Org",
        slug: "minimal-org",
        description: None,
      )

    let org = organizations.create_organization(db, req) |> should.be_ok()

    org.name |> should.equal("Minimal Org")
    org.slug |> should.equal("minimal-org")
    org.description |> should.equal(None)
  })
}

pub fn list_organizations_empty_test() {
  test_db.with_db(fn(db) {
    organizations.list_organizations(db, False)
    |> should.be_ok()
    |> should.equal([])
  })
}

pub fn list_organizations_returns_all_test() {
  test_db.with_db(fn(db) {
    let req1 =
      CreateOrganizationRequest(name: "Org 1", slug: "org-1", description: None)
    let req2 =
      CreateOrganizationRequest(name: "Org 2", slug: "org-2", description: None)

    let _ = organizations.create_organization(db, req1) |> should.be_ok()
    let _ = organizations.create_organization(db, req2) |> should.be_ok()

    let all = organizations.list_organizations(db, False) |> should.be_ok()
    list.length(all) |> should.equal(2)
  })
}

pub fn list_organizations_filter_active_only_test() {
  test_db.with_db(fn(db) {
    let req1 =
      CreateOrganizationRequest(
        name: "Active Org",
        slug: "active-org",
        description: None,
      )
    let req2 =
      CreateOrganizationRequest(
        name: "Inactive Org",
        slug: "inactive-org",
        description: None,
      )

    let _ = organizations.create_organization(db, req1) |> should.be_ok()
    let org2 = organizations.create_organization(db, req2) |> should.be_ok()

    // Deactivate org2
    let update_req =
      UpdateOrganizationRequest(
        name: None,
        slug: None,
        description: None,
        is_active: Some(False),
      )
    let _ =
      organizations.update_organization(db, org2.id, update_req)
      |> should.be_ok()

    // All orgs
    let all = organizations.list_organizations(db, False) |> should.be_ok()
    list.length(all) |> should.equal(2)

    // Active only
    let active = organizations.list_organizations(db, True) |> should.be_ok()
    list.length(active) |> should.equal(1)
    let assert [org] = active
    org.name |> should.equal("Active Org")
  })
}

pub fn get_organization_not_found_test() {
  test_db.with_db(fn(db) {
    organizations.get_organization(db, "nonexistent-id")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_organization_found_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Test Org",
        slug: "test-org",
        description: None,
      )
    let created = organizations.create_organization(db, req) |> should.be_ok()

    let found =
      organizations.get_organization(db, created.id)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.name |> should.equal("Test Org")
    found.slug |> should.equal("test-org")
  })
}

pub fn get_organization_by_slug_not_found_test() {
  test_db.with_db(fn(db) {
    organizations.get_organization_by_slug(db, "nonexistent-slug")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_organization_by_slug_found_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Slug Test",
        slug: "my-unique-slug",
        description: None,
      )
    let created = organizations.create_organization(db, req) |> should.be_ok()

    let found =
      organizations.get_organization_by_slug(db, "my-unique-slug")
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.slug |> should.equal("my-unique-slug")
  })
}

pub fn update_organization_name_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Original",
        slug: "original-org",
        description: None,
      )
    let org = organizations.create_organization(db, req) |> should.be_ok()

    let update_req =
      UpdateOrganizationRequest(
        name: Some("Updated"),
        slug: None,
        description: None,
        is_active: None,
      )

    let updated =
      organizations.update_organization(db, org.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.name |> should.equal("Updated")
    updated.slug |> should.equal("original-org")
  })
}

pub fn update_organization_slug_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Test",
        slug: "old-slug",
        description: None,
      )
    let org = organizations.create_organization(db, req) |> should.be_ok()

    let update_req =
      UpdateOrganizationRequest(
        name: None,
        slug: Some("new-slug"),
        description: None,
        is_active: None,
      )

    let updated =
      organizations.update_organization(db, org.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.slug |> should.equal("new-slug")
  })
}

pub fn update_organization_description_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Test",
        slug: "test-org",
        description: None,
      )
    let org = organizations.create_organization(db, req) |> should.be_ok()
    org.description |> should.equal(None)

    let update_req =
      UpdateOrganizationRequest(
        name: None,
        slug: None,
        description: Some("New description"),
        is_active: None,
      )

    let updated =
      organizations.update_organization(db, org.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.description |> should.equal(Some("New description"))
  })
}

pub fn update_organization_is_active_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Test",
        slug: "test-org",
        description: None,
      )
    let org = organizations.create_organization(db, req) |> should.be_ok()
    org.is_active |> should.equal(True)

    let update_req =
      UpdateOrganizationRequest(
        name: None,
        slug: None,
        description: None,
        is_active: Some(False),
      )

    let updated =
      organizations.update_organization(db, org.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.is_active |> should.equal(False)
  })
}

pub fn update_organization_not_found_test() {
  test_db.with_db(fn(db) {
    let update_req =
      UpdateOrganizationRequest(
        name: Some("Updated"),
        slug: None,
        description: None,
        is_active: None,
      )

    organizations.update_organization(db, "nonexistent-id", update_req)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_organization_test() {
  test_db.with_db(fn(db) {
    let req =
      CreateOrganizationRequest(
        name: "Test",
        slug: "test-org",
        description: None,
      )
    let org = organizations.create_organization(db, req) |> should.be_ok()

    organizations.delete_organization(db, org.id)
    |> should.be_ok()
    |> should.equal(True)

    organizations.get_organization(db, org.id)
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_organization_not_found_test() {
  test_db.with_db(fn(db) {
    organizations.delete_organization(db, "nonexistent-id")
    |> should.be_ok()
    |> should.equal(False)
  })
}

// =============================================================================
// Constraint Tests
// =============================================================================

pub fn unique_slug_constraint_test() {
  test_db.with_db(fn(db) {
    let req1 =
      CreateOrganizationRequest(
        name: "Org 1",
        slug: "same-slug",
        description: None,
      )
    let req2 =
      CreateOrganizationRequest(
        name: "Org 2",
        slug: "same-slug",
        description: None,
      )

    let _ = organizations.create_organization(db, req1) |> should.be_ok()

    // Second org with same slug should fail
    let _ =
      organizations.create_organization(db, req2)
      |> should.be_error()
    Nil
  })
}
