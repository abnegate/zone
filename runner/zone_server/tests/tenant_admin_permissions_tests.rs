//! A signed-up account can reach the console's tenant settings pages.
//!
//! The console has always gated `/settings` and `/org-settings` on
//! `workspaces:update` and `organizations:update`. Migration 001 seeded a
//! permission row for every other resource the console names and none for these
//! two, so no role could hold either and both pages answered "Access Denied" to
//! every user, including the owner of the organization. Migration 022 seeds them.
//!
//! Which tenant a caller may act on is decided per request from
//! `organization_members` and `workspace_members`; these grants only say that a
//! role administers tenants at all, which is why the refusals asserted below
//! still hold for a foreign tenant.

mod common;

use axum::http::StatusCode;
use common::{TestClient, test_email, test_password};
use serde_json::json;

async fn register(client: &TestClient) -> serde_json::Value {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
                "display_name": "Tenant Owner"
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()
}

#[tokio::test]
async fn a_new_account_may_administer_its_own_tenants() {
    let client = TestClient::with_db().await;

    let body = register(&client).await;
    let granted: Vec<String> = body["permissions"]
        .as_array()
        .expect("registration returns the permission list the console gates on")
        .iter()
        .map(|permission| permission.as_str().unwrap_or_default().to_string())
        .collect();

    for required in [
        "organizations:read",
        "organizations:create",
        "organizations:update",
        "workspaces:read",
        "workspaces:create",
        "workspaces:update",
    ] {
        assert!(
            granted.iter().any(|permission| permission == required),
            "{required} is missing, so the console refuses the settings page: {granted:?}"
        );
    }
}

#[tokio::test]
async fn deleting_a_tenant_is_not_granted_to_a_standard_account() {
    let client = TestClient::with_db().await;

    let body = register(&client).await;
    let granted: Vec<String> = body["permissions"]
        .as_array()
        .expect("registration returns a permission list")
        .iter()
        .map(|permission| permission.as_str().unwrap_or_default().to_string())
        .collect();

    for withheld in ["organizations:delete", "workspaces:delete"] {
        assert!(
            !granted.iter().any(|permission| permission == withheld),
            "{withheld} was granted to a standard account: {granted:?}"
        );
    }
}
