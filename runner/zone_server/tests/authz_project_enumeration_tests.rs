//! `tasks.rs` tells a non-member that the task does not exist, with a comment
//! saying why: "the endpoints cannot be used to enumerate ids across tenants."
//! Every id-addressed project route answered `403 Not a member of this
//! workspace` for a stranger's project and `404` for one that does not exist,
//! so the pair separated real project ids from invented ones.

mod common;

use axum::http::StatusCode;
use serde_json::json;

use common::{TestClient, test_email, test_password};

struct Tenant {
    token: String,
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

    Tenant { token, workspace }
}

async fn project(client: &TestClient, owner: &Tenant) -> String {
    let response = client
        .post_json_auth(
            "/api/projects",
            &json!({ "name": "Tenant project", "workspace_id": owner.workspace }),
            &owner.token,
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    response.json_value()["project"]["id"]
        .as_str()
        .expect("project is created")
        .to_string()
}

#[tokio::test]
async fn a_strangers_project_id_is_indistinguishable_from_one_that_does_not_exist() {
    let client = TestClient::with_db().await;
    let victim = tenant(&client).await;
    let attacker = tenant(&client).await;
    let real = project(&client, &victim).await;
    let invented = uuid::Uuid::new_v4().to_string();

    let read = client
        .get_auth(&format!("/api/projects/{real}"), &attacker.token)
        .await;
    let read_invented = client
        .get_auth(&format!("/api/projects/{invented}"), &attacker.token)
        .await;
    assert_eq!(
        (read.status, read.text()),
        (read_invented.status, read_invented.text()),
        "a real project id answers differently from an invented one"
    );
    assert_eq!(read.status, StatusCode::NOT_FOUND);

    let written = client
        .put_json_auth(
            &format!("/api/projects/{real}"),
            &json!({ "name": "seized" }),
            &attacker.token,
        )
        .await;
    let written_invented = client
        .put_json_auth(
            &format!("/api/projects/{invented}"),
            &json!({ "name": "seized" }),
            &attacker.token,
        )
        .await;
    assert_eq!(
        (written.status, written.text()),
        (written_invented.status, written_invented.text()),
        "update separates a real project id from an invented one"
    );

    let removed = client
        .delete_auth(&format!("/api/projects/{real}"), &attacker.token)
        .await;
    let removed_invented = client
        .delete_auth(&format!("/api/projects/{invented}"), &attacker.token)
        .await;
    assert_eq!(
        (removed.status, removed.text()),
        (removed_invented.status, removed_invented.text()),
        "delete separates a real project id from an invented one"
    );

    let linked = client
        .post_json_auth(
            &format!("/api/projects/{real}/github"),
            &json!({ "repo_url": "https://github.com/attacker/repository" }),
            &attacker.token,
        )
        .await;
    let linked_invented = client
        .post_json_auth(
            &format!("/api/projects/{invented}/github"),
            &json!({ "repo_url": "https://github.com/attacker/repository" }),
            &attacker.token,
        )
        .await;
    assert_eq!(
        (linked.status, linked.text()),
        (linked_invented.status, linked_invented.text()),
        "link_github separates a real project id from an invented one"
    );

    let unlinked = client
        .delete_auth(&format!("/api/projects/{real}/github"), &attacker.token)
        .await;
    let unlinked_invented = client
        .delete_auth(&format!("/api/projects/{invented}/github"), &attacker.token)
        .await;
    assert_eq!(
        (unlinked.status, unlinked.text()),
        (unlinked_invented.status, unlinked_invented.text()),
        "unlink_github separates a real project id from an invented one"
    );
}

#[tokio::test]
async fn the_owner_still_reads_and_changes_their_own_project() {
    let client = TestClient::with_db().await;
    let owner = tenant(&client).await;
    let id = project(&client, &owner).await;

    client
        .get_auth(&format!("/api/projects/{id}"), &owner.token)
        .await
        .assert_status(StatusCode::OK);

    let renamed = client
        .put_json_auth(
            &format!("/api/projects/{id}"),
            &json!({ "name": "Renamed by its owner" }),
            &owner.token,
        )
        .await;
    assert_eq!(
        renamed.status,
        StatusCode::OK,
        "the owner lost their own project: {}",
        renamed.text()
    );
    assert_eq!(
        renamed.json_value()["project"]["name"],
        json!("Renamed by its owner"),
        "the rename did not take"
    );

    client
        .delete_auth(&format!("/api/projects/{id}"), &owner.token)
        .await
        .assert_status(StatusCode::NO_CONTENT);
}
