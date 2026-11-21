//! The Community cap on active members: every path that would add a person refuses at the cap,
//! and nobody who is already a member is ever turned away.

use std::collections::HashMap;

use axum::http::StatusCode;
use chrono::{DateTime, TimeDelta, Utc};
use reqwest::Url;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::sso::{Claims, testing};
use zone_enterprise::{COMMUNITY_MEMBERS, MEMBER_CAP_REACHED, period_start};
use zone_server::db::seats;

use crate::common::identity_provider::IdentityProvider;
use crate::common::sso::{Fixture, active, grace, lapsed, lapsed_team, team};
use crate::common::{TestResponse, test_email, test_password};

const PREFIX: &str = "member_cap";
const REGISTER: &str = "/api/auth/register";
const LOGIN: &str = "/api/auth/login";
const REFRESH: &str = "/api/auth/refresh";
const ORGANIZATIONS: &str = "/api/organizations";
const LICENSE: &str = "/api/license";
const PUBLIC_PROVIDERS: &str = "/api/auth/sso/providers";
const CALLBACKS: &str = "/api/auth/sso/callbacks";
const UNVERIFIED_KEY: &str = "garbage";

struct Member {
    id: Uuid,
    email: String,
    access: String,
    refresh: String,
}

impl Member {
    fn from(response: &TestResponse, email: &str) -> Self {
        let body = response.json_value();
        Self {
            id: body["user"]["id"].as_str().unwrap().parse().unwrap(),
            email: email.to_owned(),
            access: body["access_token"].as_str().unwrap().to_owned(),
            refresh: body["refresh_token"].as_str().unwrap().to_owned(),
        }
    }
}

struct Server {
    fixture: Fixture,
}

impl Server {
    async fn capped() -> Self {
        Self {
            fixture: Fixture::capped(PREFIX).await,
        }
    }

    async fn uncapped() -> Self {
        Self {
            fixture: Fixture::new(PREFIX).await,
        }
    }

    fn pool(&self) -> &PgPool {
        self.fixture.pool()
    }

    async fn sign_up(&self, email: &str) -> TestResponse {
        self.fixture
            .client
            .post_json(
                REGISTER,
                &json!({ "email": email, "password": test_password() }),
            )
            .await
    }

    async fn join(&self) -> Member {
        let email = test_email();
        let response = self.sign_up(&email).await;
        response.assert_status(StatusCode::CREATED);
        Member::from(&response, &email)
    }

    async fn members(&self, count: usize) -> Vec<Member> {
        let mut members = Vec::with_capacity(count);
        for _ in 0..count {
            members.push(self.join().await);
        }
        members
    }

    async fn post(&self, uri: &str, body: &Value, member: &Member) -> TestResponse {
        self.fixture
            .client
            .post_json_auth(uri, body, &member.access)
            .await
    }

    async fn invite(&self, owner: &Member, email: &str) -> TestResponse {
        let organization = self.organization(owner).await;
        self.post(
            &format!("{ORGANIZATIONS}/{organization}/invitations"),
            &json!({ "email": email, "org_role": "member" }),
            owner,
        )
        .await
    }

    async fn invited(&self, owner: &Member, email: &str) -> String {
        let response = self.invite(owner, email).await;
        response.assert_status(StatusCode::CREATED);
        response.json_value()["token"].as_str().unwrap().to_owned()
    }

    async fn accept(&self, token: &str, member: &Member) -> TestResponse {
        self.post(
            &format!("/api/invitations/{token}/accept"),
            &json!({}),
            member,
        )
        .await
    }

    async fn add(&self, owner: &Member, member: &Member) -> TestResponse {
        let organization = self.organization(owner).await;
        self.post(
            &format!("{ORGANIZATIONS}/{organization}/members"),
            &json!({ "user_id": member.id, "role": "member" }),
            owner,
        )
        .await
    }

    async fn create_organization(&self, member: &Member) -> TestResponse {
        self.post(
            ORGANIZATIONS,
            &json!({ "name": "Capped", "slug": format!("capped-{}", Uuid::new_v4().simple()) }),
            member,
        )
        .await
    }

    async fn organization(&self, owner: &Member) -> Uuid {
        sqlx::query_scalar(
            "SELECT organization_id FROM organization_members
             WHERE user_id = $1 AND role = 'owner' AND is_active
             ORDER BY created_at LIMIT 1",
        )
        .bind(owner.id)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    /// Ends every membership `member` holds, so they neither count nor are a member.
    async fn strand(&self, member: &Member) {
        sqlx::query("UPDATE organization_members SET is_active = false WHERE user_id = $1")
            .bind(member.id)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn backdate(&self, member: &Member, to: DateTime<Utc>) {
        sqlx::query("UPDATE users SET last_login_at = ($1 AT TIME ZONE 'UTC') WHERE id = $2")
            .bind(to)
            .bind(member.id)
            .execute(self.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET last_active_at = $1 WHERE user_id = $2")
            .bind(to)
            .bind(member.id)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn active(&self) -> u32 {
        seats::used(self.pool(), period_start(Utc::now()))
            .await
            .unwrap()
    }

    async fn users_with_email(&self, email: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn invitations_for(&self, email: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM invitations WHERE email = $1")
            .bind(email)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn is_admin(&self, member: &Member) -> bool {
        sqlx::query_scalar("SELECT COALESCE(is_admin, false) FROM users WHERE id = $1")
            .bind(member.id)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }
}

/// An identity provider configured on a server, and the requests that sign a person in through
/// it.
struct SingleSignOn {
    identity_provider: IdentityProvider,
    provider: Uuid,
}

impl SingleSignOn {
    async fn configure(server: &Server, admin: &Member) -> Self {
        let identity_provider = IdentityProvider::start().await;
        let provider = server
            .fixture
            .created(&admin.access, &identity_provider.provider())
            .await;
        Self {
            identity_provider,
            provider,
        }
    }

    async fn sign_in(
        &self,
        server: &Server,
        adjust: impl FnOnce(Claims) -> Claims,
    ) -> TestResponse {
        let client = &server.fixture.client;
        let response = client
            .post_json(
                &format!("{PUBLIC_PROVIDERS}/{}/authorizations", self.provider),
                &json!({}),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        let body = response.json_value();
        let state = body["state"].as_str().unwrap();
        let parameters: HashMap<String, String> = Url::parse(body["url"].as_str().unwrap())
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        let code = format!("code-{}", Uuid::new_v4().simple());
        self.identity_provider.grant(
            &code,
            &parameters["code_challenge"],
            adjust(self.identity_provider.claims(&parameters["nonce"])),
        );
        client
            .post_json(CALLBACKS, &json!({ "state": state, "code": code }))
            .await
    }
}

fn message() -> String {
    format!(
        "This server runs Zone Community, which allows {COMMUNITY_MEMBERS} active members. Add a Zone Team or Enterprise license to add more people."
    )
}

fn refused(response: &TestResponse) {
    response.assert_status(StatusCode::FORBIDDEN);
    assert_eq!(
        response.json_value(),
        json!({ "error": message(), "code": MEMBER_CAP_REACHED }),
    );
}

fn stranger(subject: &str, email: &str) -> impl FnOnce(Claims) -> Claims {
    let subject = subject.to_owned();
    let email = email.to_owned();
    move |claims| Claims {
        subject,
        email: Some(email),
        ..claims
    }
}

fn unchanged(claims: Claims) -> Claims {
    claims
}

#[tokio::test]
async fn the_fourth_sign_up_is_refused_and_leaves_no_account() {
    let server = Server::capped().await;
    server.members(3).await;
    let email = test_email();

    refused(&server.sign_up(&email).await);

    assert_eq!(server.users_with_email(&email).await, 0);
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_active_only_last_month_leaves_room_for_a_sign_up() {
    let server = Server::capped().await;
    let members = server.members(3).await;
    server
        .backdate(&members[1], period_start(Utc::now()) - TimeDelta::days(1))
        .await;

    server.join().await;

    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    server.cleanup().await;
}

#[tokio::test]
async fn the_first_sign_up_on_an_empty_server_is_admitted_as_the_admin() {
    let server = Server::capped().await;

    let first = server.join().await;

    assert!(server.is_admin(&first).await);
    server.cleanup().await;
}

#[tokio::test]
async fn existing_members_sign_in_and_refresh_over_the_cap() {
    let server = Server::uncapped().await;
    let members = server.members(5).await;
    server.fixture.state().cap_members();
    assert_eq!(server.active().await, COMMUNITY_MEMBERS + 2);
    refused(&server.sign_up(&test_email()).await);

    for member in &members {
        server
            .fixture
            .client
            .post_json(
                LOGIN,
                &json!({ "email": &member.email, "password": test_password() }),
            )
            .await
            .assert_status(StatusCode::OK);
        server
            .fixture
            .client
            .post_json(REFRESH, &json!({ "refresh_token": &member.refresh }))
            .await
            .assert_status(StatusCode::OK);
    }
    server.cleanup().await;
}

#[tokio::test]
async fn at_the_cap_only_existing_members_are_invited() {
    let server = Server::capped().await;
    let owner = server.join().await;
    let colleague = server.join().await;
    let early = test_email();

    server.invited(&owner, &early).await;
    server.join().await;
    let late = test_email();

    refused(&server.invite(&owner, &late).await);
    assert_eq!(server.invitations_for(&late).await, 0);
    server.invited(&owner, &colleague.email).await;
    assert_eq!(server.invitations_for(&early).await, 1);
    server.cleanup().await;
}

#[tokio::test]
async fn at_the_cap_only_an_existing_member_accepts_an_invitation() {
    let server = Server::capped().await;
    let owner = server.join().await;
    let colleague = server.join().await;
    let outsider = server.join().await;
    server.strand(&outsider).await;
    let outsider_invitation = server.invited(&owner, &outsider.email).await;
    let colleague_invitation = server.invited(&owner, &colleague.email).await;
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);

    refused(&server.accept(&outsider_invitation, &outsider).await);
    server
        .accept(&colleague_invitation, &colleague)
        .await
        .assert_status(StatusCode::OK);

    assert!(
        !seats::member(server.pool(), outsider.id).await.unwrap(),
        "the refused invitee joined nothing"
    );
    server.cleanup().await;
}

#[tokio::test]
async fn at_the_cap_only_an_existing_member_is_added_to_an_organization() {
    let server = Server::capped().await;
    let owner = server.join().await;
    let colleague = server.join().await;
    let outsider = server.join().await;
    server.strand(&outsider).await;
    let removed = server.join().await;
    server
        .add(&owner, &removed)
        .await
        .assert_status(StatusCode::CREATED);
    server.strand(&removed).await;
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);

    refused(&server.add(&owner, &outsider).await);
    refused(&server.add(&owner, &removed).await);
    server
        .add(&owner, &colleague)
        .await
        .assert_status(StatusCode::CREATED);

    assert!(!seats::member(server.pool(), outsider.id).await.unwrap());
    assert!(!seats::member(server.pool(), removed.id).await.unwrap());
    server.cleanup().await;
}

#[tokio::test]
async fn at_the_cap_only_an_existing_member_creates_an_organization() {
    let server = Server::capped().await;
    let members = server.members(2).await;
    let outsider = server.join().await;
    server.strand(&outsider).await;
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);

    refused(&server.create_organization(&outsider).await);
    server
        .create_organization(&members[1])
        .await
        .assert_status(StatusCode::CREATED);

    assert!(!seats::member(server.pool(), outsider.id).await.unwrap());
    server.cleanup().await;
}

#[tokio::test]
async fn a_sign_up_at_the_cap_is_admitted_only_under_a_license_that_lifts_it() {
    let server = Server::capped().await;
    server.members(3).await;
    let cases = [
        ("Team, active", team(), true),
        ("Enterprise without a plan, active", active(), true),
        ("Enterprise in its grace period", grace(), true),
        ("Enterprise lapsed past its grace period", lapsed(), false),
        ("Team lapsed past its grace period", lapsed_team(), false),
        (
            "a stored key that does not verify",
            UNVERIFIED_KEY.to_owned(),
            false,
        ),
    ];

    for (license, key, admitted) in cases {
        server.fixture.store_license(&key).await;
        let response = server.sign_up(&test_email()).await;
        if admitted {
            assert_eq!(response.status, StatusCode::CREATED, "{license}");
        } else {
            assert_eq!(
                (response.status, response.json_value()["code"].clone()),
                (StatusCode::FORBIDDEN, json!(MEMBER_CAP_REACHED)),
                "{license}"
            );
            refused(&response);
        }
    }
    server.cleanup().await;
}

#[tokio::test]
async fn single_sign_on_provisions_nobody_at_the_cap_but_signs_existing_members_in() {
    let server = Server::capped().await;
    let admin = server.join().await;
    server.fixture.store_license(&active()).await;
    let single_sign_on = SingleSignOn::configure(&server, &admin).await;
    let provisioned = single_sign_on.sign_in(&server, unchanged).await;
    provisioned.assert_status(StatusCode::OK);
    let provisioned = Member::from(&provisioned, testing::EMAIL);
    let local = server.join().await;
    sqlx::query("UPDATE users SET email_verified = true WHERE id = $1")
        .bind(local.id)
        .execute(server.pool())
        .await
        .unwrap();
    server.members(2).await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS + 2);

    server.fixture.store_license(&lapsed()).await;
    let lapsed_email = test_email();
    refused(
        &single_sign_on
            .sign_in(&server, stranger("lapsed-person", &lapsed_email))
            .await,
    );
    assert_eq!(server.users_with_email(&lapsed_email).await, 0);

    let signed_in = single_sign_on.sign_in(&server, unchanged).await;
    signed_in.assert_status(StatusCode::OK);
    assert_eq!(Member::from(&signed_in, testing::EMAIL).id, provisioned.id);

    let linked = single_sign_on
        .sign_in(&server, stranger("linked-person", &local.email))
        .await;
    linked.assert_status(StatusCode::OK);
    assert_eq!(Member::from(&linked, &local.email).id, local.id);

    server
        .fixture
        .client
        .delete_auth(LICENSE, &admin.access)
        .await
        .assert_status(StatusCode::OK);
    let unlicensed_email = test_email();
    refused(
        &single_sign_on
            .sign_in(&server, stranger("unlicensed-person", &unlicensed_email))
            .await,
    );
    assert_eq!(server.users_with_email(&unlicensed_email).await, 0);
    single_sign_on
        .sign_in(&server, unchanged)
        .await
        .assert_status(StatusCode::OK);
    server.cleanup().await;
}

#[tokio::test]
async fn a_member_is_an_active_user_with_an_active_membership_of_an_active_organization() {
    let server = Server::uncapped().await;
    let members = server.members(4).await;
    let [member, stranded, deactivated, disbanded] = &members[..] else {
        unreachable!("four members were registered");
    };
    server.strand(stranded).await;
    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(deactivated.id)
        .execute(server.pool())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE organizations SET is_active = false
         WHERE id IN (SELECT organization_id FROM organization_members WHERE user_id = $1)",
    )
    .bind(disbanded.id)
    .execute(server.pool())
    .await
    .unwrap();
    server
        .backdate(member, Utc::now() - TimeDelta::days(400))
        .await;

    assert!(seats::member(server.pool(), member.id).await.unwrap());
    for outsider in [stranded, deactivated, disbanded] {
        assert!(!seats::member(server.pool(), outsider.id).await.unwrap());
    }
    assert!(!seats::member(server.pool(), Uuid::new_v4()).await.unwrap());
    server.cleanup().await;
}
