use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use futures::future::join_all;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::Postgres;
use sqlx::pool::PoolConnection;
use uuid::Uuid;
use zone_enterprise::scim::{UserChanges, UserDocument};
use zone_server::auth::Permission;
use zone_server::auth::entitlement::ENTERPRISE_MESSAGE;
use zone_server::db::audit::actions;
use zone_server::db::organization_members::OrgRole;
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::services::members::{AdmissionError, SEATS_LOCK};
use zone_server::services::scim::{Entitlement, Failure, ScimClient, users};

use crate::common::TestResponse;
use crate::common::scim::{
    CONTENT_TYPE, ERROR_SCHEMA, Fixture, GROUP_SCHEMA, IssuedToken, PATCH_SCHEMA, Placeholders,
    USER_SCHEMA, assert_refused, assert_scim, replay, resource_id,
};
use crate::common::sso::{self, Account};
use crate::common::test_email;

const PREFIX: &str = "scim_license";
const HOLD_SEATS: &str = "SELECT pg_advisory_lock($1)";
const RELEASE_SEATS: &str = "SELECT pg_advisory_unlock($1)";
const ATTEMPTS: usize = 3;

struct Provisioned {
    token: IssuedToken,
    kept: Account,
    audited: Uuid,
    auditors: Uuid,
    deactivated: Uuid,
    replaced: Uuid,
    deleted: Uuid,
    group: Uuid,
    workspace: Uuid,
}

fn patch(operations: Value) -> Value {
    json!({ "schemas": [PATCH_SCHEMA], "Operations": operations })
}

fn user(user_name: &str) -> Value {
    json!({
        "schemas": [USER_SCHEMA],
        "userName": user_name,
        "displayName": "Grace Hopper",
        "emails": [{ "value": user_name, "type": "work", "primary": true }],
        "active": true,
    })
}

fn unentitled(response: &TestResponse) {
    assert_refused(response, StatusCode::FORBIDDEN, None);
    assert_eq!(response.json_value()["detail"], json!(ENTERPRISE_MESSAGE));
}

fn held(role: OrgRole, custom_role: Option<Uuid>) -> Option<(String, Option<Uuid>, bool)> {
    Some((role.as_str().to_owned(), custom_role, true))
}

async fn group(fixture: &Fixture, token: &str, name: &str, members: &[Uuid]) -> Uuid {
    let members: Vec<Value> = members
        .iter()
        .map(|member| json!({ "value": member }))
        .collect();
    let created = fixture
        .scim_post(
            "/Groups",
            token,
            &json!({ "schemas": [GROUP_SCHEMA], "displayName": name, "members": members }),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    resource_id(&created)
}

async fn provisioned(fixture: &Fixture) -> Provisioned {
    let token = fixture.token().await;
    let kept = fixture.stranger().await;
    fixture
        .scim_post(
            "/Users",
            &token.secret,
            &replay(
                "okta/create_user",
                &Placeholders {
                    user_name: Some(kept.email.clone()),
                    external_id: Some("00u-kept".to_owned()),
                    ..Placeholders::default()
                },
            ),
        )
        .await
        .assert_status(StatusCode::CREATED);
    let audited = fixture.provision(&token.secret, &test_email()).await;
    let deactivated = fixture.provision(&token.secret, &test_email()).await;
    let replaced = fixture.provision(&token.secret, &test_email()).await;
    let deleted = fixture.provision(&token.secret, &test_email()).await;

    let administrators = group(fixture, &token.secret, "Administrators", &[kept.id]).await;
    fixture
        .map(administrators, &json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    let auditors = fixture
        .custom_role(
            fixture.organization,
            "Auditors",
            &[Permission::AuditLogsRead],
        )
        .await;
    let auditing = group(fixture, &token.secret, "Auditing", &[audited]).await;
    fixture
        .map(auditing, &json!({ "custom_role_id": auditors }))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(fixture.holding(kept.id).await, held(OrgRole::Admin, None));
    assert_eq!(
        fixture.holding(audited).await,
        held(OrgRole::Member, Some(auditors))
    );

    let workspace = fixture.workspace().await;
    workspace_members::add_member(
        fixture.pool(),
        workspace,
        kept.id,
        WorkspaceRole::Member,
        None,
    )
    .await
    .unwrap();

    Provisioned {
        token,
        kept,
        audited,
        auditors,
        deactivated,
        replaced,
        deleted,
        group: administrators,
        workspace,
    }
}

async fn display_name(fixture: &Fixture, token: &str, user: Uuid) -> Value {
    fixture
        .scim_get(&format!("/Users/{user}"), token)
        .await
        .json_value()["displayName"]
        .clone()
}

async fn only_deprovisioning_is_accepted(fixture: &Fixture, provisioned: &Provisioned) {
    let secret = provisioned.token.secret.as_str();
    let kept = format!("/Users/{}", provisioned.kept.id);
    let group = format!("/Groups/{}", provisioned.group);

    unentitled(
        &fixture
            .scim_post("/Users", secret, &user(&test_email()))
            .await,
    );
    unentitled(
        &fixture
            .scim_patch(
                &kept,
                secret,
                &patch(json!([{ "op": "replace", "path": "displayName", "value": "Renamed" }])),
            )
            .await,
    );
    unentitled(
        &fixture
            .scim_put(&kept, secret, &user(&provisioned.kept.email))
            .await,
    );
    unentitled(
        &fixture
            .scim_patch(
                &kept,
                secret,
                &patch(json!([{ "op": "replace", "path": "active", "value": true }])),
            )
            .await,
    );
    unentitled(
        &fixture
            .scim_post(
                "/Groups",
                secret,
                &json!({ "schemas": [GROUP_SCHEMA], "displayName": "Late" }),
            )
            .await,
    );
    unentitled(
        &fixture
            .scim_patch(
                &group,
                secret,
                &patch(json!([{
                    "op": "remove",
                    "path": format!("members[value eq \"{}\"]", provisioned.kept.id),
                }])),
            )
            .await,
    );
    unentitled(
        &fixture
            .scim_put(
                &group,
                secret,
                &json!({ "schemas": [GROUP_SCHEMA], "displayName": "Renamed", "members": [] }),
            )
            .await,
    );
    unentitled(&fixture.scim_delete(&group, secret).await);

    for read in [
        "/Users".to_owned(),
        kept.clone(),
        "/Groups".to_owned(),
        group.clone(),
        "/ServiceProviderConfig".to_owned(),
        "/Schemas".to_owned(),
    ] {
        assert_scim(&fixture.scim_get(&read, secret).await, StatusCode::OK);
    }
    let roster = fixture.scim_get(&group, secret).await.json_value();
    assert_eq!(roster["displayName"], json!("Administrators"));
    assert_eq!(roster["members"][0]["value"], json!(provisioned.kept.id));

    let deactivating = format!("/Users/{}", provisioned.deactivated);
    let before = display_name(fixture, secret, provisioned.deactivated).await;
    let deactivated = fixture
        .scim_patch(
            &deactivating,
            secret,
            &patch(json!([
                { "op": "replace", "path": "displayName", "value": "Renamed" },
                { "op": "replace", "value": { "active": false, "name.givenName": "Renamed" } },
            ])),
        )
        .await;
    assert_scim(&deactivated, StatusCode::OK);
    let body = deactivated.json_value();
    assert_eq!(body["active"], json!(false));
    assert_eq!(body["displayName"], before, "only the deactivation applies");
    assert_eq!(body["name"]["givenName"], json!("Ada"));
    unentitled(
        &fixture
            .scim_patch(
                &deactivating,
                secret,
                &patch(json!([{ "op": "replace", "path": "active", "value": true }])),
            )
            .await,
    );
    assert!(!fixture.holding(provisioned.deactivated).await.unwrap().2);

    let replacing = format!("/Users/{}", provisioned.replaced);
    let before = display_name(fixture, secret, provisioned.replaced).await;
    let replacement = json!({
        "schemas": [USER_SCHEMA],
        "userName": test_email(),
        "displayName": "Replaced",
        "emails": [{ "value": test_email(), "primary": true }],
        "active": false,
    });
    let replaced = fixture.scim_put(&replacing, secret, &replacement).await;
    assert_scim(&replaced, StatusCode::OK);
    let body = replaced.json_value();
    assert_eq!(body["active"], json!(false));
    assert_eq!(body["displayName"], before);
    assert_ne!(body["userName"], replacement["userName"]);

    let deleted = fixture
        .scim_delete(&format!("/Users/{}", provisioned.deleted), secret)
        .await;
    deleted.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.holding(provisioned.deleted).await, None);

    assert_eq!(
        fixture.holding(provisioned.kept.id).await,
        held(OrgRole::Admin, None),
        "the lapse changed a role"
    );
    assert_eq!(
        fixture.holding(provisioned.audited).await,
        held(OrgRole::Member, Some(provisioned.auditors))
    );
    let mapping: (Option<String>, Option<Uuid>) =
        sqlx::query_as("SELECT role, custom_role_id FROM scim_groups WHERE id = $1")
            .bind(provisioned.group)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(mapping, (Some("admin".to_owned()), None));
    assert!(fixture.signed_in(&provisioned.kept.token).await);
    fixture
        .inner
        .client
        .get_auth(
            &format!("/api/workspaces/{}", provisioned.workspace),
            &provisioned.kept.token,
        )
        .await
        .assert_status(StatusCode::OK);
    assert!(
        workspace_members::can_read(fixture.pool(), provisioned.workspace, provisioned.kept.id)
            .await
            .unwrap()
    );

    let refused: Vec<i16> = sqlx::query_scalar(
        "SELECT status FROM scim_errors WHERE organization_id = $1 ORDER BY occurred_at, id",
    )
    .bind(fixture.organization)
    .fetch_all(fixture.pool())
    .await
    .unwrap();
    assert_eq!(refused, vec![403; 9], "every refusal is recorded");
    let mut written = vec![actions::SCIM_GROUP_CREATED; 2];
    written.extend([actions::SCIM_USER_CREATED; 5]);
    written.extend([actions::SCIM_USER_DEACTIVATED; 2]);
    written.push(actions::SCIM_USER_DELETED);
    assert_eq!(
        fixture.scim_actions().await,
        written,
        "a refused or narrowed write audited more than the deactivation"
    );

    fixture
        .inner
        .client
        .delete_auth(
            &format!(
                "/api/organizations/{}/scim/tokens/{}",
                fixture.organization, provisioned.token.id
            ),
            &fixture.owner.token,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_refused(
        &fixture.scim_get("/Users", secret).await,
        StatusCode::UNAUTHORIZED,
        None,
    );
}

#[tokio::test]
async fn a_license_in_grace_accepts_every_write() {
    let fixture = Fixture::new(PREFIX).await;
    let provisioned = provisioned(&fixture).await;
    fixture.inner.store_license(&sso::grace()).await;
    let secret = provisioned.token.secret.as_str();

    let created = fixture.provision(secret, &test_email()).await;
    let path = format!("/Users/{created}");
    let renamed = fixture
        .scim_patch(
            &path,
            secret,
            &patch(json!([{ "op": "replace", "path": "displayName", "value": "Renamed" }])),
        )
        .await;
    assert_scim(&renamed, StatusCode::OK);
    assert_eq!(renamed.json_value()["displayName"], json!("Renamed"));
    let email = test_email();
    assert_scim(
        &fixture.scim_put(&path, secret, &user(&email)).await,
        StatusCode::OK,
    );
    let deactivated = fixture
        .scim_patch(
            &path,
            secret,
            &patch(json!([{ "op": "replace", "path": "active", "value": false }])),
        )
        .await;
    assert_eq!(deactivated.json_value()["active"], json!(false));
    let reactivated = fixture
        .scim_patch(
            &path,
            secret,
            &patch(json!([{ "op": "replace", "path": "active", "value": true }])),
        )
        .await;
    assert_scim(&reactivated, StatusCode::OK);
    assert_eq!(reactivated.json_value()["active"], json!(true));

    let added = group(&fixture, secret, "Late", &[created]).await;
    let group_path = format!("/Groups/{added}");
    assert_scim(
        &fixture
            .scim_patch(
                &group_path,
                secret,
                &patch(json!([{ "op": "remove", "path": "members" }])),
            )
            .await,
        StatusCode::OK,
    );
    assert_scim(
        &fixture
            .scim_put(
                &group_path,
                secret,
                &json!({ "schemas": [GROUP_SCHEMA], "displayName": "Later" }),
            )
            .await,
        StatusCode::OK,
    );
    fixture
        .scim_delete(&group_path, secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    fixture
        .scim_delete(&path, secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_enterprise_license_accepts_only_deprovisioning() {
    let fixture = Fixture::new(PREFIX).await;
    let provisioned = provisioned(&fixture).await;
    fixture.inner.store_license(&sso::lapsed()).await;

    only_deprovisioning_is_accepted(&fixture, &provisioned).await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_team_license_accepts_only_deprovisioning() {
    let fixture = Fixture::new(PREFIX).await;
    let provisioned = provisioned(&fixture).await;
    fixture.inner.install(&fixture.owner, &sso::team()).await;

    only_deprovisioning_is_accepted(&fixture, &provisioned).await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_removed_license_accepts_only_deprovisioning() {
    let fixture = Fixture::new(PREFIX).await;
    let provisioned = provisioned(&fixture).await;
    fixture.inner.remove_license(&fixture.owner).await;

    only_deprovisioning_is_accepted(&fixture, &provisioned).await;

    fixture.cleanup().await;
}

#[tokio::test]
async fn enterprise_provisions_past_the_community_cap_and_community_provisions_nobody() {
    let fixture = Fixture::capped(PREFIX).await;
    let token = fixture.token().await;
    fixture.member(OrgRole::Member).await;
    fixture.member(OrgRole::Member).await;

    let fourth = fixture.provision(&token.secret, &test_email()).await;
    let fifth = fixture.provision(&token.secret, &test_email()).await;
    for member in [fourth, fifth] {
        assert_eq!(fixture.holding(member).await, held(OrgRole::Member, None));
    }

    fixture.inner.remove_license(&fixture.owner).await;
    let email = test_email();
    unentitled(
        &fixture
            .scim_post("/Users", &token.secret, &user(&email))
            .await,
    );
    let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE lower(email) = $1")
        .bind(email.to_lowercase())
        .fetch_one(fixture.pool())
        .await
        .unwrap();
    assert_eq!(accounts, 0, "the refusal created an account");
    fixture
        .scim_delete(&format!("/Users/{fifth}"), &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    fixture.cleanup().await;
}

fn lapsing(fixture: &Fixture, token: &IssuedToken) -> ScimClient {
    ScimClient {
        organization_id: fixture.organization,
        token_id: token.id,
        token_name: "Okta".to_owned(),
        entitlement: Entitlement::Full,
    }
}

fn document(user_name: &str) -> UserDocument {
    serde_json::from_value(user(user_name)).unwrap()
}

async fn hold_seats(fixture: &Fixture) -> PoolConnection<Postgres> {
    let mut holder = fixture.pool().acquire().await.unwrap();
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

async fn accounts(fixture: &Fixture, email: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE lower(email) = $1")
        .bind(email.to_lowercase())
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

async fn answered(failure: Failure) -> (StatusCode, Option<String>, Value) {
    let response = failure.into_response();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        content_type,
        serde_json::from_slice(&bytes).unwrap(),
    )
}

async fn assert_busy(failure: Failure) {
    assert!(matches!(failure, Failure::Busy(_)), "{failure:?}");
    assert_eq!(
        answered(failure).await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Some(CONTENT_TYPE.to_owned()),
            json!({
                "schemas": [ERROR_SCHEMA],
                "status": "503",
                "detail": AdmissionError::Busy.to_string(),
            })
        )
    );
}

#[tokio::test]
async fn a_provisioning_counts_the_cap_under_the_seat_lock_and_answers_busy_in_scim_format() {
    let fixture = Fixture::capped(PREFIX).await;
    let token = fixture.token().await;
    fixture.inner.remove_license(&fixture.owner).await;
    let client = lapsing(&fixture, &token);
    let email = test_email();
    let holder = hold_seats(&fixture).await;

    let refused = users::create(fixture.state(), &client, &document(&email)).await;

    release_seats(holder).await;
    assert_busy(refused.expect_err("a held seat lock admits nobody")).await;
    assert_eq!(
        accounts(&fixture, &email).await,
        0,
        "the refusal created an account"
    );

    let admitted = users::create(fixture.state(), &client, &document(&email))
        .await
        .expect("the released seat lock admits the newcomer");
    assert_eq!(
        fixture.holding(admitted.id).await,
        held(OrgRole::Member, None)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reactivation_counts_the_cap_under_the_seat_lock_and_answers_busy_in_scim_format() {
    let fixture = Fixture::capped(PREFIX).await;
    let token = fixture.token().await;
    let user = fixture.provision(&token.secret, &test_email()).await;
    fixture
        .scim_patch(
            &format!("/Users/{user}"),
            &token.secret,
            &patch(json!([{ "op": "replace", "value": { "active": false } }])),
        )
        .await
        .assert_status(StatusCode::OK);
    fixture.inner.remove_license(&fixture.owner).await;
    let client = lapsing(&fixture, &token);
    let reactivation = UserChanges {
        active: Some(true),
        ..UserChanges::default()
    };
    let holder = hold_seats(&fixture).await;

    let refused = users::update(fixture.state(), &client, user, reactivation.clone()).await;

    release_seats(holder).await;
    assert_busy(refused.expect_err("a held seat lock reactivates nobody")).await;
    assert_eq!(
        fixture.holding(user).await,
        Some((OrgRole::Member.as_str().to_owned(), None, false))
    );

    users::update(fixture.state(), &client, user, reactivation)
        .await
        .expect("the released seat lock reactivates the member");
    assert_eq!(fixture.holding(user).await, held(OrgRole::Member, None));

    fixture.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_provisionings_one_below_the_cap_admit_exactly_one() {
    let fixture = Fixture::capped(PREFIX).await;
    let token = fixture.token().await;
    fixture.member(OrgRole::Member).await;
    let mut stranded = Vec::with_capacity(ATTEMPTS);
    for _ in 0..ATTEMPTS {
        let account = fixture.stranger().await;
        sqlx::query("UPDATE organization_members SET is_active = false WHERE user_id = $1")
            .bind(account.id)
            .execute(fixture.pool())
            .await
            .unwrap();
        stranded.push(account);
    }
    fixture.inner.remove_license(&fixture.owner).await;
    let client = lapsing(&fixture, &token);
    let documents: Vec<UserDocument> = stranded
        .iter()
        .map(|account| document(&account.email))
        .collect();

    let settled = join_all(
        documents
            .iter()
            .map(|document| users::create(fixture.state(), &client, document)),
    )
    .await;

    let admitted = settled.iter().filter(|settled| settled.is_ok()).count();
    assert_eq!(
        admitted, 1,
        "exactly one provisioning is admitted: {settled:?}"
    );
    for refused in settled.into_iter().filter_map(Result::err) {
        assert!(
            matches!(refused, Failure::Forbidden(_) | Failure::Busy(_)),
            "{refused:?}"
        );
    }
    let mut seated = 0;
    for account in &stranded {
        if fixture.holding(account.id).await.is_some() {
            seated += 1;
        }
    }
    assert_eq!(seated, 1, "only the admitted provisioning seated anyone");

    fixture.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reactivations_one_below_the_cap_admit_exactly_one() {
    let fixture = Fixture::capped(PREFIX).await;
    let token = fixture.token().await;
    fixture.member(OrgRole::Member).await;
    let mut deactivated = Vec::with_capacity(ATTEMPTS);
    for _ in 0..ATTEMPTS {
        let account = fixture.stranger().await;
        sqlx::query("UPDATE organization_members SET is_active = false WHERE user_id = $1")
            .bind(account.id)
            .execute(fixture.pool())
            .await
            .unwrap();
        assert_scim(
            &fixture
                .scim_post("/Users", &token.secret, &user(&account.email))
                .await,
            StatusCode::CREATED,
        );
        assert_scim(
            &fixture
                .scim_patch(
                    &format!("/Users/{}", account.id),
                    &token.secret,
                    &patch(json!([{ "op": "replace", "value": { "active": false } }])),
                )
                .await,
            StatusCode::OK,
        );
        deactivated.push(account.id);
    }
    fixture.inner.remove_license(&fixture.owner).await;
    let client = lapsing(&fixture, &token);
    let reactivation = UserChanges {
        active: Some(true),
        ..UserChanges::default()
    };

    let settled = join_all(
        deactivated
            .iter()
            .map(|user| users::update(fixture.state(), &client, *user, reactivation.clone())),
    )
    .await;

    let admitted = settled.iter().filter(|settled| settled.is_ok()).count();
    assert_eq!(
        admitted, 1,
        "exactly one reactivation is admitted: {settled:?}"
    );
    for refused in settled.into_iter().filter_map(Result::err) {
        assert!(
            matches!(refused, Failure::Forbidden(_) | Failure::Busy(_)),
            "{refused:?}"
        );
    }
    let mut active = 0;
    for user in &deactivated {
        if fixture
            .holding(*user)
            .await
            .is_some_and(|(.., active)| active)
        {
            active += 1;
        }
    }
    assert_eq!(active, 1, "only the admitted reactivation seated anyone");

    fixture.cleanup().await;
}
