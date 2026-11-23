//! The Community cap on active members: every path that would add a person refuses at the cap,
//! and nobody who is already a member is ever turned away.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use chrono::{DateTime, TimeDelta, Utc};
use futures::future::join_all;
use reqwest::Url;
use serde_json::{Value, json};
use sqlx::pool::PoolConnection;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres};
use uuid::Uuid;
use zone_enterprise::sso::{Claims, testing};
use zone_enterprise::{COMMUNITY_MEMBERS, MEMBER_ADMISSION_BUSY, MEMBER_CAP_REACHED, period_start};
use zone_server::db::seats;
use zone_server::routes::workspaces::OUTSIDE_ORGANIZATION;
use zone_server::services::members::SEATS_LOCK;
use zone_server::state::AppState;

use crate::common::identity_provider::IdentityProvider;
use crate::common::sso::{Fixture, active, grace, lapsed, lapsed_team, team};
use crate::common::{TestClient, TestResponse, test_config, test_email, test_password};

const PREFIX: &str = "member_cap";
const REGISTER: &str = "/api/auth/register";
const LOGIN: &str = "/api/auth/login";
const REFRESH: &str = "/api/auth/refresh";
const ORGANIZATIONS: &str = "/api/organizations";
const LICENSE: &str = "/api/license";
const PUBLIC_PROVIDERS: &str = "/api/auth/sso/providers";
const CALLBACKS: &str = "/api/auth/sso/callbacks";
const UNVERIFIED_KEY: &str = "garbage";
const ATTEMPTS: usize = 3;
const HOLD_SEATS: &str = "SELECT pg_advisory_lock($1)";
const RELEASE_SEATS: &str = "SELECT pg_advisory_unlock($1)";
const SINGLE_CONNECTION_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone)]
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

    async fn workspace(&self, owner: &Member) -> Uuid {
        sqlx::query_scalar(
            "SELECT workspace_id FROM workspace_members
             WHERE user_id = $1 AND role = 'owner' AND is_active
             ORDER BY created_at LIMIT 1",
        )
        .bind(owner.id)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    async fn add_to_workspace(&self, owner: &Member, member: &Member) -> TestResponse {
        let workspace = self.workspace(owner).await;
        self.post(
            &format!("/api/workspaces/{workspace}/members"),
            &json!({ "user_id": member.id, "role": "member" }),
            owner,
        )
        .await
    }

    async fn in_workspace(&self, owner: &Member, member: &Member) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM workspace_members
             WHERE workspace_id = $1 AND user_id = $2 AND is_active)",
        )
        .bind(self.workspace(owner).await)
        .bind(member.id)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    async fn activate(&self, owner: &Member, organization: Uuid, active: bool) -> TestResponse {
        self.fixture
            .client
            .patch_json_auth(
                &format!("{ORGANIZATIONS}/{organization}"),
                &json!({ "is_active": active }),
                &owner.access,
            )
            .await
    }

    async fn organization_active(&self, organization: Uuid) -> bool {
        sqlx::query_scalar("SELECT COALESCE(is_active, true) FROM organizations WHERE id = $1")
            .bind(organization)
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

    /// People who signed up and then left every organization: each counts again the moment a
    /// membership lands.
    async fn stranded(&self, count: usize) -> Vec<Member> {
        let mut stranded = Vec::with_capacity(count);
        for _ in 0..count {
            let member = self.join().await;
            self.strand(&member).await;
            stranded.push(member);
        }
        stranded
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

    /// Holds [`SEATS_LOCK`] on a session of its own until [`Server::release_seats`].
    async fn hold_seats(&self) -> PoolConnection<Postgres> {
        let mut holder = self.pool().acquire().await.unwrap();
        sqlx::query(HOLD_SEATS)
            .bind(SEATS_LOCK)
            .execute(&mut *holder)
            .await
            .unwrap();
        holder
    }

    async fn release_seats(mut holder: PoolConnection<Postgres>) {
        sqlx::query(RELEASE_SEATS)
            .bind(SEATS_LOCK)
            .execute(&mut *holder)
            .await
            .unwrap();
    }

    /// A capped server on this one's database whose pool has a single connection and whose
    /// license has never been read.
    async fn single_connection(&self) -> (TestClient, PgPool) {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(SINGLE_CONNECTION_WAIT)
            .connect_with((*self.pool().connect_options()).clone())
            .await
            .unwrap();
        let state = AppState::new(test_config(), pool.clone(), None);
        state.disable_mcp();
        state.cap_members();
        (TestClient::with_state(state), pool)
    }

    async fn cleanup(self) {
        self.fixture.cleanup().await;
    }

    async fn finish(server: Arc<Self>) {
        Arc::into_inner(server)
            .expect("every attempt has settled")
            .cleanup()
            .await;
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
        "This server allows {COMMUNITY_MEMBERS} active members without an active Zone Team or Enterprise license. Add a license to add more people."
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

fn outside(response: &TestResponse) {
    response.assert_status(StatusCode::CONFLICT);
    assert_eq!(response.json_value()["code"], json!(OUTSIDE_ORGANIZATION));
}

#[tokio::test]
async fn at_the_cap_a_stranded_account_is_not_added_to_a_workspace() {
    let server = Server::capped().await;
    let owner = server.join().await;
    server.join().await;
    let outsider = server.join().await;
    server.strand(&outsider).await;
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);

    outside(&server.add_to_workspace(&owner, &outsider).await);

    assert!(!server.in_workspace(&owner, &outsider).await);
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    server.cleanup().await;
}

#[tokio::test]
async fn only_a_member_of_its_organization_is_added_to_a_workspace() {
    let server = Server::uncapped().await;
    let owner = server.join().await;
    let stranger = server.join().await;

    outside(&server.add_to_workspace(&owner, &stranger).await);
    assert!(!server.in_workspace(&owner, &stranger).await);

    server
        .add(&owner, &stranger)
        .await
        .assert_status(StatusCode::CREATED);
    server
        .add_to_workspace(&owner, &stranger)
        .await
        .assert_status(StatusCode::CREATED);
    assert!(server.in_workspace(&owner, &stranger).await);
    server.cleanup().await;
}

#[tokio::test]
async fn at_the_cap_an_organization_whose_members_would_return_stays_inactive() {
    let server = Server::capped().await;
    let owner = server.join().await;
    let colleague = server.join().await;
    server
        .add(&owner, &colleague)
        .await
        .assert_status(StatusCode::CREATED);
    let organization = server.organization(&owner).await;
    server
        .activate(&owner, organization, false)
        .await
        .assert_status(StatusCode::OK);
    assert!(!seats::member(server.pool(), owner.id).await.unwrap());
    server.members(2).await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);

    refused(&server.activate(&owner, organization, true).await);

    assert!(!server.organization_active(organization).await);
    assert!(!seats::member(server.pool(), owner.id).await.unwrap());
    server.cleanup().await;
}

#[tokio::test]
async fn under_the_cap_an_organization_is_reactivated_counting_only_those_who_return() {
    let server = Server::capped().await;
    let owner = server.join().await;
    let colleague = server.join().await;
    server
        .add(&owner, &colleague)
        .await
        .assert_status(StatusCode::CREATED);
    let organization = server.organization(&owner).await;
    server
        .activate(&owner, organization, false)
        .await
        .assert_status(StatusCode::OK);
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS - 1);

    server
        .activate(&owner, organization, true)
        .await
        .assert_status(StatusCode::OK);

    assert!(server.organization_active(organization).await);
    assert!(seats::member(server.pool(), owner.id).await.unwrap());
    server.cleanup().await;
}

/// Runs every attempt on a task of its own, so password hashing on one never holds up another.
async fn concurrently<F>(attempts: impl IntoIterator<Item = F>) -> Vec<TestResponse>
where
    F: Future<Output = TestResponse> + Send + 'static,
{
    join_all(attempts.into_iter().map(tokio::spawn))
        .await
        .into_iter()
        .map(|settled| settled.expect("an attempt panicked"))
        .collect()
}

/// The index of the one attempt answered with `admitted`; every other one was refused at the cap.
fn admitted_once(responses: &[TestResponse], admitted: StatusCode) -> usize {
    let statuses: Vec<StatusCode> = responses.iter().map(|response| response.status).collect();
    let winners: Vec<usize> = statuses
        .iter()
        .enumerate()
        .filter(|(_, status)| **status == admitted)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        winners.len(),
        1,
        "exactly one of the attempts is admitted: {statuses:?}"
    );
    for (index, response) in responses.iter().enumerate() {
        if index != winners[0] {
            refused(response);
        }
    }
    winners[0]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sign_ups_one_below_the_cap_admit_exactly_one() {
    let server = Arc::new(Server::capped().await);
    server.members(2).await;
    let emails: Vec<String> = (0..ATTEMPTS).map(|_| test_email()).collect();

    let responses = concurrently(emails.iter().cloned().map(|email| {
        let server = server.clone();
        async move { server.sign_up(&email).await }
    }))
    .await;

    let admitted = admitted_once(&responses, StatusCode::CREATED);
    for (index, email) in emails.iter().enumerate() {
        if index != admitted {
            assert_eq!(server.users_with_email(email).await, 0, "{email}");
        }
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    Server::finish(server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_single_sign_ons_one_below_the_cap_provision_exactly_one() {
    let server = Arc::new(Server::capped().await);
    let admin = server.join().await;
    server.fixture.store_license(&active()).await;
    let single_sign_on = Arc::new(SingleSignOn::configure(&server, &admin).await);
    server.fixture.store_license(&lapsed()).await;
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS - 1);
    let emails: Vec<String> = (0..ATTEMPTS).map(|_| test_email()).collect();

    let responses = concurrently(emails.iter().cloned().enumerate().map(|(index, email)| {
        let server = server.clone();
        let single_sign_on = single_sign_on.clone();
        async move {
            single_sign_on
                .sign_in(&server, stranger(&format!("racer-{index}"), &email))
                .await
        }
    }))
    .await;

    let admitted = admitted_once(&responses, StatusCode::OK);
    for (index, email) in emails.iter().enumerate() {
        let expected = i64::from(index == admitted);
        assert_eq!(server.users_with_email(email).await, expected, "{email}");
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    drop(single_sign_on);
    Server::finish(server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_invitation_accepts_one_below_the_cap_admit_exactly_one() {
    let server = Arc::new(Server::capped().await);
    let invitees = server.stranded(ATTEMPTS).await;
    let owner = server.join().await;
    server.join().await;
    let mut tokens = Vec::with_capacity(ATTEMPTS);
    for invitee in &invitees {
        tokens.push(server.invited(&owner, &invitee.email).await);
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS - 1);

    let responses = concurrently(
        invitees
            .iter()
            .cloned()
            .zip(tokens)
            .map(|(invitee, token)| {
                let server = server.clone();
                async move { server.accept(&token, &invitee).await }
            }),
    )
    .await;

    let admitted = admitted_once(&responses, StatusCode::OK);
    for (index, invitee) in invitees.iter().enumerate() {
        assert_eq!(
            seats::member(server.pool(), invitee.id).await.unwrap(),
            index == admitted
        );
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    Server::finish(server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_adds_one_below_the_cap_admit_exactly_one() {
    let server = Arc::new(Server::capped().await);
    let targets = server.stranded(ATTEMPTS).await;
    let owner = server.join().await;
    server.join().await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS - 1);

    let responses = concurrently(targets.iter().cloned().map(|target| {
        let server = server.clone();
        let owner = owner.clone();
        async move { server.add(&owner, &target).await }
    }))
    .await;

    let admitted = admitted_once(&responses, StatusCode::CREATED);
    for (index, target) in targets.iter().enumerate() {
        assert_eq!(
            seats::member(server.pool(), target.id).await.unwrap(),
            index == admitted
        );
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    Server::finish(server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_organization_creates_one_below_the_cap_admit_exactly_one() {
    let server = Arc::new(Server::capped().await);
    let founders = server.stranded(ATTEMPTS).await;
    server.members(2).await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS - 1);

    let responses = concurrently(founders.iter().cloned().map(|founder| {
        let server = server.clone();
        async move { server.create_organization(&founder).await }
    }))
    .await;

    let admitted = admitted_once(&responses, StatusCode::CREATED);
    for (index, founder) in founders.iter().enumerate() {
        assert_eq!(
            seats::member(server.pool(), founder.id).await.unwrap(),
            index == admitted
        );
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    Server::finish(server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reactivations_one_below_the_cap_reactivate_exactly_one() {
    let server = Arc::new(Server::capped().await);
    let mut owners = Vec::with_capacity(ATTEMPTS);
    for _ in 0..ATTEMPTS {
        let owner = server.join().await;
        let organization = server.organization(&owner).await;
        server
            .activate(&owner, organization, false)
            .await
            .assert_status(StatusCode::OK);
        owners.push((owner, organization));
    }
    server.members(2).await;
    assert_eq!(server.active().await, COMMUNITY_MEMBERS - 1);

    let responses = concurrently(owners.iter().cloned().map(|(owner, organization)| {
        let server = server.clone();
        async move { server.activate(&owner, organization, true).await }
    }))
    .await;

    let admitted = admitted_once(&responses, StatusCode::OK);
    for (index, (_, organization)) in owners.iter().enumerate() {
        assert_eq!(
            server.organization_active(*organization).await,
            index == admitted
        );
    }
    assert_eq!(server.active().await, COMMUNITY_MEMBERS);
    Server::finish(server).await;
}

#[tokio::test]
async fn a_seat_lock_held_past_the_timeout_answers_busy() {
    let server = Server::capped().await;
    let holder = server.hold_seats().await;
    let email = test_email();

    let response = server.sign_up(&email).await;

    Server::release_seats(holder).await;
    response.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.json_value(),
        json!({
            "error": "Too many people are joining this server at once. Try again in a moment.",
            "code": MEMBER_ADMISSION_BUSY,
        })
    );
    assert_eq!(server.users_with_email(&email).await, 0);
    server.cleanup().await;
}

#[tokio::test]
async fn an_existing_member_joining_another_organization_waits_for_the_seat_lock() {
    let server = Server::capped().await;
    let member = server.join().await;
    let holder = server.hold_seats().await;

    let response = server.create_organization(&member).await;

    Server::release_seats(holder).await;
    response.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.json_value()["code"], json!(MEMBER_ADMISSION_BUSY));
    server
        .create_organization(&member)
        .await
        .assert_status(StatusCode::CREATED);
    server.cleanup().await;
}

#[tokio::test]
async fn an_admission_reads_the_cap_before_its_transaction_takes_the_only_connection() {
    let server = Server::capped().await;
    let founder = server.stranded(1).await.remove(0);
    let (client, pool) = server.single_connection().await;

    let response = client
        .post_json_auth(
            ORGANIZATIONS,
            &json!({ "name": "Single", "slug": format!("single-{}", Uuid::new_v4().simple()) }),
            &founder.access,
        )
        .await;

    drop(client);
    pool.close().await;
    response.assert_status(StatusCode::CREATED);
    assert!(seats::member(server.pool(), founder.id).await.unwrap());
    server.cleanup().await;
}
