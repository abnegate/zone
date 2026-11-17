use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::LICENSE_REQUIRED;
use zone_enterprise::Tier;
use zone_enterprise::audit::{
    BUCKET_INVALID, CERTIFICATE_INVALID, DESTINATION_DUPLICATE, DESTINATION_NOT_FOUND,
    FACILITY_INVALID, HOST_INVALID, HOSTNAME_INVALID, KIND_INVALID, NAME_INVALID, PORT_INVALID,
    PREFIX_INVALID, REGION_INVALID, SECRET_INVALID, URL_INVALID,
};
use zone_server::config::Config;
use zone_server::db::audit::actions;
use zone_server::db::organization_members::OrgRole;
use zone_server::routes::audit::refusal::BODY_INVALID;
use zone_server::services::hosts::Hosts;
use zone_server::state::AppState;

use crate::common::audit::{UNVERIFIABLE, refused, settle};
use crate::common::policy::Fixture;
use crate::common::sso::{self, Account};
use crate::common::{TestClient, TestResponse, test_config};

const PREFIX: &str = "audit_destinations";
const SERVER: &str = "/api/audit-destinations";
const SIGNING_SECRET: &str = "whsec-0123456789abcdef";
const ACCESS_KEY_ID: &str = "AKIAEXAMPLESECRETID";
const SECRET_ACCESS_KEY: &str = "wJalrXUtnFEMI-secret-access-key";
const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";

fn destinations(organization: Uuid) -> String {
    format!("/api/organizations/{organization}/audit-destinations")
}

fn destination(organization: Uuid, id: impl std::fmt::Display) -> String {
    format!("{}/{id}", destinations(organization))
}

fn webhook(name: &str, url: &str) -> Value {
    json!({
        "name": name,
        "settings": {"kind": "webhook", "url": url},
        "secrets": {"signing_secret": SIGNING_SECRET},
    })
}

fn s3(name: &str) -> Value {
    json!({
        "name": name,
        "settings": {
            "kind": "s3", "bucket": "zone-audit", "region": "us-east-1",
            "endpoint": "http://minio:9000", "prefix": "audit", "addressing": "path"
        },
        "secrets": {"access_key_id": ACCESS_KEY_ID, "secret_access_key": SECRET_ACCESS_KEY},
    })
}

fn syslog(settings: Value, secrets: Value) -> Value {
    let mut settings = settings;
    settings["kind"] = json!("syslog");
    json!({"name": "Syslog", "settings": settings, "secrets": secrets})
}

fn id(response: &TestResponse) -> Uuid {
    response.json_value()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

async fn post(client: &TestClient, uri: &str, account: &Account, body: &Value) -> TestResponse {
    client.post_json_auth(uri, body, &account.token).await
}

async fn delivery_attempts(pool: &PgPool, id: Uuid) -> (i32, Option<i64>) {
    sqlx::query_as(
        "SELECT attempts, batch_sequence FROM audit_deliveries WHERE destination_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn fail_delivery(pool: &PgPool, id: Uuid, attempts: i32) {
    sqlx::query(
        "UPDATE audit_deliveries SET attempts = $2, batch_transaction_id = 1, batch_sequence = 1, \
             last_error_code = 'status', last_error = 'HTTP 503', last_failure_at = now(), \
             next_attempt_at = now() + interval '1 hour' \
         WHERE destination_id = $1",
    )
    .bind(id)
    .bind(attempts)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn community_and_lapsed_licenses_cannot_configure_but_can_always_disable_and_delete() {
    let fixture = Fixture::new(PREFIX).await;
    let client = &fixture.sso.client;
    let path = destinations(fixture.organization);
    let owner = &fixture.owner;

    let unlicensed = post(
        client,
        &path,
        owner,
        &webhook("SIEM", "http://10.0.0.5/hook"),
    )
    .await;
    refused(&unlicensed, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
    let listed = client.get_auth(&path, &owner.token).await;
    listed.assert_status(StatusCode::OK);
    let listed = listed.json_value();
    assert_eq!(listed["standing"]["pause"], "unlicensed");
    assert_eq!(listed["standing"]["streaming"], false);
    assert_eq!(listed["destinations"], json!([]));

    fixture.license(Tier::Enterprise).await;
    let created = post(
        client,
        &path,
        owner,
        &webhook("SIEM", "http://10.0.0.5/hook"),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let siem = id(&created);
    let archive = id(&post(client, &path, owner, &s3("Archive")).await);

    fixture.store_license(&sso::lapsed(Tier::Enterprise)).await;
    let listed = client.get_auth(&path, &owner.token).await.json_value();
    assert_eq!(listed["standing"]["pause"], "lapsed");
    assert_eq!(listed["destinations"][0]["delivery"]["health"], "paused");
    assert_eq!(
        listed["destinations"][0]["delivery"]["next_attempt_at"],
        Value::Null
    );
    refused(
        &post(
            client,
            &path,
            owner,
            &webhook("Other", "http://10.0.0.6/hook"),
        )
        .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    for body in [
        json!({"name": "Renamed"}),
        json!({"enabled": true}),
        json!({"enabled": false, "name": "Renamed"}),
        json!({"secrets": {"signing_secret": "rotated"}}),
    ] {
        refused(
            &client
                .patch_json_auth(
                    &destination(fixture.organization, siem),
                    &body,
                    &owner.token,
                )
                .await,
            StatusCode::FORBIDDEN,
            LICENSE_REQUIRED,
        );
    }
    let disabled = client
        .patch_json_auth(
            &destination(fixture.organization, siem),
            &json!({"enabled": false}),
            &owner.token,
        )
        .await;
    disabled.assert_status(StatusCode::OK);
    let disabled = disabled.json_value();
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["delivery"]["health"], "disabled");

    fixture.store_license(UNVERIFIABLE).await;
    let listed = client.get_auth(&path, &owner.token).await.json_value();
    assert_eq!(listed["standing"]["pause"], "unverified");
    refused(
        &post(
            client,
            &path,
            owner,
            &webhook("Other", "http://10.0.0.6/hook"),
        )
        .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );

    fixture.sso.remove_license(owner).await;
    for id in [siem, archive] {
        client
            .delete_auth(&destination(fixture.organization, id), &owner.token)
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    let listed = client.get_auth(&path, &owner.token).await.json_value();
    assert_eq!(listed["destinations"], json!([]));
    assert_eq!(
        fixture
            .audits(actions::AUDIT_DESTINATION_DELETED)
            .await
            .len(),
        2
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn team_licenses_export_but_do_not_stream() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Team).await;
    let client = &fixture.sso.client;
    let path = destinations(fixture.organization);

    refused(
        &post(
            client,
            &path,
            &fixture.owner,
            &webhook("SIEM", "https://siem.example.com/hook"),
        )
        .await,
        StatusCode::FORBIDDEN,
        LICENSE_REQUIRED,
    );
    let standing = client
        .get_auth(&path, &fixture.owner.token)
        .await
        .json_value()["standing"]
        .clone();
    assert_eq!(standing["edition"], "team");
    assert_eq!(standing["export"], true);
    assert_eq!(standing["streaming"], false);
    assert_eq!(standing["pause"], "edition");
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_admins_manage_destinations_and_scopes_never_mix() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;
    let client = &fixture.sso.client;
    let path = destinations(fixture.organization);
    let owner = &fixture.owner;

    let admin = fixture.member(OrgRole::Admin, None).await;
    let created = post(
        client,
        &path,
        &admin,
        &webhook("SIEM", "http://splunk:8088/services/collector"),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let siem = id(&created);

    let member = fixture.member(OrgRole::Member, None).await;
    let outsider = fixture.stranger().await;
    for (account, status) in [
        (&member, StatusCode::FORBIDDEN),
        (&outsider, StatusCode::NOT_FOUND),
    ] {
        client
            .get_auth(&path, &account.token)
            .await
            .assert_status(status);
        post(
            client,
            &path,
            account,
            &webhook("Mine", "http://10.0.0.7/hook"),
        )
        .await
        .assert_status(status);
        client
            .get_auth(&destination(fixture.organization, siem), &account.token)
            .await
            .assert_status(status);
        client
            .patch_json_auth(
                &destination(fixture.organization, siem),
                &json!({"enabled": false}),
                &account.token,
            )
            .await
            .assert_status(status);
        client
            .delete_auth(&destination(fixture.organization, siem), &account.token)
            .await
            .assert_status(status);
        client
            .get_auth(SERVER, &account.token)
            .await
            .assert_status(StatusCode::FORBIDDEN);
        post(
            client,
            SERVER,
            account,
            &webhook("Server", "http://10.0.0.8/hook"),
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);
    }

    refused(
        &post(
            client,
            &path,
            owner,
            &webhook("SIEM", "http://10.0.0.9/hook"),
        )
        .await,
        StatusCode::CONFLICT,
        DESTINATION_DUPLICATE,
    );
    let (other, _) = fixture.another_organization(owner).await;
    let elsewhere = post(
        client,
        &destinations(other),
        owner,
        &webhook("SIEM", "http://10.0.0.9/hook"),
    )
    .await;
    elsewhere.assert_status(StatusCode::CREATED);
    let elsewhere = id(&elsewhere);
    for uri in [
        destination(fixture.organization, elsewhere),
        destination(fixture.organization, "not-a-uuid"),
        format!("{SERVER}/{siem}"),
    ] {
        refused(
            &client.get_auth(&uri, &owner.token).await,
            StatusCode::NOT_FOUND,
            DESTINATION_NOT_FOUND,
        );
    }
    refused(
        &client
            .delete_auth(&destination(fixture.organization, elsewhere), &owner.token)
            .await,
        StatusCode::NOT_FOUND,
        DESTINATION_NOT_FOUND,
    );

    let server = post(
        client,
        SERVER,
        owner,
        &webhook("SIEM", "http://10.0.0.10/hook"),
    )
    .await;
    server.assert_status(StatusCode::CREATED);
    let server = id(&server);
    refused(
        &post(
            client,
            SERVER,
            owner,
            &webhook("SIEM", "http://10.0.0.11/hook"),
        )
        .await,
        StatusCode::CONFLICT,
        DESTINATION_DUPLICATE,
    );
    let listed = client.get_auth(SERVER, &owner.token).await;
    listed.assert_status(StatusCode::OK);
    let names: Vec<Value> = listed.json_value()["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|destination| destination["id"].clone())
        .collect();
    assert_eq!(names, vec![json!(server)]);
    let organization: Vec<Value> =
        client.get_auth(&path, &owner.token).await.json_value()["destinations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|destination| destination["id"].clone())
            .collect();
    assert_eq!(organization, vec![json!(siem)]);
    refused(
        &client
            .get_auth(&destination(fixture.organization, server), &owner.token)
            .await,
        StatusCode::NOT_FOUND,
        DESTINATION_NOT_FOUND,
    );

    let created = fixture.audits(actions::AUDIT_DESTINATION_CREATED).await;
    let scopes: Vec<(Option<Uuid>, Option<Uuid>)> = created
        .iter()
        .map(|row| (row.organization_id, row.resource_id))
        .collect();
    assert_eq!(
        scopes,
        vec![
            (Some(fixture.organization), Some(siem)),
            (Some(other), Some(elsewhere)),
            (None, Some(server)),
        ]
    );
    assert!(
        created
            .iter()
            .all(|row| row.resource_type == "audit_destination")
    );
    client
        .delete_auth(&format!("{SERVER}/{server}"), &owner.token)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    fixture.cleanup().await;
}

#[tokio::test]
async fn secrets_are_sealed_and_only_their_names_are_ever_shown() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;
    let client = &fixture.sso.client;
    let path = destinations(fixture.organization);
    let owner = &fixture.owner;

    let created = post(
        client,
        &path,
        owner,
        &webhook("SIEM", "https://siem.example.com/hook"),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let siem = id(&created);
    let body = created.json_value();
    assert_eq!(body["kind"], "webhook");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["secrets"], json!(["signing_secret"]));
    assert_eq!(
        body["settings"],
        json!({"kind": "webhook", "url": "https://siem.example.com/hook"})
    );
    let archive = post(client, &path, owner, &s3("Archive")).await;
    archive.assert_status(StatusCode::CREATED);
    let archive_body = archive.json_value();
    assert_eq!(
        archive_body["secrets"],
        json!(["access_key_id", "secret_access_key"])
    );
    assert_eq!(archive_body["settings"]["prefix"], "audit/");
    let feed = post(
        client,
        &path,
        owner,
        &syslog(
            json!({"host": "siem.lan", "port": 6514, "protocol": "tls"}),
            json!({"ca_certificate": CERTIFICATE}),
        ),
    )
    .await;
    feed.assert_status(StatusCode::CREATED);
    let feed_body = feed.json_value();
    assert_eq!(feed_body["secrets"], json!(["ca_certificate"]));
    assert_eq!(feed_body["settings"]["facility"], 13);
    assert_eq!(feed_body["settings"]["hostname"], "zone");
    let feed = id(&feed);

    let rotated = client
        .patch_json_auth(
            &destination(fixture.organization, siem),
            &json!({"secrets": {"signing_secret": "whsec-rotated-value"}}),
            &owner.token,
        )
        .await;
    rotated.assert_status(StatusCode::OK);
    let cleared = client
        .patch_json_auth(
            &destination(fixture.organization, feed),
            &json!({"secrets": {"ca_certificate": ""}}),
            &owner.token,
        )
        .await;
    cleared.assert_status(StatusCode::OK);
    assert_eq!(cleared.json_value()["secrets"], json!([]));
    let unchanged = client
        .patch_json_auth(
            &destination(fixture.organization, siem),
            &json!({"secrets": {"signing_secret": "whsec-rotated-value"}, "name": "SIEM"}),
            &owner.token,
        )
        .await;
    unchanged.assert_status(StatusCode::OK);

    let updates = fixture.audits(actions::AUDIT_DESTINATION_UPDATED).await;
    assert_eq!(updates.len(), 2, "an unchanged update is not audited");
    assert_eq!(
        updates[0].new_values,
        Some(json!({"secrets_changed": ["signing_secret"]}))
    );
    assert_eq!(updates[0].old_values, Some(json!({})));
    assert_eq!(
        updates[1].new_values,
        Some(json!({"secrets_changed": ["ca_certificate"]}))
    );
    let created = fixture.audits(actions::AUDIT_DESTINATION_CREATED).await;
    assert_eq!(
        created[0].new_values,
        Some(json!({
            "name": "SIEM",
            "kind": "webhook",
            "enabled": true,
            "settings": {"kind": "webhook", "url": "https://siem.example.com/hook"},
            "secrets_changed": ["signing_secret"],
        }))
    );

    let listed = client.get_auth(&path, &owner.token).await.text();
    let shown = client
        .get_auth(
            &destination(fixture.organization, archive_body["id"].as_str().unwrap()),
            &owner.token,
        )
        .await
        .text();
    let audited = fixture.sso.audit_text().await;
    let stored: Vec<String> = sqlx::query_scalar("SELECT secrets FROM audit_destinations")
        .fetch_all(fixture.pool())
        .await
        .unwrap();
    for secret in [
        SIGNING_SECRET,
        "whsec-rotated-value",
        ACCESS_KEY_ID,
        SECRET_ACCESS_KEY,
        "MIIB",
    ] {
        for (place, text) in [
            ("a create response", body.to_string()),
            ("the list", listed.clone()),
            ("a destination", shown.clone()),
            ("an update response", rotated.text()),
            ("the audit log", audited.clone()),
        ] {
            assert!(!text.contains(secret), "{place} shows {secret}: {text}");
        }
        assert!(stored.iter().all(|sealed| !sealed.contains(secret)));
    }
    assert!(stored.iter().all(|sealed| sealed.starts_with("ENC[")));
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_invalid_field_is_refused_with_its_own_code() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;
    let client = &fixture.sso.client;
    let path = destinations(fixture.organization);
    let owner = &fixture.owner;

    for url in [
        "http://10.0.0.5/hook",
        "http://splunk:8088/services/collector",
    ] {
        post(client, &path, owner, &webhook(url, url))
            .await
            .assert_status(StatusCode::CREATED);
    }
    let cases = [
        (webhook("  ", "https://siem.example.com/hook"), NAME_INVALID),
        (
            json!({"name": "SIEM", "settings": {"kind": "smtp"}}),
            KIND_INVALID,
        ),
        (json!({"name": "SIEM"}), KIND_INVALID),
        (webhook("Metadata", "http://169.254.169.254/"), URL_INVALID),
        (
            webhook("Userinfo", "https://user:pass@siem.example.com/hook"),
            URL_INVALID,
        ),
        (
            webhook("Query", "https://siem.example.com/hook?token=1"),
            URL_INVALID,
        ),
        (
            webhook("Scheme", "ftp://siem.example.com/hook"),
            URL_INVALID,
        ),
        (
            json!({"name": "S3", "settings": {"kind": "s3", "bucket": "No_Such", "region": "us-east-1"},
                   "secrets": {"access_key_id": "a", "secret_access_key": "b"}}),
            BUCKET_INVALID,
        ),
        (
            json!({"name": "S3", "settings": {"kind": "s3", "bucket": "audit", "region": "US East"},
                   "secrets": {"access_key_id": "a", "secret_access_key": "b"}}),
            REGION_INVALID,
        ),
        (
            json!({"name": "S3", "settings": {"kind": "s3", "bucket": "audit", "region": "us-east-1", "prefix": "a/../b"},
                   "secrets": {"access_key_id": "a", "secret_access_key": "b"}}),
            PREFIX_INVALID,
        ),
        (
            json!({"name": "S3", "settings": {"kind": "s3", "bucket": "audit", "region": "us-east-1",
                   "endpoint": "http://169.254.169.254"},
                   "secrets": {"access_key_id": "a", "secret_access_key": "b"}}),
            URL_INVALID,
        ),
        (
            syslog(
                json!({"host": "https://siem", "port": 514, "protocol": "udp"}),
                json!({}),
            ),
            HOST_INVALID,
        ),
        (
            syslog(
                json!({"host": "169.254.169.254", "port": 514, "protocol": "udp"}),
                json!({}),
            ),
            HOST_INVALID,
        ),
        (
            syslog(
                json!({"host": "siem", "port": 0, "protocol": "udp"}),
                json!({}),
            ),
            PORT_INVALID,
        ),
        (
            syslog(
                json!({"host": "siem", "port": 65536, "protocol": "udp"}),
                json!({}),
            ),
            PORT_INVALID,
        ),
        (
            syslog(
                json!({"host": "siem", "port": 514, "protocol": "udp", "facility": 24}),
                json!({}),
            ),
            FACILITY_INVALID,
        ),
        (
            syslog(
                json!({"host": "siem", "port": 514, "protocol": "udp", "hostname": "zone host"}),
                json!({}),
            ),
            HOSTNAME_INVALID,
        ),
        (
            json!({"name": "SIEM", "settings": {"kind": "webhook", "url": "https://siem.example.com/hook"}}),
            SECRET_INVALID,
        ),
        (
            json!({"name": "SIEM", "settings": {"kind": "webhook", "url": "https://siem.example.com/hook"},
                   "secrets": {"access_key_id": "a"}}),
            SECRET_INVALID,
        ),
        (
            syslog(
                json!({"host": "siem", "port": 514, "protocol": "udp"}),
                json!({"ca_certificate": CERTIFICATE}),
            ),
            CERTIFICATE_INVALID,
        ),
        (
            syslog(
                json!({"host": "siem", "port": 6514, "protocol": "tls"}),
                json!({"ca_certificate": "garbage"}),
            ),
            CERTIFICATE_INVALID,
        ),
        (
            json!({"name": "SIEM", "settings": {"kind": "webhook", "url": "https://siem.example.com/hook"},
                   "secrets": {"signing_secret": "x"}, "extra": true}),
            BODY_INVALID,
        ),
    ];
    for (body, code) in cases {
        let response = post(client, &path, owner, &body).await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            response.text()
        );
        assert_eq!(
            response.json_value()["code"],
            code,
            "{body}: {}",
            response.text()
        );
    }
    let unreadable = client
        .send_request(
            axum::http::Request::post(&path)
                .header("authorization", format!("Bearer {}", owner.token))
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{"))
                .unwrap(),
        )
        .await;
    refused(&unreadable, StatusCode::BAD_REQUEST, BODY_INVALID);

    let siem = id(&post(
        client,
        &path,
        owner,
        &webhook("SIEM", "https://siem.example.com/hook"),
    )
    .await);
    for (body, code) in [
        (
            json!({"settings": {"kind": "s3", "bucket": "audit", "region": "us-east-1"}}),
            KIND_INVALID,
        ),
        (
            json!({"settings": {"kind": "webhook", "url": "http://169.254.169.254/"}}),
            URL_INVALID,
        ),
        (json!({"secrets": {"signing_secret": ""}}), SECRET_INVALID),
        (json!({"name": ""}), NAME_INVALID),
    ] {
        refused(
            &client
                .patch_json_auth(
                    &destination(fixture.organization, siem),
                    &body,
                    &owner.token,
                )
                .await,
            StatusCode::BAD_REQUEST,
            code,
        );
    }
    let feed = id(&post(
        client,
        &path,
        owner,
        &syslog(
            json!({"host": "siem", "port": 6514, "protocol": "tls"}),
            json!({"ca_certificate": CERTIFICATE}),
        ),
    )
    .await);
    refused(
        &client
            .patch_json_auth(
                &destination(fixture.organization, feed),
                &json!({"settings": {"kind": "syslog", "host": "siem", "port": 514, "protocol": "udp"}}),
                &owner.token,
            )
            .await,
        StatusCode::BAD_REQUEST,
        CERTIFICATE_INVALID,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn zone_endpoint_hosts_restricts_where_destinations_send() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;
    let restricted = AppState::new_with_mail(
        Config {
            endpoint_hosts: Hosts::parse("allowed.example"),
            ..test_config()
        },
        fixture.pool().clone(),
        None,
        None,
    );
    restricted.disable_mcp();
    restricted.install_keyring(sso::keyring());
    restricted.licenses().load(fixture.pool()).await.unwrap();
    let client = TestClient::with_state(restricted);
    let path = destinations(fixture.organization);
    let owner = &fixture.owner;

    for body in [
        webhook("LAN", "http://10.0.0.5/hook"),
        syslog(
            json!({"host": "10.0.0.9", "port": 514, "protocol": "udp"}),
            json!({}),
        ),
        json!({"name": "AWS", "settings": {"kind": "s3", "bucket": "audit", "region": "us-east-1"},
               "secrets": {"access_key_id": "a", "secret_access_key": "b"}}),
    ] {
        let response = post(&client, &path, owner, &body).await;
        response.assert_status(StatusCode::BAD_REQUEST);
        let code = response.json_value()["code"].clone();
        assert!(
            code == URL_INVALID || code == HOST_INVALID,
            "{body}: {code}"
        );
    }
    post(
        &client,
        &path,
        owner,
        &webhook("Allowed", "https://allowed.example/hook"),
    )
    .await
    .assert_status(StatusCode::CREATED);
    post(
        &client,
        &path,
        owner,
        &syslog(
            json!({"host": "allowed.example", "port": 6514, "protocol": "tls"}),
            json!({}),
        ),
    )
    .await
    .assert_status(StatusCode::CREATED);
    fixture.cleanup().await;
}

#[tokio::test]
async fn health_reports_failures_resets_on_change_and_reports_a_stall() {
    let fixture = Fixture::new(PREFIX).await;
    fixture.license(Tier::Enterprise).await;
    let client = &fixture.sso.client;
    let path = destinations(fixture.organization);
    let owner = &fixture.owner;
    let pool = fixture.pool();

    let siem = id(&post(
        client,
        &path,
        owner,
        &webhook("SIEM", "http://10.0.0.5/hook"),
    )
    .await);
    let uri = destination(fixture.organization, siem);
    let healthy = client.get_auth(&uri, &owner.token).await.json_value();
    let delivery = &healthy["delivery"];
    assert_eq!(delivery["health"], "healthy");
    assert_eq!(delivery["attempts"], 0);
    assert!(
        delivery["backlog"].as_i64().unwrap() >= 1,
        "the created row is pending"
    );
    assert!(delivery["oldest_pending_at"].is_string());
    assert!(delivery["next_attempt_at"].is_string());
    assert_eq!(delivery["last_error_code"], Value::Null);
    assert_eq!(delivery["delivered_count"], 0);

    fail_delivery(pool, siem, 3).await;
    let failing = client.get_auth(&uri, &owner.token).await.json_value();
    assert_eq!(failing["delivery"]["health"], "failing");
    assert_eq!(failing["delivery"]["attempts"], 3);
    assert_eq!(failing["delivery"]["last_error_code"], "status");
    assert_eq!(failing["delivery"]["last_error"], "HTTP 503");
    assert!(failing["delivery"]["last_failure_at"].is_string());

    client
        .patch_json_auth(&uri, &json!({"name": "Renamed"}), &owner.token)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        delivery_attempts(pool, siem).await,
        (3, Some(1)),
        "a rename keeps the pin"
    );

    let changed = client
        .patch_json_auth(
            &uri,
            &json!({"settings": {"kind": "webhook", "url": "http://10.0.0.6/hook"}}),
            &owner.token,
        )
        .await;
    changed.assert_status(StatusCode::OK);
    assert_eq!(changed.json_value()["delivery"]["health"], "healthy");
    assert_eq!(delivery_attempts(pool, siem).await, (0, None));

    fail_delivery(pool, siem, 2).await;
    client
        .patch_json_auth(
            &uri,
            &json!({"secrets": {"signing_secret": "rotated"}}),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(delivery_attempts(pool, siem).await, (0, None));

    client
        .patch_json_auth(&uri, &json!({"enabled": false}), &owner.token)
        .await
        .assert_status(StatusCode::OK);
    fail_delivery(pool, siem, 2).await;
    let reenabled = client
        .patch_json_auth(&uri, &json!({"enabled": true}), &owner.token)
        .await;
    reenabled.assert_status(StatusCode::OK);
    assert_eq!(delivery_attempts(pool, siem).await, (0, None));
    let due: bool = sqlx::query_scalar(
        "SELECT next_attempt_at <= now() FROM audit_deliveries WHERE destination_id = $1",
    )
    .bind(siem)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(due, "a re-enabled destination is retried now");

    let updates = fixture.audits(actions::AUDIT_DESTINATION_UPDATED).await;
    assert_eq!(updates[0].old_values, Some(json!({"name": "SIEM"})));
    assert_eq!(updates[0].new_values, Some(json!({"name": "Renamed"})));
    assert_eq!(
        updates[1].new_values.as_ref().unwrap()["settings"],
        json!({"kind": "webhook", "url": "http://10.0.0.6/hook"})
    );
    assert_eq!(updates[3].old_values, Some(json!({"enabled": true})));

    settle(pool).await;
    assert_eq!(
        client.get_auth(&path, &owner.token).await.json_value()["stalled_since"],
        Value::Null
    );
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_current_xact_id()")
        .execute(&mut *writer)
        .await
        .unwrap();
    let stuck: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO audit_logs (organization_id, action, resource_type, created_at) \
         VALUES ($1, 'test.stalled', 'test', now() - interval '10 minutes') RETURNING created_at",
    )
    .bind(fixture.organization)
    .fetch_one(pool)
    .await
    .unwrap();
    let stalled = client.get_auth(&path, &owner.token).await.json_value();
    let server = client.get_auth(SERVER, &owner.token).await.json_value();
    writer.rollback().await.unwrap();
    let since: DateTime<Utc> = serde_json::from_value(stalled["stalled_since"].clone()).unwrap();
    assert_eq!(since, stuck);
    assert_eq!(server["stalled_since"], Value::Null, "a stall is per scope");

    settle(pool).await;
    assert_eq!(
        client.get_auth(&path, &owner.token).await.json_value()["stalled_since"],
        Value::Null
    );
    fixture.cleanup().await;
}
