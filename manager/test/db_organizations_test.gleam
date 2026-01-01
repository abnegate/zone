import database/queries/organizations
import gleam/list
import gleam/option.{None, Some}
import gleeunit/should
import models/organization.{CreateOrganizationRequest, UpdateOrganizationRequest}

import test_db

// Helper to create an organization with unique slug
fn create_org(db, name: String) {
  let slug = test_db.unique_slug("org")
  let req = CreateOrganizationRequest(name: name, slug: slug, description: None)
  organizations.create_organization(db, req) |> should.be_ok()
}

// =============================================================================
// Organization CRUD Tests
// =============================================================================

pub fn create_organization_test() {
  test_db.with_db(fn(db) {
    let slug = test_db.unique_slug("test-org")
    let req =
      CreateOrganizationRequest(
        name: "Test Organization",
        slug: slug,
        description: Some("A test organization"),
      )

    let org = organizations.create_organization(db, req) |> should.be_ok()

    org.name |> should.equal("Test Organization")
    org.slug |> should.equal(slug)
    org.description |> should.equal(Some("A test organization"))
    org.is_active |> should.equal(True)
  })
}

pub fn create_organization_without_description_test() {
  test_db.with_db(fn(db) {
    let slug = test_db.unique_slug("minimal-org")
    let req =
      CreateOrganizationRequest(
        name: "Minimal Org",
        slug: slug,
        description: None,
      )

    let org = organizations.create_organization(db, req) |> should.be_ok()

    org.name |> should.equal("Minimal Org")
    org.slug |> should.equal(slug)
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
    let org1 = create_org(db, "Org 1")
    let org2 = create_org(db, "Org 2")

    let all = organizations.list_organizations(db, False) |> should.be_ok()
    // Check that both created orgs are in the list (there may be more from parallel tests)
    let ids = list.map(all, fn(o) { o.id })
    list.contains(ids, org1.id) |> should.be_true()
    list.contains(ids, org2.id) |> should.be_true()
  })
}

pub fn list_organizations_filter_active_only_test() {
  test_db.with_db(fn(db) {
    let org1 = create_org(db, "Active Org")
    let org2 = create_org(db, "Inactive Org")

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

    // All orgs should include both
    let all = organizations.list_organizations(db, False) |> should.be_ok()
    let all_ids = list.map(all, fn(o) { o.id })
    list.contains(all_ids, org1.id) |> should.be_true()
    list.contains(all_ids, org2.id) |> should.be_true()

    // Active only should include org1 but not org2
    let active = organizations.list_organizations(db, True) |> should.be_ok()
    let active_ids = list.map(active, fn(o) { o.id })
    list.contains(active_ids, org1.id) |> should.be_true()
    list.contains(active_ids, org2.id) |> should.be_false()
  })
}

pub fn get_organization_not_found_test() {
  test_db.with_db(fn(db) {
    organizations.get_organization(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn get_organization_found_test() {
  test_db.with_db(fn(db) {
    let created = create_org(db, "Test Org")

    let found =
      organizations.get_organization(db, created.id)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.name |> should.equal("Test Org")
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
    let slug = test_db.unique_slug("slug-test")
    let req =
      CreateOrganizationRequest(
        name: "Slug Test",
        slug: slug,
        description: None,
      )
    let created = organizations.create_organization(db, req) |> should.be_ok()

    let found =
      organizations.get_organization_by_slug(db, slug)
      |> should.be_ok()
      |> should.be_some()

    found.id |> should.equal(created.id)
    found.slug |> should.equal(slug)
  })
}

pub fn update_organization_name_test() {
  test_db.with_db(fn(db) {
    let org = create_org(db, "Original")

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
    updated.slug |> should.equal(org.slug)
  })
}

pub fn update_organization_slug_test() {
  test_db.with_db(fn(db) {
    let org = create_org(db, "Test")
    let new_slug = test_db.unique_slug("new-slug")

    let update_req =
      UpdateOrganizationRequest(
        name: None,
        slug: Some(new_slug),
        description: None,
        is_active: None,
      )

    let updated =
      organizations.update_organization(db, org.id, update_req)
      |> should.be_ok()
      |> should.be_some()

    updated.slug |> should.equal(new_slug)
  })
}

pub fn update_organization_description_test() {
  test_db.with_db(fn(db) {
    let org = create_org(db, "Test")
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
    let org = create_org(db, "Test")
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

    organizations.update_organization(
      db,
      "00000000-0000-0000-0000-000000000000",
      update_req,
    )
    |> should.be_ok()
    |> should.equal(None)
  })
}

pub fn delete_organization_test() {
  test_db.with_db(fn(db) {
    let org = create_org(db, "Test")

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
    organizations.delete_organization(db, "00000000-0000-0000-0000-000000000000")
    |> should.be_ok()
    |> should.equal(False)
  })
}

// =============================================================================
// Constraint Tests
// =============================================================================

pub fn unique_slug_constraint_test() {
  test_db.with_db(fn(db) {
    let same_slug = test_db.unique_slug("same-slug")
    let req1 =
      CreateOrganizationRequest(
        name: "Org 1",
        slug: same_slug,
        description: None,
      )
    let req2 =
      CreateOrganizationRequest(
        name: "Org 2",
        slug: same_slug,
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
