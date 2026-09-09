//! Every route here addresses a resource by an id taken from the request, and
//! the queries behind them are keyed on that id alone. Binding the caller as
//! `_auth: AuthUser` proved a valid token existed and then discarded who it
//! belonged to, so the id was the only credential: any account that could
//! register could read, rewrite, delete and start agent runs in any other
//! tenant's workspace, and could repoint a tenant's model host and provider
//! key at a server of its choosing.
//!
//! These tests are written from the attacker's side. Each one has a victim set
//! something up, then has an unrelated account -- a real, valid, registered
//! user, not an anonymous one -- name the victim's id directly.

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

struct Tenant {
    token: String,
    organization: String,
    workspace: String,
}

async fn tenant(client: &TestClient) -> Tenant {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    let token = response.json_value()["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();

    let slug = uuid::Uuid::new_v4().to_string();
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": slug }),
            &token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization is created")
        .to_string();

    let slug = uuid::Uuid::new_v4().to_string();
    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Tenant workspace", "slug": slug }),
            &token,
        )
        .await;
    let workspace = response.json_value()["workspace"]["id"]
        .as_str()
        .expect("workspace is created")
        .to_string();

    Tenant {
        token,
        organization,
        workspace,
    }
}

async fn task_in(client: &TestClient, owner: &Tenant) -> String {
    let response = client
        .post_json_auth(
            &format!("/api/workspaces/{}/tasks", owner.workspace),
            &json!({ "title": "Victim task", "description": "Private work" }),
            &owner.token,
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    response.json_value()["task"]["id"]
        .as_str()
        .expect("task is created")
        .to_string()
}

fn denied_write(status: StatusCode) -> bool {
    matches!(status, StatusCode::NOT_FOUND | StatusCode::FORBIDDEN)
}

#[tokio::test]
async fn a_stranger_cannot_read_or_change_another_tenants_task() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;
    let task = task_in(&client, &victim).await;

    let listed = client
        .get_auth(
            &format!("/api/workspaces/{}/tasks", victim.workspace),
            &attacker.token,
        )
        .await;
    assert_eq!(
        listed.status,
        StatusCode::NOT_FOUND,
        "listing another tenant's tasks returned {}",
        listed.status
    );

    let read = client
        .get_auth(&format!("/api/tasks/{task}"), &attacker.token)
        .await;
    assert_eq!(
        read.status,
        StatusCode::NOT_FOUND,
        "reading another tenant's task returned {}",
        read.status
    );

    let rewritten = client
        .put_json_auth(
            &format!("/api/tasks/{task}"),
            &json!({ "title": "Rewritten by an attacker" }),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(rewritten.status),
        "rewriting another tenant's task returned {}",
        rewritten.status
    );

    let deleted = client
        .delete_auth(&format!("/api/tasks/{task}"), &attacker.token)
        .await;
    assert!(
        denied_write(deleted.status),
        "deleting another tenant's task returned {}",
        deleted.status
    );

    // The owner still holds what the attacker could not take.
    let owner_read = client
        .get_auth(&format!("/api/tasks/{task}"), &victim.token)
        .await;
    owner_read.assert_status(StatusCode::OK);
    assert_eq!(owner_read.json_value()["task"]["title"], "Victim task");
}

#[tokio::test]
async fn a_stranger_cannot_start_an_agent_run_in_another_tenants_workspace() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;
    let task = task_in(&client, &victim).await;

    // The worst of the set: this spawns the victim's agent pipeline against
    // their repository and their credentials.
    let started = client
        .post_json_auth(
            &format!("/api/tasks/{task}/runs"),
            &json!({}),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(started.status),
        "starting a run in another tenant's workspace returned {}",
        started.status
    );

    let queued = client
        .post_json_auth(
            &format!("/api/tasks/{task}/queue"),
            &json!({}),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(queued.status),
        "queueing another tenant's task returned {}",
        queued.status
    );

    let runs = client
        .get_auth(&format!("/api/tasks/{task}/runs"), &attacker.token)
        .await;
    assert_eq!(
        runs.status,
        StatusCode::NOT_FOUND,
        "listing another tenant's runs returned {}",
        runs.status
    );
}

#[tokio::test]
async fn a_stranger_cannot_repoint_another_tenants_model_host() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    // These settings decide where every model call goes and carry the key sent
    // with it, so an accepted write here exfiltrates the victim's chat and
    // knowledge text along with their provider credential.
    let redirected = client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/settings/ai",
                victim.organization
            ),
            &json!({ "provider": "self_hosted", "litellm_host": "https://collector.example.invalid" }),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(redirected.status),
        "repointing another tenant's model host returned {}",
        redirected.status
    );

    let read = client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", victim.organization),
            &attacker.token,
        )
        .await;
    assert_eq!(
        read.status,
        StatusCode::NOT_FOUND,
        "reading another tenant's AI settings returned {}",
        read.status
    );

    let erased = client
        .delete_auth(
            &format!("/api/organizations/{}/settings/ai", victim.organization),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(erased.status),
        "deleting another tenant's AI settings returned {}",
        erased.status
    );

    let workspace_write = client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/workspaces/{}/settings/ai",
                victim.organization, victim.workspace
            ),
            &json!({ "provider": "self_hosted", "litellm_host": "https://collector.example.invalid" }),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(workspace_write.status),
        "repointing another tenant's workspace model host returned {}",
        workspace_write.status
    );
}

#[tokio::test]
async fn a_stranger_cannot_rewrite_another_tenants_branding() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    let rewritten = client
        .put_json_auth(
            &format!("/api/workspaces/{}/theme", victim.workspace),
            &json!({ "primary_color": "#000000" }),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(rewritten.status),
        "rewriting another tenant's theme returned {}",
        rewritten.status
    );

    let erased = client
        .delete_auth(
            &format!("/api/workspaces/{}/theme", victim.workspace),
            &attacker.token,
        )
        .await;
    assert!(
        denied_write(erased.status),
        "deleting another tenant's theme returned {}",
        erased.status
    );
}

#[tokio::test]
async fn a_member_still_reaches_everything_in_their_own_workspace() {
    let client = TestClient::with_db().await;
    let owner = tenant(&client).await;
    let task = task_in(&client, &owner).await;

    client
        .get_auth(
            &format!("/api/workspaces/{}/tasks", owner.workspace),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    client
        .get_auth(&format!("/api/tasks/{task}"), &owner.token)
        .await
        .assert_status(StatusCode::OK);

    client
        .get_auth(&format!("/api/tasks/{task}/runs"), &owner.token)
        .await
        .assert_status(StatusCode::OK);

    client
        .get_auth(
            &format!("/api/organizations/{}/settings/ai", owner.organization),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    // Writing then reading, so this covers the write gate too and does not
    // mistake the empty-state 404 for a refusal.
    client
        .put_json_auth(
            &format!("/api/workspaces/{}/theme", owner.workspace),
            &json!({ "primary_color": "#123456" }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    let theme = client
        .get_auth(
            &format!("/api/workspaces/{}/theme", owner.workspace),
            &owner.token,
        )
        .await;
    theme.assert_status(StatusCode::OK);
}
