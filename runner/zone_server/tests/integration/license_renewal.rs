use std::time::Duration;

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zone_enterprise::{Keyring, License, Payload, SigningKey, TrustedKey, sign};
use zone_server::db::audit::{actions, resources};
use zone_server::db::license_renewal::{self, LicenseRenewal};
use zone_server::db::{licenses, migrations};
use zone_server::services::license::Licenses;
use zone_server::services::license::renewal::{self, Client, Outcome, Proxy, Settings};

use crate::common::Database;

const PREFIX: &str = "license_renewal";
const RENEW_PATH: &str = "/licenses/renew";
const TRUSTED: &str = "test-1";
const UNTRUSTED: &str = "test-2";
const TRUSTED_SEED: [u8; 32] = [0x11; 32];
const UNTRUSTED_SEED: [u8; 32] = [0x22; 32];
const CUSTOMER: &str = "Acme Ltd";
const SUBSCRIPTION: &str = "6f1f4b1e-4a8e-4f4e-9a55-0d6f3b8c2a11";
const ANSWER_DELAY: Duration = Duration::from_millis(750);
const ADMIN_DELAY: Duration = Duration::from_millis(150);

#[derive(Debug, sqlx::FromRow)]
struct Audit {
    actor_id: Option<Uuid>,
    resource_type: String,
    resource_id: Option<Uuid>,
    old_values: Option<Value>,
    new_values: Option<Value>,
    row: String,
}

struct Fixture {
    database: Database,
    licenses: Licenses,
    now: DateTime<Utc>,
}

impl Fixture {
    async fn new() -> Self {
        let database = Database::new(PREFIX).await;
        migrations::run(&database.pool).await.unwrap();
        let licenses = Licenses::new();
        licenses.install_keyring(keyring());
        Self {
            database,
            licenses,
            now: Utc::now().trunc_subsecs(6),
        }
    }

    async fn storing(payload: &Payload) -> (Self, License) {
        let fixture = Self::new().await;
        let license = verified(&sign(payload, &trusted()));
        licenses::replace(fixture.pool(), license.text())
            .await
            .unwrap();
        (fixture, license)
    }

    fn pool(&self) -> &PgPool {
        &self.database.pool
    }

    async fn renew(&self, server: &MockServer) -> Option<Outcome> {
        renewal::renew(&self.licenses, self.pool(), &client(server), self.now)
            .await
            .unwrap()
    }

    async fn stored(&self) -> Option<String> {
        licenses::get(self.pool()).await.unwrap()
    }

    async fn status(&self) -> Option<LicenseRenewal> {
        license_renewal::get(self.pool()).await.unwrap()
    }

    fn recorded(&self, outcome: Outcome) -> Option<LicenseRenewal> {
        Some(LicenseRenewal {
            attempted_at: self.now,
            outcome: outcome.as_str().into(),
        })
    }

    async fn audits(&self) -> Vec<Audit> {
        sqlx::query_as(
            "SELECT actor_id, resource_type, resource_id, old_values, new_values, row_to_json(a)::text AS row
             FROM audit_logs a WHERE action = $1 ORDER BY created_at",
        )
        .bind(actions::LICENSE_RENEWED)
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

fn verified(text: &str) -> License {
    keyring().verify(text).unwrap()
}

fn expiring(expires_in: TimeDelta) -> Payload {
    let expires_at = Utc::now().trunc_subsecs(0) + expires_in;
    Payload {
        id: Uuid::new_v4(),
        customer: CUSTOMER.into(),
        seats: 25,
        issued_at: expires_at - TimeDelta::days(365),
        expires_at,
        key_id: TRUSTED.into(),
        subscription: Some(SUBSCRIPTION.into()),
    }
}

fn due() -> Payload {
    expiring(TimeDelta::days(3))
}

fn renewal(current: &Payload) -> Payload {
    Payload {
        id: Uuid::new_v4(),
        issued_at: current.expires_at,
        expires_at: current.expires_at + TimeDelta::days(365),
        ..current.clone()
    }
}

fn renewed(current: &Payload) -> String {
    sign(&renewal(current), &trusted())
}

fn client(server: &MockServer) -> Client {
    let settings = Settings {
        enabled: true,
        url: renewal::validated(&format!("{}{RENEW_PATH}", server.uri())).unwrap(),
    };
    Client::new(&settings, Proxy::Direct).unwrap()
}

async fn signer(stored: &License, template: ResponseTemplate, expected: u64) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RENEW_PATH))
        .and(body_json(json!({ "license": stored.text() })))
        .respond_with(template)
        .expect(expected)
        .mount(&server)
        .await;
    server
}

fn issuing(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "license": text }))
}

#[tokio::test]
async fn a_renewed_key_is_installed_recorded_and_audited() {
    let payload = due();
    let (fixture, stored) = Fixture::storing(&payload).await;
    let text = renewed(&payload);
    let renewed = verified(&text);
    let server = signer(&stored, issuing(&text), 1).await;

    assert_eq!(fixture.renew(&server).await, Some(Outcome::Renewed));

    assert_eq!(fixture.stored().await.as_deref(), Some(renewed.text()));
    assert_eq!(fixture.status().await, fixture.recorded(Outcome::Renewed));
    assert_eq!(
        fixture.licenses.current(fixture.pool()).await,
        Some(renewed.clone())
    );
    let audits = fixture.audits().await;
    assert_eq!(audits.len(), 1, "{audits:?}");
    let audit = &audits[0];
    assert_eq!(audit.actor_id, None);
    assert_eq!(audit.resource_type, resources::LICENSE);
    assert_eq!(audit.resource_id, Some(renewed.id()));
    assert_eq!(audit.old_values.as_ref().unwrap()["id"], json!(stored.id()));
    assert_eq!(
        audit.new_values.as_ref().unwrap()["id"],
        json!(renewed.id())
    );
    assert!(
        !audit.row.contains(stored.text()),
        "the audit row holds the old key"
    );
    assert!(
        !audit.row.contains(renewed.text()),
        "the audit row holds the new key"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_renewed_key_with_surrounding_whitespace_is_recorded_against_its_stored_text() {
    let payload = due();
    let (fixture, stored) = Fixture::storing(&payload).await;
    let text = renewed(&payload);
    let server = signer(&stored, issuing(&format!("  {text}\n")), 1).await;

    assert_eq!(fixture.renew(&server).await, Some(Outcome::Renewed));

    assert_eq!(fixture.stored().await.as_deref(), Some(text.as_str()));
    assert_eq!(fixture.status().await, fixture.recorded(Outcome::Renewed));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_renewal_against_a_key_no_longer_stored_leaves_the_admins_key() {
    let payload = due();
    let (fixture, stale) = Fixture::storing(&payload).await;
    let admin = verified(&sign(&expiring(TimeDelta::days(365)), &trusted()));
    fixture
        .licenses
        .set(fixture.pool(), admin.text(), fixture.now)
        .await
        .unwrap();

    let change = fixture
        .licenses
        .renew(
            fixture.pool(),
            stale.text(),
            &renewed(&payload),
            fixture.now,
        )
        .await
        .unwrap();

    assert_eq!(change, None);
    assert_eq!(fixture.stored().await.as_deref(), Some(admin.text()));
    assert_eq!(
        fixture.licenses.current(fixture.pool()).await,
        Some(admin.clone())
    );
    assert_eq!(fixture.status().await, None);
    assert!(fixture.audits().await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_key_an_admin_installs_while_the_signer_answers_is_kept() {
    let payload = due();
    let (fixture, stored) = Fixture::storing(&payload).await;
    let admin = verified(&sign(&expiring(TimeDelta::days(365)), &trusted()));
    let server = signer(
        &stored,
        issuing(&renewed(&payload)).set_delay(ANSWER_DELAY),
        1,
    )
    .await;

    let (outcome, installed) = tokio::join!(fixture.renew(&server), async {
        tokio::time::sleep(ADMIN_DELAY).await;
        fixture
            .licenses
            .set(fixture.pool(), admin.text(), fixture.now)
            .await
    });

    installed.unwrap();
    assert_eq!(outcome, None);
    assert_eq!(fixture.stored().await.as_deref(), Some(admin.text()));
    assert_eq!(
        fixture.licenses.current(fixture.pool()).await,
        Some(admin.clone())
    );
    assert_eq!(fixture.status().await, None);
    assert!(fixture.audits().await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn each_answer_without_a_key_is_recorded_and_the_key_kept() {
    let cases = [
        (204, Outcome::Current),
        (410, Outcome::Ended),
        (503, Outcome::Unreachable),
        (422, Outcome::Refused),
    ];
    for (status, outcome) in cases {
        let (fixture, stored) = Fixture::storing(&due()).await;
        let server = signer(&stored, ResponseTemplate::new(status), 1).await;

        assert_eq!(fixture.renew(&server).await, Some(outcome), "{status}");

        assert_eq!(
            fixture.stored().await.as_deref(),
            Some(stored.text()),
            "{status}"
        );
        assert_eq!(
            fixture.status().await,
            fixture.recorded(outcome),
            "{status}"
        );
        assert!(fixture.audits().await.is_empty(), "{status}");
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn a_renewed_key_zone_will_not_install_is_rejected_and_the_key_kept() {
    let lapsed = expiring(-TimeDelta::days(90));
    let cases: [(&str, Payload, ResponseTemplate); 6] = {
        let current = due();
        let untrusted_key = sign(
            &Payload {
                key_id: UNTRUSTED.into(),
                ..renewal(&current)
            },
            &untrusted(),
        );
        let other_customer = sign(
            &Payload {
                customer: "Globex Corp".into(),
                ..renewal(&current)
            },
            &trusted(),
        );
        let earlier = sign(
            &Payload {
                issued_at: current.issued_at,
                expires_at: current.expires_at - TimeDelta::days(1),
                ..renewal(&current)
            },
            &trusted(),
        );
        let same_id = sign(
            &Payload {
                id: current.id,
                ..renewal(&current)
            },
            &trusted(),
        );
        let still_lapsed = sign(
            &Payload {
                expires_at: lapsed.expires_at + TimeDelta::days(1),
                ..renewal(&lapsed)
            },
            &trusted(),
        );
        [
            (
                "an untrusted signer",
                current.clone(),
                issuing(&untrusted_key),
            ),
            (
                "another customer",
                current.clone(),
                issuing(&other_customer),
            ),
            ("an earlier expiry", current.clone(), issuing(&earlier)),
            ("the same id", current.clone(), issuing(&same_id)),
            ("a lapsed key", lapsed.clone(), issuing(&still_lapsed)),
            (
                "a malformed body",
                current,
                ResponseTemplate::new(200).set_body_string("not json"),
            ),
        ]
    };
    for (case, payload, template) in cases {
        let (fixture, stored) = Fixture::storing(&payload).await;
        let server = signer(&stored, template, 1).await;

        assert_eq!(
            fixture.renew(&server).await,
            Some(Outcome::Rejected),
            "{case}"
        );

        assert_eq!(
            fixture.stored().await.as_deref(),
            Some(stored.text()),
            "{case}"
        );
        assert_eq!(
            fixture.status().await,
            fixture.recorded(Outcome::Rejected),
            "{case}"
        );
        assert!(fixture.audits().await.is_empty(), "{case}");
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn a_license_that_is_not_due_sends_nothing() {
    let cases = [
        ("a year from expiry", expiring(TimeDelta::days(365))),
        (
            "no subscription",
            Payload {
                subscription: None,
                ..due()
            },
        ),
    ];
    for (case, payload) in cases {
        let (fixture, stored) = Fixture::storing(&payload).await;
        let server = signer(&stored, ResponseTemplate::new(204), 0).await;

        assert_eq!(fixture.renew(&server).await, None, "{case}");

        server.verify().await;
        assert_eq!(fixture.status().await, None, "{case}");
        assert_eq!(
            fixture.stored().await.as_deref(),
            Some(stored.text()),
            "{case}"
        );
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn no_license_sends_nothing() {
    let fixture = Fixture::new().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    assert_eq!(fixture.renew(&server).await, None);

    server.verify().await;
    assert_eq!(fixture.stored().await, None);
    fixture.cleanup().await;
}
