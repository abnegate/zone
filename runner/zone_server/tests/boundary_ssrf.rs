//! Outbound-fetch boundaries: every spelling of a private target a caller can
//! put in a URL, and the redirect hop that carries a validated fetch somewhere
//! the validator would have refused.
mod common;

use axum::http::StatusCode;
use common::{TestClient, test_email, test_password};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_server::utils::url::validate_public_url;

async fn workspace(client: &TestClient) -> (String, Uuid) {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let token = registered["access_token"]
        .as_str()
        .unwrap_or_else(|| panic!("registration failed: {registered}"))
        .to_string();
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Boundary", "slug": format!("boundary-{}", Uuid::new_v4())}),
            &token,
        )
        .await
        .json_value();
    let organization = organization["organization"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("organization creation failed: {organization}"));
    let created = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Boundary", "slug": format!("boundary-{}", Uuid::new_v4())}),
            &token,
        )
        .await
        .json_value();
    let workspace = Uuid::parse_str(
        created["workspace"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("workspace creation failed: {created}")),
    )
    .unwrap();
    (token, workspace)
}

/// Knowledge ingestion fetches a URL the caller chose, so every alternative
/// spelling of a loopback, private, or metadata address has to be refused
/// before a request is issued rather than after one comes back.
#[tokio::test]
async fn knowledge_ingestion_refuses_every_private_address_spelling() {
    let client = TestClient::with_db().await;
    let (token, workspace) = workspace(&client).await;

    for url in [
        "http://127.0.0.1/secret",
        "http://127.1/secret",
        "http://127.0.1/secret",
        "http://2130706433/secret",
        "http://0x7f000001/secret",
        "http://017700000001/secret",
        "http://0/secret",
        "http://0.0.0.0/secret",
        "http://[::1]/secret",
        "http://[::]/secret",
        "http://[::ffff:127.0.0.1]/secret",
        "http://[0:0:0:0:0:ffff:7f00:1]/secret",
        "http://[fd00::1]/secret",
        "http://[fe80::1]/secret",
        "http://127.0.0.1./secret",
        "http://localhost/secret",
        "http://LOCALHOST./secret",
        "http://anything.localhost/secret",
        "http://printer.local/secret",
        "http://vault.internal/secret",
        "http://metadata.google.internal/computeMetadata/v1beta1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::ffff:169.254.169.254]/latest/meta-data/",
        "http://10.1.2.3/secret",
        "http://172.16.9.9/secret",
        "http://192.168.0.1/secret",
        "http://169.254.1.1/secret",
        "http://user:password@example.com/secret",
        "file:///etc/passwd",
        "gopher://127.0.0.1:11211/_stats",
        "ftp://127.0.0.1/secret",
        "http://127。0。0。1/secret",
    ] {
        let response = client
            .post_json_auth(
                "/api/knowledge",
                &json!({
                    "workspace_id": workspace,
                    "title": format!("Ingest {url}"),
                    "source_url": url,
                }),
                &token,
            )
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{url} was accepted for ingestion: {}",
            response.text()
        );
        assert!(
            response.text().contains("allowed"),
            "{url} reached the fetch instead of the URL guard: {}",
            response.text()
        );
    }
}

/// The same guard, called directly, so a spelling that only the parser can tell
/// apart is on record next to the route that relies on it.
#[test]
fn the_url_guard_refuses_alternative_private_spellings() {
    for url in [
        "http://127.1/",
        "http://2130706433/",
        "http://0x7f000001/",
        "http://017700000001/",
        "http://0/",
        "http://[::]/",
        "http://[0:0:0:0:0:ffff:7f00:1]/",
        "http://127。0。0。1/",
        "http://ANYTHING.LocalHost/",
        "http://vault.INTERNAL./",
    ] {
        assert!(
            validate_public_url(url).is_err(),
            "{url} names a private target and must be refused"
        );
    }
}

/// A validated URL is only the first hop. A redirect chain must be re-checked
/// at every hop, because the host that answers is the host that matters.
#[tokio::test]
async fn a_redirect_into_a_private_host_is_never_requested() {
    let private = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string("INTERNAL-ONLY"))
        .mount(&private)
        .await;

    let public = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/secret", private.uri()).as_str()),
        )
        .mount(&public)
        .await;

    let response = zone_server::utils::url::public_client(std::time::Duration::from_secs(5))
        .expect("the validated fetch client builds")
        .get(format!("{}/redirect", public.uri()))
        .send()
        .await;

    assert!(
        private
            .received_requests()
            .await
            .is_none_or(|requests| requests.is_empty()),
        "the redirect was followed into {}, a host the validator refuses",
        private.uri()
    );
    if let Ok(response) = response {
        let body = response.text().await.unwrap_or_default();
        assert!(
            !body.contains("INTERNAL-ONLY"),
            "the body of a refused host was returned to the caller"
        );
    }
}

/// A page far larger than the cap must not be read into memory before the cap
/// is applied, and a page inside the cap must still be read whole.
#[tokio::test]
async fn a_page_is_read_only_up_to_its_cap() {
    const CAP: usize = 4096;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/huge"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'a'; 64 * 1024 * 1024]))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/small"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'b'; CAP]))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let huge = client
        .get(format!("{}/huge", server.uri()))
        .send()
        .await
        .expect("the oversized page responds");
    let huge = zone_server::utils::url::read_capped(huge, CAP).await;
    assert!(
        huge.is_err(),
        "a {} byte page must be refused against a {CAP} byte cap",
        64 * 1024 * 1024
    );

    let small = client
        .get(format!("{}/small", server.uri()))
        .send()
        .await
        .expect("the small page responds");
    let small = zone_server::utils::url::read_capped(small, CAP)
        .await
        .expect("a page inside the cap is read whole");
    assert_eq!(small.len(), CAP);
}
