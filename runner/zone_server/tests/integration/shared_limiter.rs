//! The context-gather rate limit against a real Valkey at `REDIS_URL`. Two
//! states that share one Valkey stand for two replicas behind one Service.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use metrics::Gauge;
use serde_json::json;
use uuid::Uuid;
use zone_server::cluster::Bus;
use zone_server::services::limit::{Scope, Window, key};
use zone_server::state::AppState;

use crate::common::{
    TestClient, TestResponse, create_test_pool, create_test_state, test_config, test_email,
    test_password,
};

const LIMIT: u32 = 10;
const WINDOW: Duration = Duration::from_secs(60);
const MARGIN: Duration = Duration::from_secs(10);
const CONNECTED: Duration = Duration::from_secs(10);

pub fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

pub fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub async fn valkey() -> redis::aio::MultiplexedConnection {
    redis::Client::open(redis_url())
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap()
}

async fn replica(url: &str) -> AppState {
    let state = create_test_state(test_config(), create_test_pool().await);
    let bus = Bus::connect(url, state.master_key(), Gauge::noop());
    state.with_bus(bus)
}

pub async fn connected(state: &AppState) {
    tokio::time::timeout(
        CONNECTED,
        state.bus().connection().wait_for(|connected| *connected),
    )
    .await
    .expect("the bus subscribes on Valkey")
    .unwrap();
}

/// Waits out the end of the current fixed window when too little of it is
/// left for a burst of requests to land inside it.
pub async fn start_of_a_window() {
    let window = WINDOW.as_millis();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let left = Duration::from_millis(u64::try_from(window - now % window).unwrap());
    if left < MARGIN {
        tokio::time::sleep(left).await;
    }
}

async fn register(client: &TestClient) -> String {
    let response = client
        .post_json(
            "/api/auth/register",
            &json!({ "email": test_email(), "password": test_password() }),
        )
        .await;
    response.json_value()["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string()
}

async fn workspace(client: &TestClient, token: &str) -> String {
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({ "name": "Limited", "slug": Uuid::new_v4().to_string() }),
            token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("the organization is created")
        .to_string();
    client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({ "name": "Limited workspace", "slug": Uuid::new_v4().to_string() }),
            token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("the workspace is created")
        .to_string()
}

fn header(response: &TestResponse, name: &str) -> u64 {
    response
        .header(name)
        .unwrap_or_else(|| panic!("{name} is missing"))
        .parse()
        .unwrap_or_else(|_| panic!("{name} is not a number"))
}

#[tokio::test]
async fn two_replicas_sharing_valkey_together_allow_exactly_the_limit_per_window() {
    let first = replica(&redis_url()).await;
    let second = replica(&redis_url()).await;
    connected(&first).await;
    connected(&second).await;
    let replicas = [
        TestClient::with_state(first),
        TestClient::with_state(second),
    ];
    let token = register(&replicas[0]).await;
    let workspace = workspace(&replicas[0], &token).await;
    let body = json!({ "workspace_id": workspace, "source_ids": [] });
    start_of_a_window().await;

    for request in 0..LIMIT {
        let replica = &replicas[usize::try_from(request).unwrap() % replicas.len()];
        let response = replica
            .post_json_auth("/api/context/gather", &body, &token)
            .await;
        response.assert_status(StatusCode::ACCEPTED);
        assert_eq!(header(&response, "x-ratelimit-limit"), u64::from(LIMIT));
        assert_eq!(
            header(&response, "x-ratelimit-remaining"),
            u64::from(LIMIT - request - 1)
        );
        assert!(header(&response, "x-ratelimit-reset") <= WINDOW.as_secs());
    }

    for replica in &replicas {
        let refused = replica
            .post_json_auth("/api/context/gather", &body, &token)
            .await;
        refused.assert_status(StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(header(&refused, "x-ratelimit-limit"), u64::from(LIMIT));
        assert_eq!(header(&refused, "x-ratelimit-remaining"), 0);
        assert!(header(&refused, "x-ratelimit-reset") <= WINDOW.as_secs());
    }
}

async fn limited_alone(state: &AppState, user: Uuid) {
    for remaining in (0..LIMIT).rev() {
        let decision = state.limiter().check(user).await;
        assert!(decision.allowed);
        assert_eq!(decision.remaining, remaining);
    }
    let refused = state.limiter().check(user).await;
    assert!(!refused.allowed);
    assert_eq!(refused.remaining, 0);
}

#[tokio::test]
async fn with_valkey_down_each_replica_limits_on_its_own_count() {
    let url = format!("redis://127.0.0.1:{}", closed_port());
    let first = replica(&url).await;
    let second = replica(&url).await;
    let user = Uuid::new_v4();

    limited_alone(&first, user).await;
    limited_alone(&second, user).await;
}

#[tokio::test]
async fn a_count_valkey_cannot_increment_is_limited_by_this_replica_alone() {
    let state = replica(&redis_url()).await;
    connected(&state).await;
    let user = Uuid::new_v4();
    let mut connection = valkey().await;
    let current = Window::current(WINDOW).index;
    for window in [current, current + 1] {
        redis::cmd("SET")
            .arg(key(state.bus().namespace(), Scope::Gather, user, window))
            .arg("not a count")
            .arg("EX")
            .arg(WINDOW.as_secs() * 2)
            .exec_async(&mut connection)
            .await
            .unwrap();
    }

    limited_alone(&state, user).await;
}

#[tokio::test]
async fn a_shared_count_lives_under_its_namespaced_key_for_one_window() {
    let state = replica(&redis_url()).await;
    connected(&state).await;
    let user = Uuid::new_v4();
    start_of_a_window().await;

    let decision = state.limiter().check(user).await;

    assert!(decision.allowed);
    assert_eq!(decision.remaining, LIMIT - 1);
    let counted = key(
        state.bus().namespace(),
        Scope::Gather,
        user,
        Window::current(WINDOW).index,
    );
    let mut connection = valkey().await;
    let count: u64 = redis::cmd("GET")
        .arg(&counted)
        .query_async(&mut connection)
        .await
        .unwrap();
    let seconds: i64 = redis::cmd("TTL")
        .arg(&counted)
        .query_async(&mut connection)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(
        (1..=i64::try_from(WINDOW.as_secs()).unwrap()).contains(&seconds),
        "{seconds}"
    );
}
