//! The console's contract with the authentication endpoints, pinned from the
//! live pass of 2026-09-20: a verification link opened twice still verifies,
//! every auth outcome carries `success`, the sessions listing names the user
//! and the current session, and revoking the other sessions kills their
//! refresh tokens while keeping the caller's own.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{TestClient, test_email, test_password};
use zone_server::db::{email_verification, password_reset};

struct Login {
    access: String,
    refresh: String,
    user: Uuid,
}

fn login_from(body: Value) -> Login {
    Login {
        access: body["access_token"].as_str().unwrap().to_string(),
        refresh: body["refresh_token"].as_str().unwrap().to_string(),
        user: Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap(),
    }
}

async fn register(client: &TestClient, email: &str) -> Login {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": email, "password": test_password(), "display_name": "Console" }),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    login_from(response.json_value())
}

async fn login(client: &TestClient, email: &str) -> Login {
    let response = client
        .post_json(
            "/api/auth/login",
            &json!({ "email": email, "password": test_password() }),
        )
        .await;
    response.assert_status(StatusCode::OK);
    login_from(response.json_value())
}

#[tokio::test]
async fn a_verification_link_opened_twice_reads_as_verified_both_times() {
    let client = TestClient::with_db().await;
    let account = register(&client, &test_email()).await;
    let (token, _) =
        email_verification::create_verification_token(client.state().db(), account.user)
            .await
            .unwrap();

    for attempt in 1..=2 {
        let response = client
            .post_json("/api/auth/verify-email", &json!({ "token": token }))
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "attempt {attempt} was refused: {}",
            response.text()
        );
        let body = response.json_value();
        assert_eq!(body["success"], json!(true), "attempt {attempt}: {body}");
        assert!(body["message"].is_string(), "attempt {attempt}: {body}");
    }

    let response = client
        .post_json("/api/auth/verify-email", &json!({ "token": "not-a-token" }))
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn every_auth_outcome_carries_a_success_flag() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let account = register(&client, &email).await;

    for (path, body) in [
        ("/api/auth/resend-verification", json!({ "email": email })),
        ("/api/auth/forgot-password", json!({ "email": email })),
        (
            "/api/auth/forgot-password",
            json!({ "email": test_email() }),
        ),
    ] {
        let response = client.post_json(path, &body).await;
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_eq!(body["success"], json!(true), "{path}: {body}");
        assert!(body["message"].is_string(), "{path}: {body}");
    }

    let (token, _) = password_reset::create_reset_token(client.state().db(), account.user)
        .await
        .unwrap();
    let response = client
        .post_json(
            "/api/auth/reset-password",
            &json!({ "token": token, "new_password": "Another-Password-1" }),
        )
        .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["success"], json!(true), "{body}");
    assert!(body["message"].is_string(), "{body}");
}

#[tokio::test]
async fn the_sessions_listing_names_the_user_and_the_current_session() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let first = register(&client, &email).await;
    let second = login(&client, &email).await;

    let response = client.get_auth("/api/auth/sessions", &second.access).await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{body}");
    for session in sessions {
        assert_eq!(
            session["user_id"],
            json!(first.user.to_string()),
            "{session}"
        );
        assert!(session.get("location").is_some(), "{session}");
        assert!(session["created_at"].is_string(), "{session}");
    }
    let current: Vec<&Value> = sessions
        .iter()
        .filter(|session| session["is_current"] == json!(true))
        .collect();
    assert_eq!(current.len(), 1, "one row is the caller's own: {body}");
}

#[tokio::test]
async fn revoking_the_other_sessions_keeps_the_caller_and_kills_their_refresh_tokens() {
    let client = TestClient::with_db().await;
    let email = test_email();
    let phone = register(&client, &email).await;
    let laptop = login(&client, &email).await;

    let response = client
        .delete_auth("/api/auth/sessions", &laptop.access)
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["revoked_count"], json!(1));

    client
        .get_auth("/api/auth/sessions", &phone.access)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    client
        .post_json(
            "/api/auth/refresh",
            &json!({ "refresh_token": phone.refresh }),
        )
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    let remaining = client.get_auth("/api/auth/sessions", &laptop.access).await;
    remaining.assert_status(StatusCode::OK);
    assert_eq!(
        remaining.json_value()["sessions"].as_array().unwrap().len(),
        1,
        "the caller's own session survives"
    );

    let refreshed = client
        .post_json(
            "/api/auth/refresh",
            &json!({ "refresh_token": laptop.refresh }),
        )
        .await;
    refreshed.assert_status(StatusCode::OK);
    let rotated = login_from(refreshed.json_value());
    let after = client.get_auth("/api/auth/sessions", &rotated.access).await;
    after.assert_status(StatusCode::OK);
    let sessions = after.json_value();
    assert_eq!(
        sessions["sessions"].as_array().unwrap().len(),
        1,
        "a refresh keeps the session instead of adding one: {sessions}"
    );
    assert_eq!(sessions["sessions"][0]["is_current"], json!(true));
}
