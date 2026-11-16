use axum::http::StatusCode;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::{Keyring, Payload, Seats, SigningKey, Tier, TrustedKey, period_start, sign};
use zone_server::db::{migrations, seats, sessions};

use crate::common::{self, Database, TestClient};

const PREFIX: &str = "seats";
const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];
const LICENSED: u32 = 2;

struct Account {
    id: Uuid,
    email: String,
    token: String,
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

    async fn login(&self, account: &Account) {
        self.client
            .post_json(
                "/api/auth/login",
                &json!({ "email": &account.email, "password": common::test_password() }),
            )
            .await
            .assert_status(StatusCode::OK);
    }

    async fn sign_in(&self) -> Account {
        let account = self.register().await;
        self.login(&account).await;
        account
    }

    async fn organization(&self, owner: &Account) -> Uuid {
        let slug = format!("seats-{}", Uuid::new_v4().simple());
        let response = self
            .client
            .post_json_auth(
                "/api/organizations",
                &json!({ "name": "Seats", "slug": slug }),
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

    async fn join(&self, organization: Uuid, member: &Account) {
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(organization)
        .bind(member.id)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn members(&self, count: usize) -> Vec<Account> {
        let owner = self.sign_in().await;
        let organization = self.organization(&owner).await;
        let mut accounts = vec![owner];
        for _ in 1..count {
            let member = self.sign_in().await;
            self.join(organization, &member).await;
            accounts.push(member);
        }
        accounts
    }

    async fn backdate(&self, account: &Account, to: DateTime<Utc>) {
        sqlx::query("UPDATE users SET last_login_at = ($1 AT TIME ZONE 'UTC') WHERE id = $2")
            .bind(to)
            .bind(account.id)
            .execute(self.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET last_active_at = $1 WHERE user_id = $2")
            .bind(to)
            .bind(account.id)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn memberships(&self, account: &Account) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM organization_members WHERE user_id = $1 AND is_active",
        )
        .bind(account.id)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    async fn set_organizations_active(&self, account: &Account, active: Option<bool>) {
        sqlx::query(
            "UPDATE organizations SET is_active = $1
             WHERE id IN (SELECT organization_id FROM organization_members WHERE user_id = $2)",
        )
        .bind(active)
        .bind(account.id)
        .execute(self.pool())
        .await
        .unwrap();
    }

    async fn used(&self, now: DateTime<Utc>) -> u32 {
        seats::used(self.pool(), period_start(now)).await.unwrap()
    }

    async fn cleanup(self) {
        self.database.cleanup().await;
    }
}

#[tokio::test]
async fn each_signed_in_member_takes_one_seat() {
    let fixture = Fixture::new().await;

    fixture.members(3).await;

    assert_eq!(fixture.used(Utc::now()).await, 3);
    fixture.cleanup().await;
}

#[tokio::test]
async fn several_sessions_of_one_user_take_one_seat() {
    let fixture = Fixture::new().await;
    let account = fixture.sign_in().await;
    fixture.login(&account).await;
    fixture.login(&account).await;

    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(account.id)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert!(sessions >= 3, "only {sessions} sessions");
    assert_eq!(fixture.used(Utc::now()).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn activity_counts_from_the_first_instant_of_the_month() {
    let fixture = Fixture::new().await;
    let now = Utc::now();
    let start = period_start(now);
    let accounts = fixture.members(2).await;

    fixture
        .backdate(&accounts[0], start - TimeDelta::seconds(1))
        .await;
    fixture.backdate(&accounts[1], start).await;

    assert_eq!(fixture.used(now).await, 1);
    fixture
        .backdate(&accounts[1], start - TimeDelta::seconds(1))
        .await;
    assert_eq!(fixture.used(now).await, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_login_this_month_counts_without_a_recent_session() {
    let fixture = Fixture::new().await;
    let now = Utc::now();
    let account = fixture.sign_in().await;
    fixture
        .backdate(&account, period_start(now) - TimeDelta::seconds(1))
        .await;

    sqlx::query("UPDATE users SET last_login_at = ($1 AT TIME ZONE 'UTC') WHERE id = $2")
        .bind(period_start(now))
        .bind(account.id)
        .execute(fixture.pool())
        .await
        .unwrap();

    assert_eq!(fixture.used(now).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_session_revoked_this_month_still_counts() {
    let fixture = Fixture::new().await;
    let account = fixture.sign_in().await;
    sessions::revoke_all_user_sessions(fixture.pool(), account.id)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET last_login_at = NULL WHERE id = $1")
        .bind(account.id)
        .execute(fixture.pool())
        .await
        .unwrap();

    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(account.id)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    assert_eq!(live, 0);
    assert_eq!(fixture.used(Utc::now()).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deactivated_user_takes_no_seat() {
    let fixture = Fixture::new().await;
    let accounts = fixture.members(2).await;

    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(accounts[1].id)
        .execute(fixture.pool())
        .await
        .unwrap();

    assert_eq!(fixture.used(Utc::now()).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_user_with_no_active_membership_takes_no_seat() {
    let fixture = Fixture::new().await;
    let accounts = fixture.members(2).await;

    sqlx::query("UPDATE organization_members SET is_active = false WHERE user_id = $1")
        .bind(accounts[1].id)
        .execute(fixture.pool())
        .await
        .unwrap();

    assert_eq!(fixture.used(Utc::now()).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_member_of_two_organizations_takes_one_seat() {
    let fixture = Fixture::new().await;
    let first = fixture.sign_in().await;
    let second = fixture.sign_in().await;
    let shared = fixture.organization(&second).await;
    fixture.join(shared, &first).await;

    assert!(fixture.memberships(&first).await >= 2);
    assert_eq!(fixture.used(Utc::now()).await, 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_user_with_no_recorded_activity_flag_takes_a_seat() {
    let fixture = Fixture::new().await;
    let accounts = fixture.members(2).await;

    sqlx::query("UPDATE users SET is_active = NULL WHERE id = $1")
        .bind(accounts[1].id)
        .execute(fixture.pool())
        .await
        .unwrap();

    assert_eq!(fixture.used(Utc::now()).await, 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn members_only_of_deactivated_organizations_take_no_seat() {
    let fixture = Fixture::new().await;
    let active = fixture.sign_in().await;
    let deactivated = fixture.sign_in().await;

    fixture
        .set_organizations_active(&deactivated, Some(false))
        .await;

    assert!(fixture.memberships(&deactivated).await >= 1);
    assert_eq!(fixture.used(Utc::now()).await, 1);
    fixture.set_organizations_active(&active, None).await;
    assert_eq!(fixture.used(Utc::now()).await, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn the_service_reports_licensed_seats_beside_the_used_ones() {
    let fixture = Fixture::new().await;
    let signing = SigningKey::from_bytes(&SEED);
    let state = fixture.client.state();
    state.install_keyring(
        Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
            .unwrap(),
    );
    let now = Utc::now();
    let license = state
        .licenses()
        .keyring()
        .verify(&sign(
            &Payload {
                id: Uuid::new_v4(),
                customer: "Acme Ltd".into(),
                tier: Tier::Team,
                seats: LICENSED,
                issued_at: now - TimeDelta::days(1),
                expires_at: now + TimeDelta::days(365),
                key_id: KEY_ID.into(),
            },
            &signing,
        ))
        .unwrap();
    fixture.members(3).await;

    let licensed = state
        .licenses()
        .seats(fixture.pool(), Some(&license), now)
        .await
        .unwrap();
    let community = state
        .licenses()
        .seats(fixture.pool(), None, now)
        .await
        .unwrap();

    assert_eq!(
        licensed,
        Seats {
            used: 3,
            licensed: Some(LICENSED)
        }
    );
    assert!(licensed.over());
    assert_eq!(
        community,
        Seats {
            used: 3,
            licensed: None
        }
    );
    assert!(!community.over());
    fixture.cleanup().await;
}
