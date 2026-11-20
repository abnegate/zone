use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::{
    Edition, GRACE_DAYS, Keyring, MALFORMED, PAYLOAD_INVALID, Payload, SEPARATOR,
    SIGNATURE_INVALID, SigningKey, TrustedKey, UNKNOWN_KEY, sign,
};
use zone_server::auth::admin_guard::REQUIRED;
use zone_server::db::audit::{actions, resources};
use zone_server::db::{licenses, migrations};
use zone_server::services::license::LAPSED;

use crate::common::{self, Database, TestClient, TestResponse};

const PREFIX: &str = "license";
const ROUTE: &str = "/api/license";
const TRUSTED: &str = "test-1";
const UNTRUSTED: &str = "test-2";
const TRUSTED_SEED: [u8; 32] = [0x11; 32];
const UNTRUSTED_SEED: [u8; 32] = [0x22; 32];
const LINE_WIDTH: usize = 64;
const SEATS: u32 = 25;
/// Seats are counted from `sessions`; renaming it once the license changes
/// fails the count after the change has committed, and only in this test's
/// own database. The upsert fires both the INSERT and UPDATE statement
/// triggers, so the rename must tolerate running twice.
const WITHHOLD_SESSIONS_ON_CHANGE: &str = "
    CREATE FUNCTION withhold_sessions() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN
        ALTER TABLE IF EXISTS sessions RENAME TO withheld_sessions;
        RETURN NULL;
    END
    $$;
    CREATE TRIGGER withhold_sessions AFTER INSERT OR UPDATE OR DELETE ON licenses
    FOR EACH STATEMENT EXECUTE FUNCTION withhold_sessions();
";
/// Seats are also counted from `organization_members`, which neither the
/// token check nor the admin guard reads; renaming it fails only the count,
/// so the admin's request still authenticates. Only this test's own database
/// is touched.
const WITHHOLD_MEMBERSHIPS: &str =
    "ALTER TABLE organization_members RENAME TO withheld_organization_members";
const WITHHOLD_LICENSES: &str = "ALTER TABLE licenses RENAME TO withheld_licenses";

struct Account {
    id: Uuid,
    email: String,
    token: String,
}

#[derive(sqlx::FromRow)]
struct Audit {
    organization_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    resource_type: String,
    resource_id: Option<Uuid>,
    old_values: Option<Value>,
    new_values: Option<Value>,
    row: String,
}

struct Fixture {
    database: Database,
    client: TestClient,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let client = TestClient::with_pool(database.pool.clone());
        client.state().install_keyring(keyring());
        Self { database, client }
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn register(&self) -> Account {
        let email = common::test_email();
        let response = self
            .client
            .post_json(
                "/api/auth/register",
                &json!({ "email": &email, "password": common::test_password() }),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        Account {
            id: body["user"]["id"].as_str().unwrap().parse().unwrap(),
            email,
            token: body["access_token"].as_str().unwrap().to_string(),
        }
    }

    async fn login(&self, account: &Account) -> String {
        let response = self
            .client
            .post_json(
                "/api/auth/login",
                &json!({ "email": &account.email, "password": common::test_password() }),
            )
            .await;
        response.assert_status(StatusCode::OK);
        response.json_value()["access_token"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn organization(&self, owner: &Account) -> Uuid {
        let response = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "Tenant", "slug": format!("license-{}", Uuid::new_v4().simple()) }),
                &owner.token,
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()["organization"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    async fn get(&self, token: &str) -> TestResponse {
        self.client.get_auth(ROUTE, token).await
    }

    async fn put(&self, token: &str, key: &str) -> TestResponse {
        self.client
            .put_json_auth(ROUTE, &json!({ "key": key }), token)
            .await
    }

    async fn put_body(&self, token: &str, body: &str) -> TestResponse {
        self.client
            .send_request(
                Request::builder()
                    .method(Method::PUT)
                    .uri(ROUTE)
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
    }

    async fn delete(&self, token: &str) -> TestResponse {
        self.client.delete_auth(ROUTE, token).await
    }

    async fn anonymous(&self, method: Method) -> TestResponse {
        self.client
            .send_request(
                Request::builder()
                    .method(method)
                    .uri(ROUTE)
                    .header("Content-Type", "application/json")
                    .body(Body::from(json!({ "key": active() }).to_string()))
                    .unwrap(),
            )
            .await
    }

    async fn demote(&self, account: &Account) {
        self.update_user("UPDATE users SET is_admin = false WHERE id = $1", account)
            .await;
    }

    async fn deactivate(&self, account: &Account) {
        self.update_user("UPDATE users SET is_active = false WHERE id = $1", account)
            .await;
    }

    async fn update_user(&self, statement: &'static str, account: &Account) {
        sqlx::query(statement)
            .bind(account.id)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn withhold(&self, statement: &'static str) {
        sqlx::raw_sql(statement).execute(self.pool()).await.unwrap();
    }

    async fn stored(&self) -> Option<String> {
        licenses::get(self.pool()).await.unwrap()
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM licenses")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn audits(&self, action: &str) -> Vec<Audit> {
        sqlx::query_as(
            "SELECT organization_id, actor_id, resource_type, resource_id, old_values, new_values, row_to_json(a)::text AS row
             FROM audit_logs a WHERE action = $1 ORDER BY created_at",
        )
        .bind(action)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

fn trusted() -> SigningKey {
    SigningKey::from_bytes(&TRUSTED_SEED)
}

fn untrusted() -> SigningKey {
    SigningKey::from_bytes(&UNTRUSTED_SEED)
}

fn keyring() -> Keyring {
    Keyring::new([TrustedKey::new(TRUSTED, trusted().verifying_key().to_bytes()).unwrap()]).unwrap()
}

fn expiring(expires_in: TimeDelta) -> Payload {
    let expires_at = Utc::now().trunc_subsecs(0) + expires_in;
    Payload {
        id: Uuid::new_v4(),
        customer: "Acme Ltd".into(),
        seats: SEATS,
        issued_at: expires_at - TimeDelta::days(365),
        expires_at,
        key_id: TRUSTED.into(),
        subscription: None,
    }
}

fn current() -> Payload {
    expiring(TimeDelta::days(365))
}

fn issue(payload: &Payload) -> String {
    sign(payload, &trusted())
}

fn active() -> String {
    issue(&current())
}

fn lapsed() -> String {
    issue(&expiring(-TimeDelta::days(GRACE_DAYS + 1)))
}

fn retired() -> String {
    sign(
        &Payload {
            key_id: UNTRUSTED.into(),
            ..current()
        },
        &untrusted(),
    )
}

fn forged() -> String {
    sign(&current(), &untrusted())
}

fn wrap(text: &str) -> String {
    text.as_bytes()
        .chunks(LINE_WIDTH)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn grace_end(expires_at: DateTime<Utc>) -> DateTime<Utc> {
    expires_at + TimeDelta::days(GRACE_DAYS)
}

fn details(payload: &Payload) -> Value {
    json!({
        "id": payload.id,
        "customer": payload.customer,
        "seats": payload.seats,
        "issued_at": payload.issued_at,
        "expires_at": payload.expires_at,
        "grace_ends_at": grace_end(payload.expires_at),
        "key_id": payload.key_id,
    })
}

fn assert_community(body: &Value) {
    assert_runs_as_community(body);
    assert_eq!(body["unverified"], Value::Null, "{body}");
    assert!(
        body.get("unverified").is_some(),
        "unverified is omitted: {body}"
    );
}

fn assert_runs_as_community(body: &Value) {
    assert_eq!(body["edition"], "community", "{body}");
    assert_eq!(body["status"], "community", "{body}");
    assert_eq!(body["license"], Value::Null, "{body}");
    assert_eq!(body["seats"]["licensed"], Value::Null, "{body}");
    assert_eq!(body["seats"]["over"], false, "{body}");
    assert!(body.get("license").is_some(), "license is omitted: {body}");
    assert!(
        body["seats"].get("licensed").is_some(),
        "seats.licensed is omitted: {body}"
    );
}

fn assert_forbidden(response: &TestResponse) {
    response.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(response.json_value(), json!({ "error": REQUIRED }));
}

fn assert_refused(response: &TestResponse, code: &str) {
    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.json_value();
    assert_eq!(body["code"], code, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "{body}"
    );
}

async fn assert_all_forbidden(fixture: &Fixture, token: &str) {
    assert_forbidden(&fixture.get(token).await);
    assert_forbidden(&fixture.put(token, &active()).await);
    assert_forbidden(&fixture.delete(token).await);
}

#[tokio::test]
async fn every_method_needs_a_token() {
    let fixture = Fixture::new().await;

    for method in [Method::GET, Method::PUT, Method::DELETE] {
        let response = fixture.anonymous(method.clone()).await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{method}: {}",
            response.text()
        );
    }
    assert_eq!(fixture.rows().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_user_who_is_not_a_server_admin_is_refused() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let member = fixture.register().await;

    assert_all_forbidden(&fixture, &member.token).await;
    fixture
        .get(&admin.token)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(fixture.rows().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn owning_an_organization_does_not_make_a_server_admin() {
    let fixture = Fixture::new().await;
    fixture.register().await;
    let owner = fixture.register().await;
    let organization = fixture.organization(&owner).await;
    let role: String = sqlx::query_scalar(
        "SELECT role::text FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(owner.id)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    assert_eq!(role, "owner");

    assert_all_forbidden(&fixture, &owner.token).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_demoted_admin_is_refused_while_their_token_is_still_valid() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    fixture
        .get(&admin.token)
        .await
        .assert_status(StatusCode::OK);

    fixture.demote(&admin).await;

    assert_all_forbidden(&fixture, &admin.token).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deactivated_admin_is_refused_while_their_token_is_still_live() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    fixture
        .get(&admin.token)
        .await
        .assert_status(StatusCode::OK);

    fixture.deactivate(&admin).await;

    assert_all_forbidden(&fixture, &admin.token).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn an_empty_server_reads_as_community() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;

    let response = fixture.get(&admin.token).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_community(&body);
    assert_eq!(body["seats"]["used"], 1, "{body}");
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_is_stored_shown_and_audited_without_its_text() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = current();
    let key = issue(&payload);

    let response = fixture.put(&admin.token, &key).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["status"], "active", "{body}");
    assert_eq!(body["license"], details(&payload));
    assert_eq!(
        body["seats"],
        json!({ "used": 1, "licensed": SEATS, "over": false })
    );
    let read = fixture.get(&admin.token).await;
    read.assert_status(StatusCode::OK);
    assert_eq!(read.json_value(), body);

    let audits = fixture.audits(actions::LICENSE_SET).await;
    assert_eq!(audits.len(), 1);
    let audit = &audits[0];
    assert_eq!(audit.resource_type, resources::LICENSE);
    assert_eq!(audit.resource_id, Some(payload.id));
    assert_eq!(audit.organization_id, None);
    assert_eq!(audit.actor_id, Some(admin.id));
    assert_eq!(audit.old_values, None);
    let new_values = audit.new_values.as_ref().unwrap();
    assert_eq!(new_values["id"], json!(payload.id));
    assert!(new_values.get("tier").is_none(), "{new_values}");
    assert!(!audit.row.contains(&key), "the audit row holds the key");
    let (encoded_payload, signature) = key.split_once(SEPARATOR).unwrap();
    assert!(!audit.row.contains(encoded_payload));
    assert!(!audit.row.contains(signature));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_renewed_key_replaces_the_stored_key() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let first = current();
    let renewed = current();
    fixture
        .put(&admin.token, &issue(&first))
        .await
        .assert_status(StatusCode::OK);

    let response = fixture.put(&admin.token, &issue(&renewed)).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["license"], details(&renewed));
    assert_eq!(fixture.rows().await, 1);
    let audits = fixture.audits(actions::LICENSE_SET).await;
    assert_eq!(audits.len(), 2);
    let replacement = &audits[1];
    assert_eq!(replacement.resource_id, Some(renewed.id));
    assert_eq!(
        replacement.old_values.as_ref().unwrap()["id"],
        json!(first.id)
    );
    assert_eq!(
        replacement.new_values.as_ref().unwrap()["id"],
        json!(renewed.id)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_wrapped_across_lines_is_accepted() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = current();
    let key = issue(&payload);
    let wrapped = wrap(&key);
    assert!(wrapped.contains('\n'));

    let response = fixture.put(&admin.token, &wrapped).await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["license"], details(&payload));
    assert_eq!(fixture.stored().await.as_deref(), Some(key.as_str()));
    fixture.cleanup().await;
}

#[tokio::test]
async fn each_refused_key_names_its_reason_and_leaves_the_license_alone() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let original = issue(&current());
    fixture
        .put(&admin.token, &original)
        .await
        .assert_status(StatusCode::OK);
    let before = fixture.get(&admin.token).await.json_value();
    let (_, signature) = original.split_once(SEPARATOR).unwrap();
    let other = issue(&current());
    let (other_payload, _) = other.split_once(SEPARATOR).unwrap();
    let tampered = format!("{other_payload}{SEPARATOR}{signature}");
    let unknown = sign(
        &Payload {
            key_id: UNTRUSTED.into(),
            ..current()
        },
        &untrusted(),
    );
    let seatless = issue(&Payload {
        seats: 0,
        ..current()
    });
    let expired = lapsed();

    let cases = [
        (tampered.as_str(), SIGNATURE_INVALID),
        (unknown.as_str(), UNKNOWN_KEY),
        ("not a license key", MALFORMED),
        (seatless.as_str(), PAYLOAD_INVALID),
        (expired.as_str(), LAPSED),
    ];
    for (key, code) in cases {
        assert_refused(&fixture.put(&admin.token, key).await, code);
        assert_eq!(
            fixture.stored().await.as_deref(),
            Some(original.as_str()),
            "{code}"
        );
        assert_eq!(
            fixture.get(&admin.token).await.json_value(),
            before,
            "{code}"
        );
    }
    assert_eq!(fixture.audits(actions::LICENSE_SET).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_body_without_a_readable_key_is_a_bad_request() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;

    for body in ["{\"key\": ", "{}", "{\"key\": 7}"] {
        let response = fixture.put_body(&admin.token, body).await;
        response.assert_status(StatusCode::BAD_REQUEST);
        let body = response.json_value();
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "{body}"
        );
        assert!(body.get("code").is_none(), "{body}");
    }
    assert_eq!(fixture.rows().await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_in_grace_is_accepted_as_enterprise() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = expiring(-TimeDelta::days(1));

    let response = fixture.put(&admin.token, &issue(&payload)).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["status"], "grace", "{body}");
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["license"], details(&payload));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_lapsed_key_is_shown_as_lapsed_enterprise() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = expiring(-TimeDelta::days(GRACE_DAYS + 1));
    licenses::replace(fixture.pool(), &issue(&payload))
        .await
        .unwrap();

    let response = fixture.get(&admin.token).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["status"], "lapsed", "{body}");
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["license"], details(&payload));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_locks_nobody_out() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let member = fixture.register().await;
    licenses::replace(fixture.pool(), &lapsed()).await.unwrap();
    assert_eq!(
        fixture.get(&admin.token).await.json_value()["status"],
        "lapsed"
    );

    let token = fixture.login(&member).await;
    fixture
        .client
        .get_auth("/api/auth/sessions", &token)
        .await
        .assert_status(StatusCode::OK);
    fixture
        .client
        .get_auth("/api/organizations", &token)
        .await
        .assert_status(StatusCode::OK);

    let renewed = fixture.put(&admin.token, &active()).await;
    renewed.assert_status(StatusCode::OK);
    assert_eq!(renewed.json_value()["status"], "active");

    let removed = fixture.delete(&admin.token).await;
    removed.assert_status(StatusCode::OK);
    assert_community(&removed.json_value());
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_stored_key_that_no_longer_verifies_reads_as_community_and_names_why() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;

    for (text, code) in [(retired(), UNKNOWN_KEY), (forged(), SIGNATURE_INVALID)] {
        licenses::replace(fixture.pool(), &text).await.unwrap();

        let response = fixture.get(&admin.token).await;

        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_runs_as_community(&body);
        assert_eq!(body["unverified"], json!({ "code": code }));
        assert_eq!(fixture.stored().await, Some(text));
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn removing_a_key_that_no_longer_verifies_is_audited() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    licenses::replace(fixture.pool(), &retired()).await.unwrap();

    let response = fixture.delete(&admin.token).await;

    response.assert_status(StatusCode::OK);
    assert_community(&response.json_value());
    assert_eq!(fixture.rows().await, 0);
    let audits = fixture.audits(actions::LICENSE_REMOVED).await;
    assert_eq!(audits.len(), 1);
    let audit = &audits[0];
    assert_eq!(audit.resource_type, resources::LICENSE);
    assert_eq!(audit.resource_id, None);
    assert_eq!(audit.organization_id, None);
    assert_eq!(audit.actor_id, Some(admin.id));
    assert_eq!(audit.old_values, Some(json!({ "verified": false })));
    assert_eq!(audit.new_values, None);
    fixture.cleanup().await;
}

#[tokio::test]
async fn replacing_a_key_that_no_longer_verifies_is_audited() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    licenses::replace(fixture.pool(), &forged()).await.unwrap();
    let payload = current();

    let response = fixture.put(&admin.token, &issue(&payload)).await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["unverified"], Value::Null);
    let audits = fixture.audits(actions::LICENSE_SET).await;
    assert_eq!(audits.len(), 1);
    let audit = &audits[0];
    assert_eq!(audit.resource_id, Some(payload.id));
    assert_eq!(audit.old_values, Some(json!({ "verified": false })));
    assert_eq!(audit.new_values.as_ref().unwrap()["id"], json!(payload.id));
    fixture.cleanup().await;
}

#[tokio::test]
async fn removing_the_license_returns_community_and_audits_once() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = current();
    let key = issue(&payload);
    fixture
        .put(&admin.token, &key)
        .await
        .assert_status(StatusCode::OK);

    let response = fixture.delete(&admin.token).await;

    response.assert_status(StatusCode::OK);
    assert_community(&response.json_value());
    assert_eq!(fixture.rows().await, 0);
    let audits = fixture.audits(actions::LICENSE_REMOVED).await;
    assert_eq!(audits.len(), 1);
    let audit = &audits[0];
    assert_eq!(audit.resource_type, resources::LICENSE);
    assert_eq!(audit.resource_id, Some(payload.id));
    assert_eq!(audit.organization_id, None);
    assert_eq!(audit.actor_id, Some(admin.id));
    assert_eq!(audit.old_values.as_ref().unwrap()["id"], json!(payload.id));
    assert_eq!(audit.new_values, None);
    assert!(!audit.row.contains(&key), "the audit row holds the key");

    let again = fixture.delete(&admin.token).await;

    again.assert_status(StatusCode::OK);
    assert_community(&again.json_value());
    assert_eq!(fixture.audits(actions::LICENSE_REMOVED).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn seats_over_the_license_are_reported_but_never_block_sign_in() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let member = fixture.register().await;
    let response = fixture
        .put(
            &admin.token,
            &issue(&Payload {
                seats: 1,
                ..current()
            }),
        )
        .await;
    response.assert_status(StatusCode::OK);

    let body = fixture.get(&admin.token).await.json_value();

    assert_eq!(
        body["seats"],
        json!({ "used": 2, "licensed": 1, "over": true })
    );
    fixture.login(&admin).await;
    fixture.login(&member).await;
    fixture.cleanup().await;
}

fn assert_uncounted(body: &Value, licensed: Value) {
    assert_eq!(
        body["seats"],
        json!({ "used": null, "licensed": licensed, "over": false })
    );
    assert!(
        body["seats"].get("used").is_some(),
        "seats.used is omitted: {body}"
    );
}

#[tokio::test]
async fn setting_a_key_whose_seats_cannot_be_counted_reports_usage_as_unknown() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = Payload {
        seats: 1,
        ..current()
    };
    let key = issue(&payload);
    fixture.register().await;
    fixture.withhold(WITHHOLD_SESSIONS_ON_CHANGE).await;

    let response = fixture.put(&admin.token, &key).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["license"], details(&payload));
    assert_uncounted(&body, json!(1));
    assert_eq!(fixture.stored().await.as_deref(), Some(key.as_str()));
    assert_eq!(fixture.audits(actions::LICENSE_SET).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn removing_a_key_whose_seats_cannot_be_counted_reports_usage_as_unknown() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    fixture
        .put(&admin.token, &active())
        .await
        .assert_status(StatusCode::OK);
    fixture.withhold(WITHHOLD_SESSIONS_ON_CHANGE).await;

    let response = fixture.delete(&admin.token).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_community(&body);
    assert_uncounted(&body, Value::Null);
    assert_eq!(fixture.rows().await, 0);
    assert_eq!(fixture.audits(actions::LICENSE_REMOVED).await.len(), 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn reading_a_license_whose_seats_cannot_be_counted_reports_usage_as_unknown() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let payload = current();
    fixture
        .put(&admin.token, &issue(&payload))
        .await
        .assert_status(StatusCode::OK);
    fixture.withhold(WITHHOLD_MEMBERSHIPS).await;

    let response = fixture.get(&admin.token).await;

    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["edition"], "enterprise", "{body}");
    assert_eq!(body["status"], "active", "{body}");
    assert_eq!(body["license"], details(&payload));
    assert_uncounted(&body, json!(SEATS));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_license_that_cannot_be_loaded_is_a_server_error() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    fixture
        .put(&admin.token, &active())
        .await
        .assert_status(StatusCode::OK);
    fixture.withhold(WITHHOLD_LICENSES).await;

    let response = fixture.get(&admin.token).await;

    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.json_value().get("license").is_none());
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_edition_accessor_follows_set_and_remove() {
    let fixture = Fixture::new().await;
    let admin = fixture.register().await;
    let state = fixture.client.state();
    assert_eq!(state.edition().await, Edition::Community);

    fixture
        .put(&admin.token, &active())
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(state.edition().await, Edition::Enterprise);

    fixture
        .delete(&admin.token)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(state.edition().await, Edition::Community);
    fixture.cleanup().await;
}
