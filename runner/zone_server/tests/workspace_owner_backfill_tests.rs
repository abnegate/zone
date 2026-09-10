//! Workspaces created before the creator was enrolled as owner have no owner
//! at all, and "only owners can remove admins or owners" leaves nobody able to
//! remove or demote an admin in them. The backfill seats one.

mod common;

use std::path::Path;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::{AssertSqlSafe, PgPool, raw_sql};

use common::{TestClient, create_test_pool, test_email, test_password};

fn backfill() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("migrations")
            .join("023_workspace_owner_backfill.sql"),
    )
    .expect("the backfill migration is readable")
}

async fn apply(pool: &PgPool) {
    raw_sql(AssertSqlSafe(backfill()))
        .execute(pool)
        .await
        .expect("the backfill applies");
}

struct Account {
    token: String,
    user: String,
}

async fn account(client: &TestClient) -> Account {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let body = response.json_value();
    Account {
        token: body["access_token"].as_str().expect("token").to_string(),
        user: body["user"]["id"].as_str().expect("user").to_string(),
    }
}

async fn tenant(client: &TestClient, token: &str) -> (String, String) {
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": uuid::Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization")
        .to_string();
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Workspace", "slug": uuid::Uuid::new_v4().to_string() }),
            token,
        )
        .await;
    let workspace = response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace")
        .to_string();
    (organization, workspace)
}

async fn role(pool: &PgPool, workspace: &str, user: &str) -> String {
    sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1::uuid AND user_id = $2::uuid",
    )
    .bind(workspace)
    .bind(user)
    .fetch_one(pool)
    .await
    .expect("the member is still there")
}

/// Recreate what the old enrolment left behind: the creator holding admin, and
/// no owner anywhere in the workspace.
async fn strip_owner(pool: &PgPool, workspace: &str) {
    sqlx::query(
        "UPDATE workspace_members SET role = 'admin' WHERE workspace_id = $1::uuid AND role = 'owner'",
    )
    .bind(workspace)
    .execute(pool)
    .await
    .expect("the legacy shape is seeded");
}

#[tokio::test]
async fn an_ownerless_workspace_gets_its_creator_back() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let creator = account(&client).await;
    let (_organization, workspace) = tenant(&client, &creator.token).await;

    strip_owner(&pool, &workspace).await;
    assert_eq!(role(&pool, &workspace, &creator.user).await, "admin");

    apply(&pool).await;

    assert_eq!(
        role(&pool, &workspace, &creator.user).await,
        "owner",
        "the creator was not seated"
    );
}

#[tokio::test]
async fn a_workspace_that_still_has_an_owner_is_left_alone() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let creator = account(&client).await;
    let deputy = account(&client).await;
    let (organization, workspace) = tenant(&client, &creator.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "member" }),
            &creator.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &creator.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    apply(&pool).await;

    assert_eq!(role(&pool, &workspace, &creator.user).await, "owner");
    assert_eq!(
        role(&pool, &workspace, &deputy.user).await,
        "admin",
        "an admin was promoted in a workspace that already had an owner"
    );
}

/// The creator is preferred over an earlier-joining invitee, so the outcome
/// does not depend on who happens to be first in the table.
#[tokio::test]
async fn the_creator_is_preferred_over_an_invited_admin() {
    let client = TestClient::with_db().await;
    let pool = create_test_pool().await;
    let creator = account(&client).await;
    let deputy = account(&client).await;
    let (organization, workspace) = tenant(&client, &creator.token).await;

    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/members"),
            &json!({ "user_id": deputy.user, "role": "member" }),
            &creator.token,
        )
        .await
        .assert_status(StatusCode::CREATED);
    client
        .post_json_auth(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": deputy.user, "role": "admin" }),
            &creator.token,
        )
        .await
        .assert_status(StatusCode::CREATED);

    // Make the invited admin the earlier joiner, so only the creator rule can
    // decide it.
    sqlx::query(
        "UPDATE workspace_members SET created_at = NOW() - INTERVAL '1 day' WHERE workspace_id = $1::uuid AND user_id = $2::uuid",
    )
    .bind(&workspace)
    .bind(&deputy.user)
    .execute(&pool)
    .await
    .unwrap();
    strip_owner(&pool, &workspace).await;

    apply(&pool).await;

    assert_eq!(role(&pool, &workspace, &creator.user).await, "owner");
    assert_eq!(
        role(&pool, &workspace, &deputy.user).await,
        "admin",
        "the backfill seated two owners"
    );
}
