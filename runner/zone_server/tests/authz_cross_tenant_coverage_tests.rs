//! `cross_tenant_authorization_tests` covers tasks, task runs, AI settings and
//! branding. These are the id-addressed routes it does not: chats and their
//! messages, sources, knowledge, audit logs, billing, and session revocation.
//! Each one takes a resource id straight from the request, so each one is
//! driven here with a stranger's id to pin that the binding holds.

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
    let body = response.json_value();
    let token = body["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let response = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Tenant", "slug": uuid::Uuid::new_v4().to_string() }),
            &token,
        )
        .await;
    let organization = response.json_value()["organization"]["id"]
        .as_str()
        .expect("organization is created")
        .to_string();

    let response = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Tenant workspace", "slug": uuid::Uuid::new_v4().to_string() }),
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

fn refused(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED
    )
}

#[tokio::test]
async fn a_stranger_cannot_reach_another_tenants_chat_or_its_messages() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    let created = client
        .post_json_auth(
            "/api/chats",
            &json!({
                "workspace_id": victim.workspace,
                "title": "The tenant's own chat",
                "model_name": "test-model",
            }),
            &victim.token,
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let body = created.json_value();
    let chat = body["chat"]["id"]
        .as_str()
        .or_else(|| body["id"].as_str())
        .expect("chat is created")
        .to_string();

    for (label, response) in [
        (
            "read the chat",
            client
                .get_auth(&format!("/api/chats/{chat}"), &attacker.token)
                .await,
        ),
        (
            "read its messages",
            client
                .get_auth(&format!("/api/chats/{chat}/messages"), &attacker.token)
                .await,
        ),
        (
            "rename the chat",
            client
                .put_json_auth(
                    &format!("/api/chats/{chat}"),
                    &json!({ "title": "seized" }),
                    &attacker.token,
                )
                .await,
        ),
        (
            "archive the chat",
            client
                .post_json_auth(
                    &format!("/api/chats/{chat}/archive"),
                    &json!({}),
                    &attacker.token,
                )
                .await,
        ),
        (
            "delete a message in it",
            client
                .delete_auth(
                    &format!("/api/chats/{chat}/messages/{}", uuid::Uuid::new_v4()),
                    &attacker.token,
                )
                .await,
        ),
        (
            "delete the chat",
            client
                .delete_auth(&format!("/api/chats/{chat}"), &attacker.token)
                .await,
        ),
    ] {
        assert!(
            refused(response.status),
            "a stranger could {label}: {} {}",
            response.status,
            response.text()
        );
    }

    client
        .get_auth(&format!("/api/chats/{chat}"), &victim.token)
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn a_stranger_cannot_reach_another_tenants_source_through_their_own_workspace() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    let created = client
        .post_json_auth(
            &format!("/api/workspaces/{}/sources", victim.workspace),
            &json!({
                "name": format!("The tenant's source {}", uuid::Uuid::new_v4()),
                "source_type": "text",
                "config": {},
            }),
            &victim.token,
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let body = created.json_value();
    let source = body["source"]["id"]
        .as_str()
        .or_else(|| body["id"].as_str())
        .expect("source is created")
        .to_string();

    // Named under the attacker's own workspace, which they legitimately write.
    let theirs = &attacker.workspace;
    for (label, response) in [
        (
            "read it",
            client
                .get_auth(
                    &format!("/api/workspaces/{theirs}/sources/{source}"),
                    &attacker.token,
                )
                .await,
        ),
        (
            "rewrite it",
            client
                .put_json_auth(
                    &format!("/api/workspaces/{theirs}/sources/{source}"),
                    &json!({ "name": "seized" }),
                    &attacker.token,
                )
                .await,
        ),
        (
            "delete it",
            client
                .delete_auth(
                    &format!("/api/workspaces/{theirs}/sources/{source}"),
                    &attacker.token,
                )
                .await,
        ),
        (
            "reindex it",
            client
                .post_json_auth(
                    &format!("/api/workspaces/{theirs}/sources/{source}/reindex"),
                    &json!({}),
                    &attacker.token,
                )
                .await,
        ),
        (
            "verify it",
            client
                .post_json_auth(
                    &format!("/api/workspaces/{theirs}/sources/{source}/verify"),
                    &json!({}),
                    &attacker.token,
                )
                .await,
        ),
    ] {
        assert!(
            refused(response.status),
            "a stranger could {label}: {} {}",
            response.status,
            response.text()
        );
    }

    client
        .get_auth(
            &format!("/api/workspaces/{}/sources/{source}", victim.workspace),
            &victim.token,
        )
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn a_stranger_cannot_delete_another_tenants_knowledge() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    let created = client
        .post_json_auth(
            "/api/knowledge",
            &json!({
                "workspace_id": victim.workspace,
                "title": "The tenant's note",
                "content": "Something only the tenant should hold.",
            }),
            &victim.token,
        )
        .await;
    created.assert_status(StatusCode::CREATED);
    let body = created.json_value();
    let entry = body["id"]
        .as_str()
        .or_else(|| body["knowledge"]["id"].as_str())
        .expect("knowledge is created")
        .to_string();

    let deleted = client
        .delete_auth(&format!("/api/knowledge/{entry}"), &attacker.token)
        .await;
    assert!(
        refused(deleted.status),
        "a stranger deleted another tenant's knowledge: {} {}",
        deleted.status,
        deleted.text()
    );

    let listed = client
        .get_auth(
            &format!("/api/knowledge?workspace_id={}", victim.workspace),
            &victim.token,
        )
        .await;
    assert!(
        listed.text().contains("The tenant's note"),
        "the entry did not survive: {}",
        listed.text()
    );
}

#[tokio::test]
async fn a_stranger_cannot_read_another_tenants_audit_log_or_billing() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    let logs = client
        .get_auth(
            &format!("/api/organizations/{}/audit-logs", victim.organization),
            &victim.token,
        )
        .await;
    logs.assert_status(StatusCode::OK);
    let log = logs.json_value()["logs"]
        .as_array()
        .and_then(|logs| logs.first())
        .and_then(|log| log["id"].as_str())
        .map(str::to_string);

    let victim_org = &victim.organization;
    let mut probes = vec![
        (
            "list the audit log",
            client
                .get_auth(
                    &format!("/api/organizations/{victim_org}/audit-logs"),
                    &attacker.token,
                )
                .await,
        ),
        (
            "export the audit log",
            client
                .get_auth(
                    &format!(
                        "/api/organizations/{victim_org}/audit-logs/export?start_date=2000-01-01T00:00:00Z&end_date=2100-01-01T00:00:00Z"
                    ),
                    &attacker.token,
                )
                .await,
        ),
        (
            "read the subscription",
            client
                .get_auth(
                    &format!("/api/organizations/{victim_org}/subscription"),
                    &attacker.token,
                )
                .await,
        ),
        (
            "read the usage",
            client
                .get_auth(
                    &format!("/api/organizations/{victim_org}/usage"),
                    &attacker.token,
                )
                .await,
        ),
        (
            "read the limits",
            client
                .get_auth(
                    &format!("/api/organizations/{victim_org}/limits"),
                    &attacker.token,
                )
                .await,
        ),
    ];

    // A stranger's log id, named under the organization the attacker owns.
    if let Some(log) = log {
        probes.push((
            "read one log entry through their own organization",
            client
                .get_auth(
                    &format!(
                        "/api/organizations/{}/audit-logs/{log}",
                        attacker.organization
                    ),
                    &attacker.token,
                )
                .await,
        ));
    }

    for (label, response) in probes {
        assert!(
            refused(response.status),
            "a stranger could {label}: {} {}",
            response.status,
            response.text()
        );
    }
}

#[tokio::test]
async fn a_stranger_cannot_revoke_another_users_session() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;

    let sessions = client.get_auth("/api/auth/sessions", &victim.token).await;
    sessions.assert_status(StatusCode::OK);
    let session = sessions.json_value()["sessions"]
        .as_array()
        .and_then(|sessions| sessions.first())
        .and_then(|session| session["id"].as_str())
        .expect("the victim holds a session")
        .to_string();

    let revoked = client
        .delete_auth(&format!("/api/auth/sessions/{session}"), &attacker.token)
        .await;
    assert!(
        refused(revoked.status),
        "a stranger revoked another user's session: {} {}",
        revoked.status,
        revoked.text()
    );

    client
        .get_auth("/api/auth/sessions", &victim.token)
        .await
        .assert_status(StatusCode::OK);
}

/// Deactivating an organization is reachable by an admin while deleting one
/// takes an owner. It is only a real asymmetry if the flag locks anyone out --
/// nothing reads it, and this pins that, so the two routes are not siblings.
#[tokio::test]
async fn deactivating_an_organization_locks_nobody_out() {
    let client = TestClient::with_db().await;
    let owner = tenant(&client).await;

    client
        .patch_json_auth(
            &format!("/api/organizations/{}", owner.organization),
            &json!({ "is_active": false }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    client
        .get_auth(
            &format!("/api/organizations/{}", owner.organization),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
    client
        .get_auth(
            &format!("/api/organizations/{}/members", owner.organization),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
}
