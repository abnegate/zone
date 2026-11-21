//! Configuring single sign-on providers: the server admin guard, the license gate, validation,
//! discovery, storage of the secret and the audit log.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::LICENSE_REQUIRED;
use zone_enterprise::sso::{
    CLIENT_ID_INVALID, CLIENT_SECRET_INVALID, DISCOVERY_FAILED, DISPLAY_NAME_INVALID,
    DOMAIN_INVALID, ISSUER_INVALID, PROVIDER_DUPLICATE, PROVIDER_NOT_FOUND,
};
use zone_server::auth::admin_guard::REQUIRED;
use zone_server::auth::entitlement::MESSAGE;
use zone_server::db::audit::{actions, resources};
use zone_server::db::sso::providers;

use crate::common::identity_provider::{
    CLIENT_ID, CLIENT_SECRET, DISCOVERY_PATH, DISPLAY_NAME, IdentityProvider, MISMATCHED_ISSUER,
};
use crate::common::sso::{Account, ENFORCEMENT, Fixture, PROVIDERS, SETTINGS, grace, lapsed};
use crate::common::{REFUSED_URL, TestResponse};

const PREFIX: &str = "sso_providers";
const REPLACED_SECRET: &str = "r0tated:secret+value";

fn assert_code(response: &TestResponse, status: StatusCode, code: &str) {
    response.assert_status(status);
    let body = response.json_value();
    assert_eq!(body["code"], code, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "{body}"
    );
}

fn assert_license_required(response: &TestResponse) {
    assert_code(response, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
    assert_eq!(response.json_value()["error"], MESSAGE);
}

async fn licensed(fixture: &Fixture) -> Account {
    let admin = fixture.register().await;
    fixture.license(&admin).await;
    admin
}

async fn licensed_writes(fixture: &Fixture, token: &str, id: Uuid) -> [TestResponse; 5] {
    [
        fixture
            .create(token, &json!({ "display_name": "Okta" }))
            .await,
        fixture
            .update(token, id, &json!({ "display_name": "Renamed" }))
            .await,
        fixture
            .update(
                token,
                id,
                &json!({ "enabled": false, "require_verified_email": false }),
            )
            .await,
        fixture.update(token, id, &json!({ "enabled": true })).await,
        fixture.enforce(token, true).await,
    ]
}

async fn narrowing_writes(fixture: &Fixture, token: &str, id: Uuid) -> [TestResponse; 3] {
    [
        fixture
            .update(token, id, &json!({ "enabled": false }))
            .await,
        fixture.delete(token, id).await,
        fixture.enforce(token, false).await,
    ]
}

#[tokio::test]
async fn every_route_needs_a_signed_in_server_admin() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let member = fixture.register().await;
    let id = Uuid::new_v4();

    let mut responses = vec![fixture.settings(&member.token).await];
    responses.extend(licensed_writes(&fixture, &member.token, id).await);
    responses.extend(narrowing_writes(&fixture, &member.token, id).await);
    for response in responses {
        response.assert_status(StatusCode::FORBIDDEN);
        assert_eq!(response.json_value(), json!({ "error": REQUIRED }));
    }

    for (method, uri) in [
        (Method::GET, SETTINGS.to_owned()),
        (Method::POST, PROVIDERS.to_owned()),
        (Method::PATCH, format!("{PROVIDERS}/{id}")),
        (Method::DELETE, format!("{PROVIDERS}/{id}")),
        (Method::PUT, ENFORCEMENT.to_owned()),
    ] {
        let response = fixture
            .client
            .send_request(
                Request::builder()
                    .method(method.clone())
                    .uri(&uri)
                    .header("Content-Type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        response.assert_status(StatusCode::UNAUTHORIZED);
    }
    fixture
        .settings(&admin.token)
        .await
        .assert_status(StatusCode::OK);
    fixture.cleanup().await;
}

#[tokio::test]
async fn community_can_read_but_not_change_anything() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;

    let response = fixture.settings(&admin.token).await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({
            "entitled": false,
            "edition": "community",
            "status": "community",
            "required": false,
            "redirect_uri": "http://localhost:3000/sso/callback",
            "providers": [],
        })
    );

    for response in licensed_writes(&fixture, &admin.token, Uuid::new_v4()).await {
        assert_license_required(&response);
    }
    assert_eq!(fixture.providers().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_license_is_checked_before_the_request_is_read() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;

    let unreadable = fixture
        .client
        .send_request(
            Request::builder()
                .method(Method::POST)
                .uri(PROVIDERS)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", admin.token))
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await;
    assert_license_required(&unreadable);
    let malformed_id = fixture.update(&admin.token, "not-a-uuid", &json!({})).await;
    assert_license_required(&malformed_id);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_shows_existing_providers_read_only() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let id = fixture
        .created(&admin.token, &identity_provider.provider())
        .await;

    fixture.store_license(&lapsed()).await;

    let body = fixture.settings(&admin.token).await.json_value();
    assert_eq!(body["entitled"], false, "{body}");
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["status"], "lapsed", "{body}");
    assert_eq!(body["providers"][0]["id"], id.to_string(), "{body}");
    for response in licensed_writes(&fixture, &admin.token, id).await {
        assert_license_required(&response);
    }
    assert_eq!(fixture.providers().await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_still_takes_a_provider_out_of_service() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let id = fixture
        .created(&admin.token, &identity_provider.provider())
        .await;
    fixture.store_license(&lapsed()).await;

    let [disabled, deleted, relaxed] = narrowing_writes(&fixture, &admin.token, id).await;

    disabled.assert_status(StatusCode::OK);
    assert_eq!(disabled.json_value()["enabled"], false);
    deleted.assert_status(StatusCode::NO_CONTENT);
    relaxed.assert_status(StatusCode::OK);
    assert_eq!(fixture.providers().await, 0);
    let missing = fixture
        .update(&admin.token, Uuid::new_v4(), &json!({ "enabled": false }))
        .await;
    assert_code(&missing, StatusCode::NOT_FOUND, PROVIDER_NOT_FOUND);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_license_in_grace_is_entitled() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.register().await;
    fixture.install(&admin, &grace()).await;
    let identity_provider = IdentityProvider::start().await;

    fixture
        .create(&admin.token, &identity_provider.provider())
        .await
        .assert_status(StatusCode::CREATED);

    let body = fixture.settings(&admin.token).await.json_value();
    assert_eq!(body["entitled"], true, "{body}");
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["status"], "grace", "{body}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_admin_adds_a_provider_through_a_loopback_identity_provider() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    assert!(identity_provider.issuer().starts_with("http://127.0.0.1:"));

    let response = fixture
        .create(
            &admin.token,
            &json!({
                "display_name": " Okta ",
                "issuer": identity_provider.issuer(),
                "client_id": CLIENT_ID,
                "client_secret": CLIENT_SECRET,
                "allowed_domains": [" ACME.com", "acme.com", "acme.io"],
                "require_verified_email": false,
                "enabled": false,
            }),
        )
        .await;

    response.assert_status(StatusCode::CREATED);
    let created = response.json_value();
    let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let mut expected = json!({
        "id": id,
        "protocol": "oidc",
        "display_name": "Okta",
        "issuer": identity_provider.issuer(),
        "client_id": CLIENT_ID,
        "allowed_domains": ["acme.com", "acme.io"],
        "require_verified_email": false,
        "enabled": false,
    });
    for timestamp in ["created_at", "updated_at"] {
        assert!(created[timestamp].is_string(), "{created}");
        expected[timestamp] = created[timestamp].clone();
    }
    assert_eq!(created, expected);
    assert_eq!(identity_provider.requests(DISCOVERY_PATH), 1);

    let settings = fixture.settings(&admin.token).await.json_value();
    assert_eq!(settings["entitled"], true, "{settings}");
    assert_eq!(settings["edition"], "enterprise", "{settings}");
    assert_eq!(settings["status"], "active", "{settings}");
    assert_eq!(
        settings["redirect_uri"],
        "http://localhost:3000/sso/callback"
    );
    assert_eq!(settings["providers"], json!([expected]));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_new_provider_is_enabled_and_requires_verified_email_by_default() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;

    let response = fixture
        .create(&admin.token, &identity_provider.provider())
        .await;

    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    assert_eq!(body["display_name"], DISPLAY_NAME);
    assert_eq!(body["enabled"], true);
    assert_eq!(body["require_verified_email"], true);
    assert_eq!(body["allowed_domains"], json!([]));
    fixture.cleanup().await;
}

#[tokio::test]
async fn each_invalid_field_is_refused_with_its_own_code_and_nothing_is_stored() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let valid = identity_provider.provider();
    let with = |field: &str, value: Value| {
        let mut body = valid.clone();
        body[field] = value;
        body
    };
    let without = |field: &str| {
        let mut body = valid.clone();
        body.as_object_mut().unwrap().remove(field);
        body
    };

    let cases = [
        (with("display_name", json!("  ")), DISPLAY_NAME_INVALID),
        (
            with("display_name", json!("x".repeat(65))),
            DISPLAY_NAME_INVALID,
        ),
        (without("display_name"), DISPLAY_NAME_INVALID),
        (with("issuer", json!("ftp://idp.acme.test")), ISSUER_INVALID),
        (
            with("issuer", json!("https://idp.acme.test?tenant=1")),
            ISSUER_INVALID,
        ),
        (
            with("issuer", json!("http://intranet.acme.test")),
            ISSUER_INVALID,
        ),
        (with("client_id", json!("")), CLIENT_ID_INVALID),
        (with("client_id", json!("x".repeat(256))), CLIENT_ID_INVALID),
        (with("client_secret", json!("")), CLIENT_SECRET_INVALID),
        (without("client_secret"), CLIENT_SECRET_INVALID),
        (
            with("allowed_domains", json!(["*.acme.com"])),
            DOMAIN_INVALID,
        ),
        (
            with("allowed_domains", json!(["jane@acme.com"])),
            DOMAIN_INVALID,
        ),
    ];
    for (body, code) in cases {
        let response = fixture.create(&admin.token, &body).await;
        assert_code(&response, StatusCode::BAD_REQUEST, code);
    }
    assert_eq!(fixture.providers().await, 0);
    assert_eq!(identity_provider.requests(DISCOVERY_PATH), 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_discovery_document_naming_another_issuer_is_refused() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    identity_provider.mismatch_issuer(true);

    let response = fixture
        .create(&admin.token, &identity_provider.provider())
        .await;

    assert_code(&response, StatusCode::BAD_REQUEST, DISCOVERY_FAILED);
    let message = response.json_value()["error"].as_str().unwrap().to_owned();
    assert!(message.contains(MISMATCHED_ISSUER), "{message}");
    assert_eq!(fixture.providers().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unreachable_issuer_is_a_discovery_failure() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;

    let response = fixture
        .create(
            &admin.token,
            &json!({
                "display_name": "Okta", "issuer": REFUSED_URL,
                "client_id": CLIENT_ID, "client_secret": CLIENT_SECRET,
            }),
        )
        .await;

    assert_code(&response, StatusCode::BAD_REQUEST, DISCOVERY_FAILED);
    assert_eq!(fixture.providers().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_same_issuer_and_client_id_cannot_be_added_twice() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    fixture
        .created(&admin.token, &identity_provider.provider())
        .await;

    let again = fixture
        .create(&admin.token, &identity_provider.provider())
        .await;
    assert_code(&again, StatusCode::CONFLICT, PROVIDER_DUPLICATE);

    let mut other = identity_provider.provider();
    other["client_id"] = json!("another-client");
    let second = fixture.created(&admin.token, &other).await;
    let renamed = fixture
        .update(&admin.token, second, &json!({ "client_id": CLIENT_ID }))
        .await;
    assert_code(&renamed, StatusCode::CONFLICT, PROVIDER_DUPLICATE);
    assert_eq!(fixture.providers().await, 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_sparse_update_keeps_every_field_it_leaves_out_and_the_secret() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let id = fixture
        .created(&admin.token, &identity_provider.provider())
        .await;
    let discoveries = identity_provider.requests(DISCOVERY_PATH);

    let response = fixture
        .update(&admin.token, id, &json!({ "display_name": "Okta SSO" }))
        .await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["display_name"], "Okta SSO");
    assert_eq!(body["issuer"], identity_provider.issuer());
    assert_eq!(body["client_id"], CLIENT_ID);
    assert_eq!(body["enabled"], true);
    assert_eq!(body["require_verified_email"], true);
    assert_eq!(identity_provider.requests(DISCOVERY_PATH), discoveries);
    let stored = providers::get(fixture.pool(), id).await.unwrap().unwrap();
    assert_eq!(
        stored
            .client_secret(fixture.state().master_key())
            .unwrap()
            .expose(),
        CLIENT_SECRET
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_update_validates_the_fields_it_carries() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let id = fixture
        .created(&admin.token, &identity_provider.provider())
        .await;

    for (body, code) in [
        (json!({ "display_name": "" }), DISPLAY_NAME_INVALID),
        (
            json!({ "issuer": "http://intranet.acme.test" }),
            ISSUER_INVALID,
        ),
        (json!({ "client_id": " " }), CLIENT_ID_INVALID),
        (json!({ "client_secret": "" }), CLIENT_SECRET_INVALID),
        (json!({ "allowed_domains": ["*"] }), DOMAIN_INVALID),
    ] {
        let response = fixture.update(&admin.token, id, &body).await;
        assert_code(&response, StatusCode::BAD_REQUEST, code);
    }
    let body = fixture.settings(&admin.token).await.json_value();
    assert_eq!(body["providers"][0]["display_name"], DISPLAY_NAME, "{body}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn changing_the_issuer_rediscovers_it() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let first = IdentityProvider::start().await;
    let second = IdentityProvider::start().await;
    let id = fixture.created(&admin.token, &first.provider()).await;

    second.mismatch_issuer(true);
    let refused = fixture
        .update(&admin.token, id, &json!({ "issuer": second.issuer() }))
        .await;
    assert_code(&refused, StatusCode::BAD_REQUEST, DISCOVERY_FAILED);

    second.mismatch_issuer(false);
    let moved = fixture
        .update(&admin.token, id, &json!({ "issuer": second.issuer() }))
        .await;
    moved.assert_status(StatusCode::OK);
    assert_eq!(moved.json_value()["issuer"], second.issuer());
    assert_eq!(second.requests(DISCOVERY_PATH), 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unknown_provider_is_not_found() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let unknown = Uuid::new_v4();

    for response in [
        fixture
            .update(&admin.token, unknown, &json!({ "display_name": "Okta" }))
            .await,
        fixture.delete(&admin.token, unknown).await,
        fixture
            .update(
                &admin.token,
                "not-a-uuid",
                &json!({ "display_name": "Okta" }),
            )
            .await,
        fixture.delete(&admin.token, "not-a-uuid").await,
    ] {
        assert_code(&response, StatusCode::NOT_FOUND, PROVIDER_NOT_FOUND);
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deleted_provider_is_gone() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let id = fixture
        .created(&admin.token, &identity_provider.provider())
        .await;

    let response = fixture.delete(&admin.token, id).await;

    response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.providers().await, 0);
    let again = fixture.delete(&admin.token, id).await;
    assert_code(&again, StatusCode::NOT_FOUND, PROVIDER_NOT_FOUND);
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_change_is_audited_without_the_secret() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;
    let id = fixture
        .created(&admin.token, &identity_provider.provider())
        .await;
    let recorded = json!({
        "protocol": "oidc",
        "display_name": DISPLAY_NAME,
        "issuer": identity_provider.issuer(),
        "client_id": CLIENT_ID,
        "allowed_domains": [],
        "require_verified_email": true,
        "enabled": true,
    });

    let created = fixture.audits(actions::SSO_PROVIDER_CREATED).await;
    assert_eq!(created.len(), 1);
    let mut expected = recorded.clone();
    expected["client_secret_changed"] = json!(true);
    assert_eq!(created[0].old_values, None);
    assert_eq!(created[0].new_values, Some(expected));

    let renamed = fixture
        .update(
            &admin.token,
            id,
            &json!({ "display_name": "Okta SSO", "enabled": false, "issuer": identity_provider.issuer() }),
        )
        .await;
    renamed.assert_status(StatusCode::OK);
    let rotated = fixture
        .update(
            &admin.token,
            id,
            &json!({ "client_secret": REPLACED_SECRET }),
        )
        .await;
    rotated.assert_status(StatusCode::OK);
    let unchanged = fixture
        .update(&admin.token, id, &json!({ "display_name": "Okta SSO" }))
        .await;
    unchanged.assert_status(StatusCode::OK);

    let updated = fixture.audits(actions::SSO_PROVIDER_UPDATED).await;
    assert_eq!(
        updated.len(),
        2,
        "a change that changes nothing is not audited"
    );
    assert_eq!(
        updated[0].old_values,
        Some(json!({ "display_name": DISPLAY_NAME, "enabled": true }))
    );
    assert_eq!(
        updated[0].new_values,
        Some(json!({ "display_name": "Okta SSO", "enabled": false }))
    );
    assert_eq!(updated[1].old_values, Some(json!({})));
    assert_eq!(
        updated[1].new_values,
        Some(json!({ "client_secret_changed": true }))
    );
    let stored = providers::get(fixture.pool(), id).await.unwrap().unwrap();
    assert_eq!(
        stored
            .client_secret(fixture.state().master_key())
            .unwrap()
            .expose(),
        REPLACED_SECRET
    );

    fixture
        .delete(&admin.token, id)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let deleted = fixture.audits(actions::SSO_PROVIDER_DELETED).await;
    assert_eq!(deleted.len(), 1);
    let mut former = recorded;
    former["display_name"] = json!("Okta SSO");
    former["enabled"] = json!(false);
    assert_eq!(deleted[0].old_values, Some(former));
    assert_eq!(deleted[0].new_values, None);

    for audit in created.iter().chain(&updated).chain(&deleted) {
        assert_eq!(audit.actor_id, Some(admin.id));
        assert_eq!(audit.organization_id, None);
        assert_eq!(audit.resource_type, resources::SSO_PROVIDER);
        assert_eq!(audit.resource_id, Some(id));
    }
    let text = fixture.audit_text().await;
    for secret in [CLIENT_SECRET, REPLACED_SECRET] {
        assert!(
            !text.contains(secret),
            "the audit log holds a client secret"
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn no_response_ever_carries_the_secret() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = licensed(&fixture).await;
    let identity_provider = IdentityProvider::start().await;

    let created = fixture
        .create(&admin.token, &identity_provider.provider())
        .await;
    let id = created.json_value()["id"].as_str().unwrap().to_owned();
    let responses = [
        created,
        fixture
            .update(
                &admin.token,
                &id,
                &json!({ "client_secret": REPLACED_SECRET }),
            )
            .await,
        fixture.settings(&admin.token).await,
        fixture.delete(&admin.token, &id).await,
    ];
    for response in responses {
        let text = response.text();
        for secret in [CLIENT_SECRET, REPLACED_SECRET] {
            assert!(!text.contains(secret), "{text}");
        }
        assert!(!text.contains("client_secret"), "{text}");
    }
    fixture.cleanup().await;
}
