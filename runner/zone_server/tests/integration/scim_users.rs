//! The SCIM `/Users` endpoints as an identity provider calls them: provisioning, reading, filtering
//! and paging, updating, deactivating and reactivating, and deleting organization members, with
//! the SCIM content type and error format on every answer.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::entitlement::ENTERPRISE_MESSAGE;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::audit::actions;
use zone_server::db::custom_roles::{self, Creation, Definition};
use zone_server::db::organization_members::OrgRole;
use zone_server::db::workspace_members::{self, WorkspaceRole};

use crate::common::TestResponse;
use crate::common::scim::{self, CONTENT_TYPE, ERROR_SCHEMA, Fixture, PATCH_SCHEMA, USER_SCHEMA};
use crate::common::sso::{self as sso_fixture};

const PREFIX: &str = "scim_users";

fn address() -> String {
    format!("{}@acme.test", Uuid::new_v4())
}

fn user(user_name: &str, external_id: &str) -> Value {
    json!({
        "schemas": [USER_SCHEMA],
        "userName": user_name,
        "externalId": external_id,
        "name": { "givenName": "Grace", "familyName": "Hopper" },
        "displayName": "Grace Hopper",
        "emails": [{ "value": user_name, "type": "work", "primary": true }],
        "active": true,
    })
}

fn patch(operations: Value) -> Value {
    json!({ "schemas": [PATCH_SCHEMA], "Operations": operations })
}

fn deactivation() -> Value {
    patch(json!([{ "op": "replace", "value": { "active": false } }]))
}

fn reactivation() -> Value {
    patch(json!([{ "op": "replace", "path": "active", "value": true }]))
}

fn assert_scim(response: &TestResponse, status: StatusCode) {
    response.assert_status(status);
    assert_eq!(
        response.header("content-type"),
        Some(CONTENT_TYPE),
        "{}",
        response.text()
    );
}

fn assert_refused(response: &TestResponse, status: StatusCode, scim_type: Option<&str>) {
    assert_scim(response, status);
    let body = response.json_value();
    assert_eq!(body["schemas"], json!([ERROR_SCHEMA]));
    assert_eq!(body["status"], json!(status.as_u16().to_string()));
    match scim_type {
        Some(scim_type) => assert_eq!(body["scimType"], json!(scim_type), "{body}"),
        None => assert!(body.get("scimType").is_none(), "{body}"),
    }
}

fn id(response: &TestResponse) -> Uuid {
    response.json_value()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

async fn recorded(fixture: &Fixture) -> Vec<(String, String, i16, Option<String>)> {
    sqlx::query_as(
        "SELECT method, path, status, scim_type FROM scim_errors \
         WHERE organization_id = $1 ORDER BY occurred_at, id",
    )
    .bind(fixture.organization)
    .fetch_all(fixture.pool())
    .await
    .unwrap()
}

async fn account(fixture: &Fixture, user: Uuid) -> (String, Option<String>, bool, String) {
    sqlx::query_as(
        "SELECT email, display_name, email_verified, password_hash FROM users WHERE id = $1",
    )
    .bind(user)
    .fetch_one(fixture.pool())
    .await
    .unwrap()
}

async fn provisioned_password(fixture: &Fixture, user: Uuid) -> Option<Option<String>> {
    sqlx::query_scalar(
        "SELECT provisioned_password FROM scim_users WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(user)
    .fetch_optional(fixture.pool())
    .await
    .unwrap()
}

async fn memberships(fixture: &Fixture, user: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM organization_members WHERE user_id = $1")
        .bind(user)
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

async fn seat(fixture: &Fixture, workspace: Uuid, user: Uuid) -> Option<(String, bool)> {
    sqlx::query_as(
        "SELECT role, is_active FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(user)
    .fetch_optional(fixture.pool())
    .await
    .unwrap()
}

async fn organization_role(fixture: &Fixture, name: &str) -> Uuid {
    let grant = Grant::compose(
        Scope::Organization,
        &[Permission::AuditLogsRead.name().to_owned()],
    )
    .unwrap();
    let definition = Definition {
        name,
        description: None,
        scope: Scope::Organization,
        grant: &grant,
    };
    match custom_roles::create(
        fixture.pool(),
        fixture.organization,
        &definition,
        fixture.owner.id,
    )
    .await
    .unwrap()
    {
        Creation::Created(role) => role.id,
        Creation::NameTaken => panic!("{name} is already taken"),
    }
}

#[tokio::test]
async fn a_provisioned_user_is_created_read_found_and_deleted() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let email = address();

    let created = fixture
        .scim_post("/Users", &token.secret, &user(&email, "00u-grace"))
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let body = created.json_value();
    let user_id = id(&created);
    let location = format!("http://localhost:3000{}/Users/{user_id}", fixture.base());
    assert_eq!(created.header("location"), Some(location.as_str()));
    assert_eq!(body["schemas"], json!([USER_SCHEMA]));
    assert_eq!(body["userName"], json!(email));
    assert_eq!(body["externalId"], json!("00u-grace"));
    assert_eq!(body["displayName"], json!("Grace Hopper"));
    assert_eq!(body["name"]["givenName"], json!("Grace"));
    assert_eq!(body["emails"][0]["value"], json!(email));
    assert_eq!(body["active"], json!(true));
    assert_eq!(body["meta"]["resourceType"], json!("User"));
    assert_eq!(body["meta"]["location"], json!(location));

    let (stored_email, display_name, verified, password_hash) = account(&fixture, user_id).await;
    assert_eq!(stored_email, email);
    assert_eq!(display_name.as_deref(), Some("Grace Hopper"));
    assert!(!verified, "a provisioned email is never verified");
    assert_eq!(
        provisioned_password(&fixture, user_id).await,
        Some(Some(password_hash))
    );
    assert_eq!(
        memberships(&fixture, user_id).await,
        1,
        "a provisioned account gets no organization of its own"
    );
    assert_eq!(
        fixture.holding(user_id).await,
        Some((OrgRole::Member.as_str().to_owned(), None, true))
    );

    let read = fixture
        .scim_get(&format!("/Users/{user_id}"), &token.secret)
        .await;
    assert_scim(&read, StatusCode::OK);
    assert_eq!(read.json_value(), body);

    let found = fixture
        .scim_get(
            &format!(
                "/Users?filter=userName%20eq%20%22{}%22",
                email.to_uppercase()
            ),
            &token.secret,
        )
        .await;
    assert_scim(&found, StatusCode::OK);
    let found = found.json_value();
    assert_eq!(
        found["schemas"],
        json!(["urn:ietf:params:scim:api:messages:2.0:ListResponse"])
    );
    assert_eq!(found["totalResults"], json!(1));
    assert_eq!(found["Resources"][0]["id"], json!(user_id));

    let created_audits = fixture.audits(actions::SCIM_USER_CREATED).await;
    assert_eq!(created_audits.len(), 1);
    assert_eq!(created_audits[0].actor_id, None);
    assert_eq!(
        created_audits[0].organization_id,
        Some(fixture.organization)
    );
    assert_eq!(created_audits[0].resource_id, Some(user_id));
    let values = created_audits[0].new_values.as_ref().unwrap();
    assert_eq!(values["scim_token_id"], json!(token.id));
    assert_eq!(values["scim_token_name"], json!("Identity provider"));
    assert_eq!(values["user_name"], json!(email));
    assert!(!fixture.inner.audit_text().await.contains(&token.secret));

    let deleted = fixture
        .scim_delete(&format!("/Users/{user_id}"), &token.secret)
        .await;
    deleted.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.holding(user_id).await, None);
    assert_eq!(
        provisioned_password(&fixture, user_id).await,
        None,
        "deleting drops the link"
    );
    let gone = fixture
        .scim_get(&format!("/Users/{user_id}"), &token.secret)
        .await;
    assert_refused(&gone, StatusCode::NOT_FOUND, None);
    assert_eq!(
        account(&fixture, user_id).await.0,
        email,
        "deleting keeps the Zone account"
    );
    let deleted_audits = fixture.audits(actions::SCIM_USER_DELETED).await;
    assert_eq!(deleted_audits.len(), 1);
    assert_eq!(deleted_audits[0].actor_id, None);

    fixture.cleanup().await;
}

#[tokio::test]
async fn users_are_filtered_and_paged() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let mut created = Vec::new();
    for index in 0..3 {
        let response = fixture
            .scim_post(
                "/Users",
                &token.secret,
                &user(&address(), &format!("external-{index}")),
            )
            .await;
        response.assert_status(StatusCode::CREATED);
        created.push(id(&response));
    }

    let everyone = fixture.scim_get("/Users", &token.secret).await;
    assert_scim(&everyone, StatusCode::OK);
    let everyone = everyone.json_value();
    assert_eq!(
        everyone["totalResults"],
        json!(4),
        "the owner is a member too"
    );
    assert_eq!(everyone["startIndex"], json!(1));

    let page = fixture
        .scim_get("/Users?startIndex=2&count=2", &token.secret)
        .await
        .json_value();
    assert_eq!(page["totalResults"], json!(4));
    assert_eq!(page["startIndex"], json!(2));
    assert_eq!(page["itemsPerPage"], json!(2));
    assert_eq!(page["Resources"][0]["id"], json!(created[0]));
    assert_eq!(page["Resources"][1]["id"], json!(created[1]));

    let counted = fixture
        .scim_get("/Users?count=0", &token.secret)
        .await
        .json_value();
    assert_eq!(counted["totalResults"], json!(4));
    assert_eq!(counted["Resources"], json!([]));

    let external = fixture
        .scim_get("/Users?filter=externalId%20eq%20external-2", &token.secret)
        .await
        .json_value();
    assert_eq!(external["totalResults"], json!(1));
    assert_eq!(external["Resources"][0]["id"], json!(created[2]));

    let by_id = fixture
        .scim_get(
            &format!("/Users?filter=id%20eq%20%22{}%22", created[1]),
            &token.secret,
        )
        .await
        .json_value();
    assert_eq!(by_id["totalResults"], json!(1));

    for impossible in [
        "/Users?filter=id%20eq%20jyoung",
        "/Users?filter=userName%20eq%20%22non-existent%20user%22",
        "/Users?filter=members%20eq%20%22someone%22",
    ] {
        let none = fixture.scim_get(impossible, &token.secret).await;
        assert_scim(&none, StatusCode::OK);
        assert_eq!(none.json_value()["totalResults"], json!(0), "{impossible}");
    }

    let unsupported = fixture
        .scim_get("/Users?filter=userName%20co%20%22a%22", &token.secret)
        .await;
    assert_refused(&unsupported, StatusCode::BAD_REQUEST, Some("invalidFilter"));
    let unreadable = fixture
        .scim_get("/Users?startIndex=first", &token.secret)
        .await;
    assert_refused(&unreadable, StatusCode::BAD_REQUEST, Some("invalidSyntax"));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_caller_without_a_valid_token_is_challenged_and_nothing_is_recorded() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let other = fixture.another_organization().await;
    let foreign = fixture.token_for(other).await;
    let revoked = fixture.token().await;
    zone_server::db::scim::tokens::revoke(fixture.pool(), fixture.organization, revoked.id)
        .await
        .unwrap()
        .unwrap();
    let uri = format!("{}/Users", fixture.base());

    for secret in [
        String::new(),
        "zscim_not-a-real-secret".to_owned(),
        foreign.secret.clone(),
        revoked.secret.clone(),
        fixture.owner.token.clone(),
    ] {
        let response = fixture.scim(Method::GET, &uri, &secret, None).await;
        assert_refused(&response, StatusCode::UNAUTHORIZED, None);
        assert_eq!(response.header("www-authenticate"), Some("Bearer"));
    }
    let wrong_scheme = fixture
        .inner
        .client
        .send_request(
            axum::http::Request::builder()
                .uri(&uri)
                .header("Authorization", format!("Basic {}", token.secret))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    assert_refused(&wrong_scheme, StatusCode::UNAUTHORIZED, None);

    let malformed = fixture
        .scim(
            Method::GET,
            &format!("{}/not-an-organization/Users", scim::BASE_PATH),
            &token.secret,
            None,
        )
        .await;
    assert_refused(&malformed, StatusCode::NOT_FOUND, None);

    let elsewhere = fixture
        .scim(
            Method::GET,
            &format!("{}/Users", scim::base(other)),
            &token.secret,
            None,
        )
        .await;
    assert_refused(&elsewhere, StatusCode::UNAUTHORIZED, None);

    assert!(recorded(&fixture).await.is_empty());
    let accepted = fixture.scim_get("/Users", &token.secret).await;
    assert_scim(&accepted, StatusCode::OK);

    fixture.cleanup().await;
}

#[tokio::test]
async fn unknown_and_foreign_users_are_not_found_and_each_refusal_is_recorded() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let other = fixture.another_organization().await;
    let outsider = fixture.member_of(other, OrgRole::Member).await;

    for path in [
        format!("/Users/{}", Uuid::new_v4()),
        "/Users/jyoung".to_owned(),
        format!("/Users/{}", outsider.id),
    ] {
        let read = fixture.scim_get(&path, &token.secret).await;
        assert_refused(&read, StatusCode::NOT_FOUND, None);
        let changed = fixture
            .scim_patch(&path, &token.secret, &deactivation())
            .await;
        assert_refused(&changed, StatusCode::NOT_FOUND, None);
        let deleted = fixture.scim_delete(&path, &token.secret).await;
        assert_refused(&deleted, StatusCode::NOT_FOUND, None);
    }
    assert_eq!(
        fixture.holding(outsider.id).await,
        None,
        "nothing reached the other organization"
    );

    let unrouted = fixture.scim_get("/Bulk", &token.secret).await;
    assert_refused(&unrouted, StatusCode::NOT_FOUND, None);
    let unallowed = fixture
        .scim_post(
            &format!("/Users/{}", Uuid::new_v4()),
            &token.secret,
            &json!({}),
        )
        .await;
    assert_refused(&unallowed, StatusCode::METHOD_NOT_ALLOWED, None);

    let errors = recorded(&fixture).await;
    assert_eq!(errors.len(), 9, "{errors:?}");
    let template = format!("{}/{{org_id}}/Users/{{user_id}}", scim::BASE_PATH);
    assert!(
        errors
            .iter()
            .all(|(_, path, status, _)| { *path == template && *status == 404 })
    );
    assert_eq!(
        errors
            .iter()
            .map(|(method, ..)| method.as_str())
            .collect::<Vec<_>>()[..3],
        ["GET", "PATCH", "DELETE"]
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn duplicates_bad_documents_and_the_last_owner_are_refused() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let email = address();
    fixture.provision(&token.secret, &email).await;

    let again = fixture
        .scim_post("/Users", &token.secret, &user(&email, "another"))
        .await;
    assert_refused(&again, StatusCode::CONFLICT, Some("uniqueness"));

    let renamed_twin = fixture
        .scim_post(
            "/Users",
            &token.secret,
            &json!({ "userName": email.to_uppercase(), "emails": [{ "value": address() }] }),
        )
        .await;
    assert_refused(&renamed_twin, StatusCode::CONFLICT, Some("uniqueness"));

    let already = fixture.member(OrgRole::Admin).await;
    let member = fixture
        .scim_post("/Users", &token.secret, &user(&already.email, "admin"))
        .await;
    assert_refused(&member, StatusCode::CONFLICT, Some("uniqueness"));

    let addressless = fixture
        .scim_post("/Users", &token.secret, &json!({ "userName": "jyoung" }))
        .await;
    assert_refused(&addressless, StatusCode::BAD_REQUEST, Some("invalidValue"));
    let blank = fixture
        .scim_post("/Users", &token.secret, &json!({ "userName": " " }))
        .await;
    assert_refused(&blank, StatusCode::BAD_REQUEST, Some("invalidValue"));
    let unreadable = fixture
        .scim(
            Method::POST,
            &format!("{}/Users", fixture.base()),
            &token.secret,
            None,
        )
        .await;
    assert_refused(&unreadable, StatusCode::BAD_REQUEST, Some("invalidSyntax"));
    let unknown_op = fixture
        .scim_patch(
            &format!("/Users/{}", fixture.owner.id),
            &token.secret,
            &patch(json!([{ "op": "move", "path": "active", "value": false }])),
        )
        .await;
    assert_refused(&unknown_op, StatusCode::BAD_REQUEST, Some("invalidSyntax"));

    let owner = format!("/Users/{}", fixture.owner.id);
    let deactivated = fixture
        .scim_patch(&owner, &token.secret, &deactivation())
        .await;
    assert_refused(&deactivated, StatusCode::CONFLICT, None);
    let deleted = fixture.scim_delete(&owner, &token.secret).await;
    assert_refused(&deleted, StatusCode::CONFLICT, None);
    assert_eq!(
        fixture.holding(fixture.owner.id).await,
        Some((OrgRole::Owner.as_str().to_owned(), None, true))
    );

    let statuses: Vec<i16> = recorded(&fixture)
        .await
        .into_iter()
        .map(|(_, _, status, _)| status)
        .collect();
    assert_eq!(statuses, [409, 409, 409, 400, 400, 400, 400, 409, 409]);

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_existing_account_joins_as_itself_and_keeps_its_own_name() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let stranger = fixture.stranger().await;
    let (_, display_before, ..) = account(&fixture, stranger.id).await;

    let joined = fixture
        .scim_post(
            "/Users",
            &token.secret,
            &user(&stranger.email, "00u-stranger"),
        )
        .await;
    assert_scim(&joined, StatusCode::CREATED);
    assert_eq!(id(&joined), stranger.id);
    assert_eq!(
        provisioned_password(&fixture, stranger.id).await,
        Some(None)
    );

    let renamed = fixture
        .scim_patch(
            &format!("/Users/{}", stranger.id),
            &token.secret,
            &patch(json!([
                { "op": "replace", "path": "displayName", "value": "Rear Admiral" },
                { "op": "replace", "path": "emails[type eq \"work\"].value", "value": address() },
            ])),
        )
        .await;
    assert_scim(&renamed, StatusCode::OK);
    assert_eq!(renamed.json_value()["displayName"], json!("Rear Admiral"));
    let (email, display_after, ..) = account(&fixture, stranger.id).await;
    assert_eq!(
        email, stranger.email,
        "SCIM never rewrites another's account"
    );
    assert_eq!(display_after, display_before);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_provisioned_account_takes_its_email_and_name_from_scim() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let user_id = fixture.provision(&token.secret, &address()).await;
    let email = address();

    let replaced = fixture
        .scim_put(
            &format!("/Users/{user_id}"),
            &token.secret,
            &json!({
                "schemas": [USER_SCHEMA],
                "userName": email,
                "displayName": "Grace Brewster Hopper",
                "emails": [{ "value": email, "primary": true }],
            }),
        )
        .await;
    assert_scim(&replaced, StatusCode::OK);
    let body = replaced.json_value();
    assert_eq!(body["userName"], json!(email));
    assert_eq!(
        body["active"],
        json!(true),
        "an absent active changes nothing"
    );
    assert!(body.get("name").is_none(), "PUT replaces the name");
    let (stored, display_name, ..) = account(&fixture, user_id).await;
    assert_eq!(stored, email);
    assert_eq!(display_name.as_deref(), Some("Grace Brewster Hopper"));

    let taken = fixture.stranger().await;
    let clash = fixture
        .scim_patch(
            &format!("/Users/{user_id}"),
            &token.secret,
            &patch(
                json!([{ "op": "replace", "path": "emails", "value": [{ "value": taken.email }] }]),
            ),
        )
        .await;
    assert_refused(&clash, StatusCode::CONFLICT, Some("uniqueness"));
    assert_eq!(account(&fixture, user_id).await.0, email);

    let updates = fixture.audits(actions::SCIM_USER_UPDATED).await;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].actor_id, None);
    assert_eq!(
        updates[0].new_values.as_ref().unwrap()["display_name"],
        json!("Grace Brewster Hopper")
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn deactivating_then_reactivating_restores_workspaces_and_the_custom_role() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let person = fixture.stranger().await;
    fixture
        .scim_post("/Users", &token.secret, &user(&person.email, "00u-person"))
        .await
        .assert_status(StatusCode::CREATED);
    let workspace = fixture.workspace().await;
    workspace_members::add_member(
        fixture.pool(),
        workspace,
        person.id,
        WorkspaceRole::Admin,
        None,
    )
    .await
    .unwrap();
    let role = organization_role(&fixture, "Auditors").await;
    sqlx::query(
        "UPDATE organization_members SET role = 'member', custom_role_id = $3 \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(person.id)
    .bind(role)
    .execute(fixture.pool())
    .await
    .unwrap();
    fixture
        .inner
        .client
        .get_auth("/api/auth/sessions", &person.token)
        .await
        .assert_status(StatusCode::OK);

    let path = format!("/Users/{}", person.id);
    let deactivated = fixture
        .scim_patch(&path, &token.secret, &deactivation())
        .await;
    assert_scim(&deactivated, StatusCode::OK);
    assert_eq!(deactivated.json_value()["active"], json!(false));
    let (_, _, active) = fixture.holding(person.id).await.unwrap();
    assert!(!active);
    assert_eq!(
        seat(&fixture, workspace, person.id).await,
        Some((WorkspaceRole::Admin.as_str().to_owned(), false))
    );
    fixture
        .inner
        .client
        .get_auth("/api/auth/sessions", &person.token)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    let listed = fixture
        .scim_get(
            &format!("/Users?filter=userName%20eq%20%22{}%22", person.email),
            &token.secret,
        )
        .await
        .json_value();
    assert_eq!(listed["Resources"][0]["active"], json!(false));

    let again = fixture
        .scim_patch(&path, &token.secret, &deactivation())
        .await;
    assert_scim(&again, StatusCode::OK);
    assert_eq!(
        fixture.audits(actions::SCIM_USER_DEACTIVATED).await.len(),
        1
    );

    let reactivated = fixture
        .scim_patch(&path, &token.secret, &reactivation())
        .await;
    assert_scim(&reactivated, StatusCode::OK);
    assert_eq!(reactivated.json_value()["active"], json!(true));
    assert_eq!(
        fixture.holding(person.id).await,
        Some((OrgRole::Member.as_str().to_owned(), Some(role), true))
    );
    assert_eq!(
        seat(&fixture, workspace, person.id).await,
        Some((WorkspaceRole::Admin.as_str().to_owned(), true))
    );

    let deactivations = fixture.audits(actions::SCIM_USER_DEACTIVATED).await;
    assert_eq!(deactivations[0].actor_id, None);
    let reactivations = fixture.audits(actions::SCIM_USER_REACTIVATED).await;
    assert_eq!(reactivations.len(), 1);
    assert_eq!(reactivations[0].actor_id, None);
    assert_eq!(
        reactivations[0].new_values.as_ref().unwrap()["scim_token_id"],
        json!(token.id)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_deactivated_only_owner_of_a_workspace_is_succeeded_and_audited() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let leaving = fixture.provision(&token.secret, &address()).await;
    let staying = fixture.provision(&token.secret, &address()).await;
    let workspace = fixture.workspace().await;
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
        .bind(workspace)
        .execute(fixture.pool())
        .await
        .unwrap();
    for (user, role) in [
        (leaving, WorkspaceRole::Owner),
        (staying, WorkspaceRole::Member),
    ] {
        workspace_members::add_member(fixture.pool(), workspace, user, role, None)
            .await
            .unwrap();
    }

    fixture
        .scim_patch(&format!("/Users/{leaving}"), &token.secret, &deactivation())
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        seat(&fixture, workspace, staying).await,
        Some((WorkspaceRole::Owner.as_str().to_owned(), true))
    );
    let seated = fixture.audits(actions::WORKSPACE_OWNER_SEATED).await;
    assert_eq!(seated.len(), 1);
    assert_eq!(seated[0].actor_id, None);
    assert_eq!(seated[0].resource_id, Some(workspace));
    assert_eq!(
        seated[0].new_values.as_ref().unwrap()["user_id"],
        json!(staying)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_accepts_only_deprovisioning() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let kept = fixture.provision(&token.secret, &address()).await;
    let removed = fixture.provision(&token.secret, &address()).await;
    fixture.inner.store_license(&sso_fixture::lapsed()).await;

    let created = fixture
        .scim_post("/Users", &token.secret, &user(&address(), "late"))
        .await;
    assert_refused(&created, StatusCode::FORBIDDEN, None);
    assert_eq!(created.json_value()["detail"], json!(ENTERPRISE_MESSAGE));

    let path = format!("/Users/{kept}");
    let renamed = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{ "op": "replace", "path": "displayName", "value": "Renamed" }])),
        )
        .await;
    assert_refused(&renamed, StatusCode::FORBIDDEN, None);

    let read = fixture.scim_get(&path, &token.secret).await;
    assert_scim(&read, StatusCode::OK);

    let deactivated = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([
                { "op": "replace", "path": "displayName", "value": "Renamed" },
                { "op": "replace", "path": "active", "value": "False" },
            ])),
        )
        .await;
    assert_scim(&deactivated, StatusCode::OK);
    let body = deactivated.json_value();
    assert_eq!(body["active"], json!(false));
    assert_eq!(body["displayName"], json!("Ada Lovelace"));

    let reactivated = fixture
        .scim_patch(&path, &token.secret, &reactivation())
        .await;
    assert_refused(&reactivated, StatusCode::FORBIDDEN, None);

    fixture
        .scim_delete(&format!("/Users/{removed}"), &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.holding(removed).await, None);

    fixture.cleanup().await;
}

#[tokio::test]
async fn discovery_answers_with_a_token_only() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;

    let config = fixture
        .scim_get("/ServiceProviderConfig", &token.secret)
        .await;
    assert_scim(&config, StatusCode::OK);
    assert_eq!(config.json_value()["patch"]["supported"], json!(true));
    assert_eq!(config.json_value()["filter"]["maxResults"], json!(1000));

    let types = fixture.scim_get("/ResourceTypes", &token.secret).await;
    assert_scim(&types, StatusCode::OK);
    assert_eq!(types.json_value()["totalResults"], json!(2));
    let user_type = fixture.scim_get("/ResourceTypes/User", &token.secret).await;
    assert_scim(&user_type, StatusCode::OK);
    assert_eq!(user_type.json_value()["endpoint"], json!("/Users"));
    let missing = fixture
        .scim_get("/ResourceTypes/Device", &token.secret)
        .await;
    assert_refused(&missing, StatusCode::NOT_FOUND, None);

    let schemas = fixture.scim_get("/Schemas", &token.secret).await;
    assert_scim(&schemas, StatusCode::OK);
    let schema = fixture
        .scim_get(&format!("/Schemas/{USER_SCHEMA}"), &token.secret)
        .await;
    assert_scim(&schema, StatusCode::OK);
    assert_eq!(schema.json_value()["id"], json!(USER_SCHEMA));

    let anonymous = fixture.scim_get("/ServiceProviderConfig", "").await;
    assert_refused(&anonymous, StatusCode::UNAUTHORIZED, None);

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_organization_over_its_budget_is_told_when_to_retry() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let other = fixture.another_organization().await;
    let foreign = fixture.token_for(other).await;
    fixture.state().limit_scim(2, Duration::from_secs(60));

    for _ in 0..3 {
        let refused = fixture.scim_get("/Users", "zscim_wrong").await;
        assert_refused(&refused, StatusCode::UNAUTHORIZED, None);
    }
    for _ in 0..2 {
        assert_scim(
            &fixture.scim_get("/Users", &token.secret).await,
            StatusCode::OK,
        );
    }
    let throttled = fixture.scim_get("/Users", &token.secret).await;
    assert_refused(&throttled, StatusCode::TOO_MANY_REQUESTS, Some("tooMany"));
    let retry_after: u64 = throttled.header("retry-after").unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry_after), "{retry_after}");

    let elsewhere = fixture
        .scim(
            Method::GET,
            &format!("{}/Users", scim::base(other)),
            &foreign.secret,
            None,
        )
        .await;
    assert_scim(&elsewhere, StatusCode::OK);

    let errors = recorded(&fixture).await;
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].2, 429);
    assert_eq!(errors[0].3.as_deref(), Some("tooMany"));

    fixture.cleanup().await;
}
