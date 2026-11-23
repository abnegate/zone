//! The single sign-on throttle against a real Valkey at `REDIS_URL`. Two states that share one
//! Valkey stand for two replicas behind one Service. Each test derives its own namespace from a
//! fresh encryption key, so parallel test processes on one Valkey never count into its buckets.

use std::net::IpAddr;

use metrics::Gauge;
use uuid::Uuid;
use zone_server::cluster::Bus;
use zone_server::config::Config;
use zone_server::services::limit::{Scope, Window, key};
use zone_server::services::sso::throttle::WINDOW;
use zone_server::state::AppState;

use crate::common::{TestClient, create_test_pool, create_test_state};
use crate::shared_limiter::{closed_port, connected, redis_url, start_of_a_window, valkey};
use crate::sso_throttle::{LIMIT, authorize, limited, not_found, throttled};

const PEER: &str = "203.0.113.60";
const NEIGHBOUR: &str = "203.0.113.61";

fn isolated() -> Config {
    Config {
        encryption_key: Uuid::new_v4().simple().to_string(),
        ..throttled()
    }
}

async fn replica(url: &str, config: Config) -> AppState {
    let state = create_test_state(config, create_test_pool().await);
    let bus = Bus::connect(url, state.master_key(), Gauge::noop());
    state.with_bus(bus)
}

async fn replicas(url: &str) -> [AppState; 2] {
    let config = isolated();
    [
        replica(url, config.clone()).await,
        replica(url, config).await,
    ]
}

async fn limited_alone(client: &TestClient, peer: &str) {
    for _ in 0..LIMIT {
        not_found(&authorize(client, Some(peer), None).await);
    }
    limited(&authorize(client, Some(peer), None).await);
}

#[tokio::test]
async fn two_replicas_sharing_valkey_allow_one_combined_limit_per_address() {
    let states = replicas(&redis_url()).await;
    for state in &states {
        connected(state).await;
    }
    let clients = states.map(TestClient::with_state);
    start_of_a_window().await;

    for request in 0..LIMIT {
        let client = &clients[usize::try_from(request).unwrap() % clients.len()];
        not_found(&authorize(client, Some(PEER), None).await);
    }

    for client in &clients {
        limited(&authorize(client, Some(PEER), None).await);
        not_found(&authorize(client, Some(NEIGHBOUR), None).await);
    }
}

#[tokio::test]
async fn a_shared_count_lives_under_the_address_bucket_for_one_window() {
    let [state, _] = replicas(&redis_url()).await;
    connected(&state).await;
    let client = TestClient::with_state(state.clone());
    start_of_a_window().await;

    not_found(&authorize(&client, Some("2001:db8:9:9::5"), None).await);

    let bucket: IpAddr = "2001:db8:9:9::".parse().unwrap();
    let counted = key(
        state.bus().namespace(),
        Scope::Sso,
        bucket,
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

#[tokio::test]
async fn a_count_valkey_cannot_increment_is_limited_by_this_replica_alone() {
    let [state, _] = replicas(&redis_url()).await;
    connected(&state).await;
    let peer: IpAddr = PEER.parse().unwrap();
    let mut connection = valkey().await;
    let current = Window::current(WINDOW).index;
    for window in [current, current + 1] {
        redis::cmd("SET")
            .arg(key(state.bus().namespace(), Scope::Sso, peer, window))
            .arg("not a count")
            .arg("EX")
            .arg(WINDOW.as_secs() * 2)
            .exec_async(&mut connection)
            .await
            .unwrap();
    }

    limited_alone(&TestClient::with_state(state), PEER).await;
}

#[tokio::test]
async fn with_valkey_unreachable_each_replica_limits_on_its_own_count() {
    let url = format!("redis://127.0.0.1:{}", closed_port());
    let clients = replicas(&url).await.map(TestClient::with_state);

    for client in &clients {
        limited_alone(client, PEER).await;
    }
}
