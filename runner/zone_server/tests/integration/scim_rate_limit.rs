use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::json;
use uuid::Uuid;

use crate::common::TestResponse;
use crate::common::scim::{self, Fixture, USER_SCHEMA, assert_refused, assert_scim};
use crate::common::test_email;

const PREFIX: &str = "scim_rate_limit";
const WINDOW: Duration = Duration::from_secs(60);

pub fn throttled(response: &TestResponse, window: Duration) {
    assert_refused(response, StatusCode::TOO_MANY_REQUESTS, Some("tooMany"));
    let retry_after: u64 = response
        .header("retry-after")
        .unwrap_or_else(|| panic!("no Retry-After on {}", response.text()))
        .parse()
        .unwrap();
    assert!(
        (1..=window.as_secs()).contains(&retry_after),
        "{retry_after}"
    );
}

async fn statuses(fixture: &Fixture, organization: Uuid) -> Vec<i16> {
    sqlx::query_scalar(
        "SELECT status FROM scim_errors WHERE organization_id = $1 ORDER BY occurred_at, id",
    )
    .bind(organization)
    .fetch_all(fixture.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn an_organization_over_its_budget_is_told_to_wait_while_another_carries_on() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let second = fixture.token().await;
    let neighbour = fixture.stranger().await;
    let other = fixture.organization_of(&neighbour).await;
    let foreign = fixture.token_for(other).await;
    fixture.state().limit_scim(3, WINDOW);

    for _ in 0..2 {
        for secret in ["zscim_wrong", "", foreign.secret.as_str()] {
            assert_refused(
                &fixture.scim_get("/Users", secret).await,
                StatusCode::UNAUTHORIZED,
                None,
            );
        }
    }

    assert_scim(
        &fixture.scim_get("/Users", &token.secret).await,
        StatusCode::OK,
    );
    fixture.provision(&token.secret, &test_email()).await;
    assert_scim(
        &fixture
            .scim_get("/ServiceProviderConfig", &second.secret)
            .await,
        StatusCode::OK,
    );

    throttled(&fixture.scim_get("/Users", &token.secret).await, WINDOW);
    throttled(&fixture.scim_get("/Users", &second.secret).await, WINDOW);
    let email = test_email();
    throttled(
        &fixture
            .scim_post(
                "/Users",
                &token.secret,
                &json!({ "schemas": [USER_SCHEMA], "userName": email }),
            )
            .await,
        WINDOW,
    );
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = $1")
        .bind(&email)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(accounts, 0, "a throttled write still ran");
    assert_refused(
        &fixture.scim_get("/Users", "zscim_wrong").await,
        StatusCode::UNAUTHORIZED,
        None,
    );

    let elsewhere = format!("{}/Users", scim::base(other));
    for _ in 0..3 {
        assert_scim(
            &fixture
                .scim(Method::GET, &elsewhere, &foreign.secret, None)
                .await,
            StatusCode::OK,
        );
    }
    throttled(
        &fixture
            .scim(Method::GET, &elsewhere, &foreign.secret, None)
            .await,
        WINDOW,
    );

    assert_eq!(
        statuses(&fixture, fixture.organization).await,
        [429],
        "a window's refusals are recorded once"
    );
    assert_eq!(statuses(&fixture, other).await, [429]);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_flood_from_one_network_is_refused_before_any_token_is_read() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    fixture.state().limit_scim_addresses(2);

    for _ in 0..5 {
        assert_scim(
            &fixture.scim_get("/Users", &token.secret).await,
            StatusCode::OK,
        );
    }
    for _ in 0..2 {
        assert_refused(
            &fixture.scim_get("/Users", "zscim_wrong").await,
            StatusCode::UNAUTHORIZED,
            None,
        );
    }

    for _ in 0..5 {
        throttled(&fixture.scim_get("/Users", "zscim_wrong").await, WINDOW);
        throttled(&fixture.scim_get("/Users", "zscim_guessed").await, WINDOW);
    }
    assert_scim(
        &fixture.scim_get("/Users", &token.secret).await,
        StatusCode::OK,
    );
    assert_eq!(
        statuses(&fixture, fixture.organization).await,
        Vec::<i16>::new(),
        "a refusal before any token is read records nothing"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_valid_token_still_deprovisions_from_a_network_refused_for_failures() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    fixture.state().limit_scim_addresses(2);

    for _ in 0..2 {
        assert_refused(
            &fixture.scim_get("/Users", "zscim_wrong").await,
            StatusCode::UNAUTHORIZED,
            None,
        );
    }
    throttled(&fixture.scim_get("/Users", "zscim_wrong").await, WINDOW);

    assert_scim(
        &fixture.scim_get("/Users", &token.secret).await,
        StatusCode::OK,
    );
    let user = fixture.provision(&token.secret, &test_email()).await;
    fixture
        .scim_delete(&format!("/Users/{user}"), &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.holding(user).await, None);

    throttled(&fixture.scim_get("/Users", "zscim_wrong").await, WINDOW);
    assert_eq!(
        statuses(&fixture, fixture.organization).await,
        Vec::<i16>::new()
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_budget_comes_back_when_the_window_passes() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let window = Duration::from_secs(1);
    fixture.state().limit_scim(1, window);

    assert_scim(
        &fixture.scim_get("/Users", &token.secret).await,
        StatusCode::OK,
    );
    throttled(&fixture.scim_get("/Users", &token.secret).await, window);

    tokio::time::sleep(window + Duration::from_millis(200)).await;
    assert_scim(
        &fixture.scim_get("/Users", &token.secret).await,
        StatusCode::OK,
    );

    fixture.cleanup().await;
}
