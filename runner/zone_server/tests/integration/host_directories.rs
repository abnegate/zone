use axum::http::StatusCode;
use serde_json::json;
use tempfile::TempDir;

use crate::common::{TestClient, test_email, test_password};
use zone_server::config::Config;
use zone_server::host_mounts::HostMounts;

async fn signed_in(client: &TestClient) -> (String, String) {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({
                "email": test_email(),
                "password": test_password(),
                "display_name": "Host Folders"
            }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    let token = response.json_value()["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let orgs = client.get_auth("/api/organizations", &token).await;
    orgs.assert_status(StatusCode::OK);
    let org = orgs.json_value()["organizations"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspaces = client
        .get_auth(&format!("/api/organizations/{org}/workspaces"), &token)
        .await;
    workspaces.assert_status(StatusCode::OK);
    let workspace = workspaces.json_value()["workspaces"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    (token, workspace)
}

#[tokio::test]
async fn native_host_directories_round_trip() {
    let client = TestClient::with_db().await;
    let (token, workspace) = signed_in(&client).await;

    let mounts = client.get_auth("/api/host-mounts", &token).await;
    mounts.assert_status(StatusCode::OK);
    assert_eq!(mounts.json_value()["in_container"], false);
    assert_eq!(mounts.json_value()["ready"], true);

    let listed = client
        .get_auth(
            &format!("/api/workspaces/{workspace}/host-directories"),
            &token,
        )
        .await;
    listed.assert_status(StatusCode::OK);
    assert!(
        listed.json_value()["directories"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let directory = TempDir::new().unwrap();
    let path = directory.path().to_string_lossy().into_owned();
    let saved = client
        .put_json_auth(
            &format!("/api/workspaces/{workspace}/host-directories"),
            &json!({ "directories": [path] }),
            &token,
        )
        .await;
    saved.assert_status(StatusCode::OK);
    assert_eq!(saved.json_value()["directories"][0], path);
    assert_eq!(saved.json_value()["folders"][0]["exists"], true);
    assert_eq!(saved.json_value()["folders"][0]["mapped"], path);
}

#[tokio::test]
async fn a_relative_host_directory_is_refused() {
    let client = TestClient::with_db().await;
    let (token, workspace) = signed_in(&client).await;
    let response = client
        .put_json_auth(
            &format!("/api/workspaces/{workspace}/host-directories"),
            &json!({ "directories": ["Local/jbs"] }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(response.json_value()["code"], "invalid_path");
}

#[tokio::test]
async fn a_container_without_host_root_refuses_saves() {
    let config = Config {
        host_mounts: HostMounts {
            host_root: None,
            in_container: true,
        },
        ..crate::common::test_config()
    };
    let client = TestClient::with_config(config).await;
    let (token, workspace) = signed_in(&client).await;
    let mounts = client.get_auth("/api/host-mounts", &token).await;
    mounts.assert_status(StatusCode::OK);
    assert_eq!(mounts.json_value()["ready"], false);
    let response = client
        .put_json_auth(
            &format!("/api/workspaces/{workspace}/host-directories"),
            &json!({ "directories": ["/Users/jake/Local/jbs"] }),
            &token,
        )
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(response.json_value()["code"], "host_root_unset");
}
