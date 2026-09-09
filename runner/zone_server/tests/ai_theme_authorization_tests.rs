//! Regression coverage for AI settings and workspace theme authorization.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions, postgres::PgQueryResult};
use uuid::Uuid;
use zone_server::db::{organization_members, workspace_members};

use common::{
    TestClient, create_test_pool, create_test_router, create_test_state, test_config, test_password,
};

struct Tenant {
    organization: Uuid,
    token: String,
    user: Uuid,
    workspace: Uuid,
}

async fn tenant(client: &TestClient, pool: &PgPool) -> Tenant {
    let email = format!("ai-theme-auth-{}@example.com", Uuid::new_v4());
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": email, "password": test_password() }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let token = response.json_value()["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let user = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_one(pool)
        .await
        .expect("registered user exists");

    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "AI authorization tenant", "slug": Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization is created")
        .parse()
        .expect("organization id is a UUID");

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "AI authorization workspace", "slug": Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let workspace = response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace is created")
        .parse()
        .expect("workspace id is a UUID");

    Tenant {
        organization,
        token,
        user,
        workspace,
    }
}

fn client(pool: PgPool) -> TestClient {
    TestClient::new(create_test_router(create_test_state(test_config(), pool)))
}

fn database() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/zone_test".to_string())
}

async fn pool(database: &str, connections: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(connections)
        .connect(database)
        .await
        .expect("test database is available")
}

async fn wait_until_blocked(pool: &PgPool, application: &str, relation: &str) {
    for _ in 0..100 {
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name = $1 AND wait_event_type = 'Lock' AND query LIKE '%' || $2 || '%')",
        )
        .bind(application)
        .bind(relation)
        .fetch_one(pool)
        .await
        .expect("blocked query can be observed");
        if blocked {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("mutation did not reach the locked {relation} row");
}

async fn try_revoke(
    pool: &PgPool,
    statement: &'static str,
    scope: Uuid,
    user: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    let mut transaction = pool.begin().await.expect("revocation starts");
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *transaction)
        .await
        .expect("lock timeout is set");
    let result = sqlx::query(statement)
        .bind(scope)
        .bind(user)
        .execute(&mut *transaction)
        .await;
    transaction.rollback().await.expect("revocation rolls back");
    result
}

fn assert_lock_timeout(result: Result<PgQueryResult, sqlx::Error>) {
    let error = result.expect_err("membership revocation must wait for the authorized write");
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()),
        Some("55P03".into()),
        "revocation should time out on the membership lock: {error}"
    );
}

#[tokio::test]
async fn nested_ai_routes_reject_an_organization_workspace_mismatch() {
    let pool = create_test_pool().await;
    let client = client(pool.clone());
    let attacker = tenant(&client, &pool).await;
    let victim = tenant(&client, &pool).await;

    client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                victim.organization, victim.workspace
            ),
            &json!({ "provider": "self_hosted", "litellm_host": "https://victim.example.invalid" }),
            &victim.token,
        )
        .await
        .assert_status(StatusCode::OK);

    workspace_members::add_member(
        &pool,
        victim.workspace,
        attacker.user,
        workspace_members::WorkspaceRole::Member,
        Some(victim.user),
    )
    .await
    .expect("attacker is a member of the victim workspace");

    let path = format!(
        "/api/organizations/{}/workspaces/{}/settings/ai",
        attacker.organization, victim.workspace
    );
    let read = client.get_auth(&path, &attacker.token).await;
    let effective = client
        .get_auth(&format!("{path}/effective"), &attacker.token)
        .await;
    let write = client
        .put_json_auth(
            &path,
            &json!({ "litellm_host": "https://attacker.example.invalid" }),
            &attacker.token,
        )
        .await;
    let delete = client.delete_auth(&path, &attacker.token).await;

    read.assert_status(StatusCode::NOT_FOUND);
    effective.assert_status(StatusCode::NOT_FOUND);
    write.assert_status(StatusCode::NOT_FOUND);
    delete.assert_status(StatusCode::NOT_FOUND);

    let settings = client
        .get_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                victim.organization, victim.workspace
            ),
            &victim.token,
        )
        .await;
    settings.assert_status(StatusCode::OK);
    assert_eq!(
        settings.json_value()["litellm_host"],
        "https://victim.example.invalid"
    );
}

#[tokio::test]
async fn nested_ai_writes_require_a_workspace_writer() {
    let pool = create_test_pool().await;
    let client = client(pool.clone());
    let owner = tenant(&client, &pool).await;
    let viewer = tenant(&client, &pool).await;

    organization_members::add_member(
        &pool,
        owner.organization,
        viewer.user,
        organization_members::OrgRole::Admin,
        Some(owner.user),
    )
    .await
    .expect("viewer is an organization admin");
    workspace_members::add_member(
        &pool,
        owner.workspace,
        viewer.user,
        workspace_members::WorkspaceRole::Viewer,
        Some(owner.user),
    )
    .await
    .expect("viewer has read-only workspace access");

    let path = format!(
        "/api/organizations/{}/workspaces/{}/settings/ai",
        owner.organization, owner.workspace
    );
    client
        .get_auth(&path, &viewer.token)
        .await
        .assert_status(StatusCode::OK);
    client
        .put_json_auth(&path, &json!({ "provider": "openai" }), &viewer.token)
        .await
        .assert_status(StatusCode::FORBIDDEN);
    client
        .delete_auth(&path, &viewer.token)
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_workspace_admin_writes_without_being_an_organization_admin() {
    let pool = create_test_pool().await;
    let client = client(pool.clone());
    let owner = tenant(&client, &pool).await;
    let lead = tenant(&client, &pool).await;

    organization_members::add_member(
        &pool,
        owner.organization,
        lead.user,
        organization_members::OrgRole::Member,
        Some(owner.user),
    )
    .await
    .expect("lead is an ordinary organization member");
    workspace_members::add_member(
        &pool,
        owner.workspace,
        lead.user,
        workspace_members::WorkspaceRole::Admin,
        Some(owner.user),
    )
    .await
    .expect("lead administers the workspace");

    let path = format!(
        "/api/organizations/{}/workspaces/{}/settings/ai",
        owner.organization, owner.workspace
    );
    client
        .put_json_auth(&path, &json!({ "provider": "openai" }), &lead.token)
        .await
        .assert_status(StatusCode::OK);
    client
        .delete_auth(&path, &lead.token)
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn ai_write_holds_both_memberships_until_the_mutation_finishes() {
    let database = database();
    let server_pool = pool(&database, 1).await;
    let monitor_pool = pool(&database, 3).await;
    let client = client(server_pool.clone());
    let owner = tenant(&client, &monitor_pool).await;

    sqlx::query(
        "INSERT INTO workspace_ai_settings (workspace_id, provider) VALUES ($1, 'self_hosted')",
    )
    .bind(owner.workspace)
    .execute(&monitor_pool)
    .await
    .expect("AI settings fixture is created");

    let application = format!("zone-ai-race-{}", Uuid::new_v4().simple());
    sqlx::query_scalar::<_, String>("SELECT set_config('application_name', $1, false)")
        .bind(&application)
        .fetch_one(&server_pool)
        .await
        .expect("server connection is named");

    let mut blocker = monitor_pool.begin().await.expect("blocker starts");
    sqlx::query("SELECT id FROM workspace_ai_settings WHERE workspace_id = $1 FOR UPDATE")
        .bind(owner.workspace)
        .execute(&mut *blocker)
        .await
        .expect("AI settings row is locked");

    let organization = owner.organization;
    let workspace = owner.workspace;
    let user = owner.user;
    let request = tokio::spawn(async move {
        client
            .put_json_auth(
                &format!("/api/organizations/{organization}/workspaces/{workspace}/settings/ai"),
                &json!({ "litellm_host": "https://authorized.example.invalid" }),
                &owner.token,
            )
            .await
    });

    wait_until_blocked(&monitor_pool, &application, "workspace_ai_settings").await;
    assert_lock_timeout(
        try_revoke(
            &monitor_pool,
            "UPDATE organization_members SET is_active = FALSE WHERE organization_id = $1 AND user_id = $2",
            organization,
            user,
        )
        .await,
    );
    assert_lock_timeout(
        try_revoke(
            &monitor_pool,
            "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
            workspace,
            user,
        )
        .await,
    );

    blocker
        .rollback()
        .await
        .expect("AI settings row is released");
    request
        .await
        .expect("AI settings request completes")
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn theme_write_holds_membership_until_the_mutation_finishes() {
    let database = database();
    let server_pool = pool(&database, 1).await;
    let monitor_pool = pool(&database, 3).await;
    let client = client(server_pool.clone());
    let owner = tenant(&client, &monitor_pool).await;

    sqlx::query(
        "INSERT INTO workspace_themes (workspace_id, primary_color_light) VALUES ($1, '#111111')",
    )
    .bind(owner.workspace)
    .execute(&monitor_pool)
    .await
    .expect("theme fixture is created");

    let application = format!("zone-theme-race-{}", Uuid::new_v4().simple());
    sqlx::query_scalar::<_, String>("SELECT set_config('application_name', $1, false)")
        .bind(&application)
        .fetch_one(&server_pool)
        .await
        .expect("server connection is named");

    let mut blocker = monitor_pool.begin().await.expect("blocker starts");
    sqlx::query("SELECT id FROM workspace_themes WHERE workspace_id = $1 FOR UPDATE")
        .bind(owner.workspace)
        .execute(&mut *blocker)
        .await
        .expect("theme row is locked");

    let workspace = owner.workspace;
    let user = owner.user;
    let request = tokio::spawn(async move {
        client
            .put_json_auth(
                &format!("/api/workspaces/{workspace}/theme"),
                &json!({ "primary_color_light": "#222222" }),
                &owner.token,
            )
            .await
    });

    wait_until_blocked(&monitor_pool, &application, "workspace_themes").await;
    let revocation = try_revoke(
        &monitor_pool,
        "UPDATE workspace_members SET is_active = FALSE WHERE workspace_id = $1 AND user_id = $2",
        workspace,
        user,
    )
    .await;
    blocker.rollback().await.expect("theme row is released");
    request
        .await
        .expect("theme request completes")
        .assert_status(StatusCode::OK);

    assert_lock_timeout(revocation);
}
