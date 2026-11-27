//! The SCIM limits against a real Valkey at `REDIS_URL`. A fixture's server and a replica of it
//! share one database and one Valkey, standing for two replicas behind one Service. Each test
//! derives its own namespace from a fresh encryption key, so parallel tests on one Valkey never
//! count into each other's keys.

use axum::http::StatusCode;
use uuid::Uuid;
use zone_server::config::Config;
use zone_server::services::limit::{Scope, Window, key};
use zone_server::services::scim::limit;
use zone_server::services::sso::throttle;

use crate::common::scim::{Fixture, assert_refused, assert_scim};
use crate::common::{TestClient, TestResponse, test_config};
use crate::scim_rate_limit::throttled;
use crate::shared_limiter::{closed_port, connected, redis_url, start_of_a_window, valkey};

const PREFIX: &str = "shared_scim_rate_limit";
const REQUESTS: u32 = 4;
const FAILURES: u32 = 4;
const WRONG: &str = "zscim_wrong";

fn isolated() -> Config {
    Config {
        encryption_key: Uuid::new_v4().simple().to_string(),
        ..test_config()
    }
}

async fn replicas(url: &str) -> (Fixture, TestClient) {
    let fixture = Fixture::clustered(PREFIX, isolated(), url).await;
    let replica = fixture.replica(url);
    (fixture, replica)
}

async fn connected_replicas() -> (Fixture, TestClient) {
    let (fixture, replica) = replicas(&redis_url()).await;
    connected(fixture.state()).await;
    connected(replica.state()).await;
    (fixture, replica)
}

async fn get(fixture: &Fixture, replica: &TestClient, turn: u32, token: &str) -> TestResponse {
    if turn.is_multiple_of(2) {
        fixture.scim_get("/Users", token).await
    } else {
        fixture.scim_get_through(replica, "/Users", token).await
    }
}

async fn count(counted: &str) -> Option<u64> {
    redis::cmd("GET")
        .arg(counted)
        .query_async(&mut valkey().await)
        .await
        .unwrap()
}

#[tokio::test]
async fn two_replicas_sharing_valkey_allow_an_organization_one_combined_budget() {
    let (fixture, replica) = connected_replicas().await;
    let token = fixture.token().await;
    fixture.state().limit_scim(REQUESTS, limit::WINDOW);
    replica.state().limit_scim(REQUESTS, limit::WINDOW);
    start_of_a_window().await;

    for turn in 0..REQUESTS {
        assert_scim(
            &get(&fixture, &replica, turn, &token.secret).await,
            StatusCode::OK,
        );
    }

    throttled(
        &fixture.scim_get("/Users", &token.secret).await,
        limit::WINDOW,
    );
    throttled(
        &fixture
            .scim_get_through(&replica, "/Users", &token.secret)
            .await,
        limit::WINDOW,
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn two_replicas_sharing_valkey_allow_an_address_one_combined_failure_budget() {
    let (fixture, replica) = connected_replicas().await;
    let token = fixture.token().await;
    fixture.state().limit_scim_addresses(FAILURES);
    replica.state().limit_scim_addresses(FAILURES);
    start_of_a_window().await;

    for turn in 0..FAILURES {
        assert_refused(
            &get(&fixture, &replica, turn, WRONG).await,
            StatusCode::UNAUTHORIZED,
            None,
        );
    }

    for _ in 0..2 {
        throttled(&fixture.scim_get("/Users", WRONG).await, throttle::WINDOW);
        throttled(
            &fixture.scim_get_through(&replica, "/Users", WRONG).await,
            throttle::WINDOW,
        );
    }
    for turn in 0..2 {
        assert_scim(
            &get(&fixture, &replica, turn, &token.secret).await,
            StatusCode::OK,
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn each_scim_limit_counts_under_its_own_key_apart_from_sign_ins() {
    let (fixture, _replica) = connected_replicas().await;
    let token = fixture.token().await;
    start_of_a_window().await;

    assert_scim(
        &fixture.scim_get("/Users", &token.secret).await,
        StatusCode::OK,
    );
    assert_refused(
        &fixture.scim_get("/Users", WRONG).await,
        StatusCode::UNAUTHORIZED,
        None,
    );

    let namespace = fixture.state().bus().namespace();
    let bucket = throttle::bucket(None);
    let failures = Window::current(throttle::WINDOW).index;
    let requests = Window::current(limit::WINDOW).index;
    assert_eq!(
        count(&key(namespace, Scope::Scim, fixture.organization, requests)).await,
        Some(1)
    );
    assert_eq!(
        count(&key(namespace, Scope::ScimFailures, bucket, failures)).await,
        Some(1)
    );
    assert_eq!(
        count(&key(namespace, Scope::Sso, bucket, failures)).await,
        None
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn with_valkey_unreachable_each_replica_limits_an_organization_on_its_own_count() {
    let url = format!("redis://127.0.0.1:{}", closed_port());
    let (fixture, replica) = replicas(&url).await;
    let token = fixture.token().await;
    fixture.state().limit_scim(REQUESTS, limit::WINDOW);
    replica.state().limit_scim(REQUESTS, limit::WINDOW);

    for _ in 0..REQUESTS {
        assert_scim(
            &fixture.scim_get("/Users", &token.secret).await,
            StatusCode::OK,
        );
    }
    throttled(
        &fixture.scim_get("/Users", &token.secret).await,
        limit::WINDOW,
    );
    for _ in 0..REQUESTS {
        assert_scim(
            &fixture
                .scim_get_through(&replica, "/Users", &token.secret)
                .await,
            StatusCode::OK,
        );
    }
    throttled(
        &fixture
            .scim_get_through(&replica, "/Users", &token.secret)
            .await,
        limit::WINDOW,
    );

    fixture.cleanup().await;
}
