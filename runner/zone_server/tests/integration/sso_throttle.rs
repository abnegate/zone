use std::net::{IpAddr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use reqwest::Url;
use uuid::Uuid;
use zone_enterprise::sso::{PROVIDER_NOT_FOUND, RATE_LIMITED, STATE_INVALID};
use zone_server::config::Config;

use crate::common::{TestClient, TestResponse, test_config};

pub const LIMIT: u32 = 3;
const PORT: u16 = 40_000;
const FORWARDED_FOR: &str = "x-forwarded-for";
const LOOPBACK: &str = "127.0.0.1";

pub fn throttled() -> Config {
    Config {
        sso_requests_per_minute: LIMIT,
        ..test_config()
    }
}

async fn client() -> TestClient {
    TestClient::with_config(throttled()).await
}

fn address(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn connected(
    builder: axum::http::request::Builder,
    peer: Option<&str>,
    forwarded: Option<&str>,
) -> axum::http::request::Builder {
    let builder = match peer {
        Some(peer) => builder.extension(ConnectInfo(SocketAddr::new(address(peer), PORT))),
        None => builder,
    };
    match forwarded {
        Some(forwarded) => builder.header(FORWARDED_FOR, forwarded),
        None => builder,
    }
}

pub async fn authorize(
    client: &TestClient,
    peer: Option<&str>,
    forwarded: Option<&str>,
) -> TestResponse {
    let builder = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/auth/sso/providers/{}/authorizations",
            Uuid::new_v4()
        ))
        .header(header::CONTENT_TYPE, "application/json");
    client
        .send_request(
            connected(builder, peer, forwarded)
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
}

async fn post_acs(client: &TestClient, peer: &str) -> TestResponse {
    let builder = Request::builder()
        .method("POST")
        .uri("/api/auth/sso/saml/acs")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    client
        .send_request(
            connected(builder, Some(peer), None)
                .body(Body::from(
                    "SAMLResponse=PHNhbWw%2B&RelayState=never-issued",
                ))
                .unwrap(),
        )
        .await
}

pub fn not_found(response: &TestResponse) {
    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(response.json_value()["code"], PROVIDER_NOT_FOUND);
}

pub fn limited(response: &TestResponse) {
    response.assert_status(StatusCode::TOO_MANY_REQUESTS);
    let body = response.json_value();
    assert_eq!(body["code"], RATE_LIMITED, "{body}");
    assert_eq!(
        body["error"],
        "Too many sign-in attempts from your network. Wait a minute, then try again.",
    );
    let seconds: u64 = response
        .header(header::RETRY_AFTER.as_str())
        .expect("a refusal says when to retry")
        .parse()
        .expect("Retry-After is a number of seconds");
    assert!((1..=60).contains(&seconds), "Retry-After: {seconds}");
}

fn redirected(response: &TestResponse) -> Vec<(String, String)> {
    response.assert_status(StatusCode::SEE_OTHER);
    let location = response
        .header(header::LOCATION.as_str())
        .expect("a redirect");
    Url::parse(location)
        .expect("an absolute callback")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

async fn exhaust(client: &TestClient, peer: Option<&str>, forwarded: Option<&str>) {
    for _ in 0..LIMIT {
        not_found(&authorize(client, peer, forwarded).await);
    }
    limited(&authorize(client, peer, forwarded).await);
}

#[tokio::test]
async fn authorizations_from_one_address_are_limited_and_another_address_is_not() {
    let client = client().await;

    exhaust(&client, Some("203.0.113.10"), None).await;

    not_found(&authorize(&client, Some("203.0.113.11"), None).await);
    limited(&authorize(&client, Some("203.0.113.10"), None).await);
}

#[tokio::test]
async fn acs_posts_share_the_limit_and_redirect_with_the_code() {
    let client = client().await;
    let peer = "203.0.113.20";

    not_found(&authorize(&client, Some(peer), None).await);
    assert_eq!(
        redirected(&post_acs(&client, peer).await),
        [
            ("state".to_owned(), "never-issued".to_owned()),
            ("error".to_owned(), STATE_INVALID.to_owned()),
        ]
    );
    not_found(&authorize(&client, Some(peer), None).await);

    assert_eq!(
        redirected(&post_acs(&client, peer).await),
        [("error".to_owned(), RATE_LIMITED.to_owned())],
        "a throttled post is refused before its RelayState is read"
    );
    limited(&authorize(&client, Some(peer), None).await);
    assert_eq!(
        redirected(&post_acs(&client, "203.0.113.21").await)[1],
        ("error".to_owned(), STATE_INVALID.to_owned())
    );
}

#[tokio::test]
async fn forwarded_clients_behind_a_trusted_proxy_get_their_own_buckets() {
    let client = client().await;

    exhaust(&client, Some(LOOPBACK), Some("198.51.100.1")).await;

    not_found(&authorize(&client, Some(LOOPBACK), Some("198.51.100.2")).await);
    limited(&authorize(&client, Some(LOOPBACK), Some("1.2.3.4, 198.51.100.1")).await);
    not_found(&authorize(&client, Some(LOOPBACK), None).await);
}

#[tokio::test]
async fn a_forwarded_header_from_an_untrusted_peer_cannot_dodge_the_limit() {
    let client = client().await;
    let peer = Some("203.0.113.30");

    for client_number in 0..LIMIT {
        let forwarded = format!("198.51.100.{client_number}");
        not_found(&authorize(&client, peer, Some(&forwarded)).await);
    }

    limited(&authorize(&client, peer, Some("198.51.100.99")).await);
    limited(&authorize(&client, peer, None).await);
}

#[tokio::test]
async fn a_lan_peer_is_untrusted_by_default() {
    let client = client().await;

    for client_number in 0..LIMIT {
        let forwarded = format!("198.51.100.{client_number}");
        not_found(&authorize(&client, Some("192.168.1.5"), Some(&forwarded)).await);
    }

    limited(&authorize(&client, Some("192.168.1.5"), Some("198.51.100.200")).await);
    not_found(&authorize(&client, Some("192.168.1.6"), Some("198.51.100.200")).await);
}

#[tokio::test]
async fn an_ipv6_network_shares_one_bucket() {
    let client = client().await;

    for host in 1..=LIMIT {
        not_found(&authorize(&client, Some(&format!("2001:db8:7:7::{host}")), None).await);
    }

    limited(&authorize(&client, Some("2001:db8:7:7:ffff::1"), None).await);
    not_found(&authorize(&client, Some("2001:db8:7:8::1"), None).await);
}

#[tokio::test]
async fn requests_with_no_known_peer_share_one_bucket() {
    let client = client().await;

    exhaust(&client, None, Some("198.51.100.1")).await;

    limited(&authorize(&client, None, Some("198.51.100.2")).await);
    not_found(&authorize(&client, Some("203.0.113.40"), None).await);
}

#[tokio::test]
async fn other_single_sign_on_routes_are_not_limited() {
    let client = client().await;
    let peer = Some("203.0.113.50");

    exhaust(&client, peer, None).await;

    let listing = client
        .send_request(
            connected(
                Request::builder().uri("/api/auth/sso/providers"),
                peer,
                None,
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    listing.assert_status(StatusCode::OK);
}
