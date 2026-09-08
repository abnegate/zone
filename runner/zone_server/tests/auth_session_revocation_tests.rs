mod common;

use axum::{Router, http::StatusCode, middleware, routing::get};
use uuid::Uuid;
use zone_server::auth::{AuthUser, create_session_access_token, require_auth};
use zone_server::db::{sessions, users};

async fn extractor_handler(_auth: AuthUser) -> StatusCode {
    StatusCode::OK
}

#[tokio::test]
async fn revoked_session_is_rejected_by_extractor_and_middleware() {
    let config = common::test_config();
    let pool = common::create_test_pool().await;
    let state = common::create_test_state(config.clone(), pool.clone());
    let user = users::create_user(
        &pool,
        &common::test_email(),
        "password-hash",
        Some("Revoked session test"),
        false,
    )
    .await
    .unwrap();
    let session = sessions::create_session(
        &pool,
        user.id,
        &format!("refresh-{}", Uuid::new_v4()),
        None,
        None,
        None,
        (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
    )
    .await
    .unwrap();
    let token = create_session_access_token(
        user.id,
        &user.email,
        vec![],
        vec![],
        false,
        session.id,
        config.jwt_secret(),
        chrono::Duration::hours(1),
    )
    .unwrap();

    let extractor = common::TestClient::new(
        Router::new()
            .route("/extractor", get(extractor_handler))
            .with_state(state.clone()),
    );
    let middleware = common::TestClient::new(
        Router::new()
            .route("/middleware", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn_with_state(state.clone(), require_auth))
            .with_state(state),
    );

    extractor
        .get_auth("/extractor", &token)
        .await
        .assert_status(StatusCode::OK);
    middleware
        .get_auth("/middleware", &token)
        .await
        .assert_status(StatusCode::OK);

    sessions::revoke_session(&pool, session.id).await.unwrap();

    extractor
        .get_auth("/extractor", &token)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    middleware
        .get_auth("/middleware", &token)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}
