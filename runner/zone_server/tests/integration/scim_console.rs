//! Setting up SCIM provisioning from the console: the settings an owner reads, issuing, rotating
//! and revoking tokens whose secret is shown once, and mapping pushed groups to organization
//! roles. Only owners reach any of it, and a lapsed license leaves only reading and revoking.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::LICENSE_REQUIRED;
use zone_server::auth::{Grant, OrganizationMember, Permission, Scope};
use zone_server::db::audit::{actions, resources};
use zone_server::db::custom_roles::{self, Assignment, Creation, Definition, Summary};
use zone_server::db::organization_members::OrgRole;
use zone_server::db::scim::{Link, NewError, errors, group_members, groups, tokens, users};
use zone_server::routes::scim::console::refusal::{
    GROUP_NOT_FOUND, MAPPING_INVALID, OWNER_UNMAPPABLE, ROLE_NOT_FOUND, TOKEN_NAME_INVALID,
    TOKEN_NOT_FOUND,
};
use zone_server::services::scim::{Remapping, console, secret};

use crate::common::TestResponse;
use crate::common::scim::Fixture;
use crate::common::sso::{Account, lapsed, team};

const PREFIX: &str = "scim_console";
const NO_STORE: &str = "no-store";
const BASE_URL: &str = "http://localhost:3000/api/scim/v2/organizations";

fn settings(fixture: &Fixture) -> String {
    format!("/api/organizations/{}/scim", fixture.organization)
}

fn tokens_path(fixture: &Fixture) -> String {
    format!("{}/tokens", settings(fixture))
}

fn token_path(fixture: &Fixture, token: impl std::fmt::Display) -> String {
    format!("{}/{token}", tokens_path(fixture))
}

fn rotations(fixture: &Fixture, token: impl std::fmt::Display) -> String {
    format!("{}/rotations", token_path(fixture, token))
}

fn mapping(fixture: &Fixture, group: impl std::fmt::Display) -> String {
    format!("{}/groups/{group}/mapping", settings(fixture))
}

fn refused(response: &TestResponse, status: StatusCode, code: &str) {
    response.assert_status(status);
    assert_eq!(
        response.json_value()["code"],
        json!(code),
        "{}",
        response.text()
    );
}

async fn read(fixture: &Fixture, token: &str) -> TestResponse {
    fixture
        .inner
        .client
        .get_auth(&settings(fixture), token)
        .await
}

async fn create(fixture: &Fixture, token: &str, body: &Value) -> TestResponse {
    fixture
        .inner
        .client
        .post_json_auth(&tokens_path(fixture), body, token)
        .await
}

async fn rotate(fixture: &Fixture, token: &str, id: impl std::fmt::Display) -> TestResponse {
    fixture
        .inner
        .client
        .post_json_auth(&rotations(fixture, id), &json!({}), token)
        .await
}

async fn revoke(fixture: &Fixture, token: &str, id: impl std::fmt::Display) -> TestResponse {
    fixture
        .inner
        .client
        .delete_auth(&token_path(fixture, id), token)
        .await
}

async fn map(
    fixture: &Fixture,
    token: &str,
    group: impl std::fmt::Display,
    body: &Value,
) -> TestResponse {
    fixture
        .inner
        .client
        .put_json_auth(&mapping(fixture, group), body, token)
        .await
}

async fn unmap(fixture: &Fixture, token: &str, group: impl std::fmt::Display) -> TestResponse {
    fixture
        .inner
        .client
        .delete_auth(&mapping(fixture, group), token)
        .await
}

/// Issues a token through the console and returns its ID and secret.
async fn issued(fixture: &Fixture, name: &str) -> (Uuid, String) {
    let response = create(fixture, &fixture.owner.token, &json!({ "name": name })).await;
    response.assert_status(StatusCode::CREATED);
    let body = response.json_value();
    (
        body["token"]["id"].as_str().unwrap().parse().unwrap(),
        body["secret"].as_str().unwrap().to_owned(),
    )
}

/// Whether `secret` is one an identity provider could authenticate with right now.
async fn authenticates(fixture: &Fixture, secret: &str) -> bool {
    tokens::active(fixture.pool(), fixture.organization)
        .await
        .unwrap()
        .iter()
        .any(|credential| secret::matches(&credential.token_hash, secret))
}

async fn stored_tokens(fixture: &Fixture) -> String {
    sqlx::query_scalar::<_, String>("SELECT row_to_json(t)::text FROM scim_tokens t")
        .fetch_all(fixture.pool())
        .await
        .unwrap()
        .join("\n")
}

/// A member SCIM has linked, so group mappings govern their role.
async fn linked(fixture: &Fixture, role: OrgRole) -> Account {
    let account = fixture.member(role).await;
    users::upsert(
        fixture.pool(),
        &Link::new(fixture.organization, account.id, account.email.clone()),
    )
    .await
    .unwrap();
    account
}

async fn group(fixture: &Fixture, organization: Uuid, name: &str, members: &[Uuid]) -> Uuid {
    let id = groups::create(fixture.pool(), organization, name, None)
        .await
        .unwrap()
        .id;
    let mut connection = fixture.pool().acquire().await.unwrap();
    group_members::add(&mut connection, organization, id, members)
        .await
        .unwrap();
    id
}

async fn custom_role(fixture: &Fixture, organization: Uuid, name: &str, scope: Scope) -> Summary {
    let permission = match scope {
        Scope::Workspace => Permission::WorkspaceWrite,
        _ => Permission::AuditLogsRead,
    };
    let grant = Grant::compose(scope, &[permission.name().to_owned()]).unwrap();
    let definition = Definition {
        name,
        description: None,
        scope,
        grant: &grant,
    };
    match custom_roles::create(fixture.pool(), organization, &definition, fixture.owner.id)
        .await
        .unwrap()
    {
        Creation::Created(role) => Summary {
            id: role.id,
            name: role.name,
        },
        Creation::NameTaken => panic!("{name} is already taken"),
    }
}

/// The member's stored role and custom role.
async fn held(fixture: &Fixture, user: Uuid) -> (String, Option<Uuid>) {
    sqlx::query_as(
        "SELECT role, custom_role_id FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(user)
    .fetch_one(fixture.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn an_owner_reads_the_settings_with_the_base_url_tokens_groups_and_errors() {
    let fixture = Fixture::new(PREFIX).await;
    let response = read(&fixture, &fixture.owner.token).await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.json_value(),
        json!({
            "entitled": true,
            "edition": "enterprise",
            "status": "active",
            "base_url": format!("{BASE_URL}/{}", fixture.organization),
            "last_synced_at": null,
            "tokens": [],
            "groups": [],
            "errors": [],
        })
    );

    let (token, _) = issued(&fixture, "Okta").await;
    tokens::touch(fixture.pool(), token).await.unwrap();
    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
    )
    .await;
    let engineering = group(&fixture, fixture.organization, "Engineering", &[]).await;
    let auditors = group(&fixture, fixture.organization, "Auditors", &[]).await;
    map(
        &fixture,
        &fixture.owner.token,
        auditors,
        &json!({ "custom_role_id": auditor.id }),
    )
    .await
    .assert_status(StatusCode::OK);
    errors::record(
        fixture.pool(),
        &NewError {
            organization_id: fixture.organization,
            token_id: Some(token),
            method: "PATCH",
            path: "/api/scim/v2/organizations/{org_id}/Users/{user_id}",
            status: 400,
            scim_type: Some("invalidValue"),
            detail: "active must be a boolean",
        },
    )
    .await
    .unwrap();
    let other = fixture.another_organization().await;
    group(&fixture, other, "Elsewhere", &[]).await;

    let body = read(&fixture, &fixture.owner.token).await.json_value();
    assert!(body["last_synced_at"].is_string(), "{body}");
    assert_eq!(body["tokens"].as_array().unwrap().len(), 1);
    assert_eq!(body["tokens"][0]["id"], json!(token));
    assert_eq!(body["tokens"][0]["name"], json!("Okta"));
    assert_eq!(body["tokens"][0]["created_by"], json!(fixture.owner.id));
    assert!(body["tokens"][0].get("secret").is_none(), "{body}");
    let groups = body["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "{body}");
    assert_eq!(groups[0]["id"], json!(engineering));
    assert_eq!(groups[0]["role"], Value::Null);
    assert_eq!(groups[0]["custom_role"], Value::Null);
    assert_eq!(groups[1]["id"], json!(auditors));
    assert_eq!(
        groups[1]["custom_role"],
        json!({ "id": auditor.id, "name": "Auditor" })
    );
    assert_eq!(body["errors"].as_array().unwrap().len(), 1);
    assert_eq!(body["errors"][0]["status"], json!(400));
    assert_eq!(body["errors"][0]["method"], json!("PATCH"));
    assert_eq!(body["errors"][0]["scim_type"], json!("invalidValue"));
    assert_eq!(
        body["errors"][0]["detail"],
        json!("active must be a boolean")
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn only_owners_reach_scim_provisioning() {
    let fixture = Fixture::new(PREFIX).await;
    let admin = fixture.member(OrgRole::Admin).await;
    let member = fixture.member(OrgRole::Member).await;
    let stranger = fixture.stranger().await;
    let (token, secret) = issued(&fixture, "Okta").await;
    let engineering = group(&fixture, fixture.organization, "Engineering", &[]).await;

    for (who, account) in [
        ("admin", &admin),
        ("member", &member),
        ("stranger", &stranger),
    ] {
        let token_body = json!({ "name": "Entra" });
        let mapping_body = json!({ "role": "admin" });
        for response in [
            read(&fixture, &account.token).await,
            create(&fixture, &account.token, &token_body).await,
            rotate(&fixture, &account.token, token).await,
            revoke(&fixture, &account.token, token).await,
            map(&fixture, &account.token, engineering, &mapping_body).await,
            unmap(&fixture, &account.token, engineering).await,
        ] {
            assert_eq!(
                response.status,
                StatusCode::FORBIDDEN,
                "{who}: {}",
                response.text()
            );
        }
    }

    assert!(authenticates(&fixture, &secret).await);
    assert_eq!(
        tokens::list(fixture.pool(), fixture.organization)
            .await
            .unwrap()
            .len(),
        1
    );
    let unchanged = groups::get(fixture.pool(), fixture.organization, engineering)
        .await
        .unwrap()
        .unwrap();
    assert!(!unchanged.mapping.is_mapped());
    read(&fixture, &fixture.owner.token)
        .await
        .assert_status(StatusCode::OK);
    fixture
        .inner
        .client
        .get(&settings(&fixture))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_token_secret_is_answered_once_and_never_stored_logged_or_audited() {
    let fixture = Fixture::new(PREFIX).await;
    let response = create(
        &fixture,
        &fixture.owner.token,
        &json!({ "name": "  Okta  " }),
    )
    .await;
    response.assert_status(StatusCode::CREATED);
    assert_eq!(response.header("cache-control"), Some(NO_STORE));
    let body = response.json_value();
    let secret = body["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with(secret::PREFIX), "{secret}");
    let id: Uuid = body["token"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(body["token"]["name"], json!("Okta"));
    assert_eq!(body["token"]["hint"], json!(secret::hint(&secret)));
    assert_eq!(body["token"]["created_by"], json!(fixture.owner.id));
    assert_eq!(body["token"]["rotated_at"], Value::Null);
    assert_eq!(body["token"]["last_used_at"], Value::Null);
    assert!(authenticates(&fixture, &secret).await);

    assert!(!stored_tokens(&fixture).await.contains(&secret));
    assert!(!fixture.inner.audit_text().await.contains(&secret));
    assert!(
        !read(&fixture, &fixture.owner.token)
            .await
            .text()
            .contains(&secret)
    );
    let audits = fixture.audits(actions::SCIM_TOKEN_CREATED).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor_id, Some(fixture.owner.id));
    assert_eq!(audits[0].organization_id, Some(fixture.organization));
    assert_eq!(audits[0].resource_type, resources::SCIM_TOKEN);
    assert_eq!(audits[0].resource_id, Some(id));
    assert_eq!(audits[0].old_values, None);
    assert_eq!(audits[0].new_values, Some(json!({ "name": "Okta" })));
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_token_needs_a_name() {
    let fixture = Fixture::new(PREFIX).await;
    for body in [
        json!({ "name": "   " }),
        json!({ "name": "" }),
        json!({}),
        json!({ "name": 7 }),
        json!({ "name": "Okta", "secret": "zscim_mine" }),
    ] {
        refused(
            &create(&fixture, &fixture.owner.token, &body).await,
            StatusCode::BAD_REQUEST,
            TOKEN_NAME_INVALID,
        );
    }
    assert!(stored_tokens(&fixture).await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn rotating_a_token_replaces_its_secret_at_once() {
    let fixture = Fixture::new(PREFIX).await;
    let (id, first) = issued(&fixture, "Okta").await;

    let response = rotate(&fixture, &fixture.owner.token, id).await;
    response.assert_status(StatusCode::CREATED);
    assert_eq!(response.header("cache-control"), Some(NO_STORE));
    let body = response.json_value();
    let second = body["secret"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    assert_eq!(body["token"]["id"], json!(id));
    assert_eq!(body["token"]["name"], json!("Okta"));
    assert_eq!(body["token"]["hint"], json!(secret::hint(&second)));
    assert!(body["token"]["rotated_at"].is_string(), "{body}");
    assert!(!authenticates(&fixture, &first).await);
    assert!(authenticates(&fixture, &second).await);

    let stored = stored_tokens(&fixture).await;
    let audited = fixture.inner.audit_text().await;
    for secret in [&first, &second] {
        assert!(!stored.contains(secret.as_str()));
        assert!(!audited.contains(secret.as_str()));
    }
    let audits = fixture.audits(actions::SCIM_TOKEN_ROTATED).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor_id, Some(fixture.owner.id));
    assert_eq!(audits[0].resource_id, Some(id));
    assert_eq!(audits[0].new_values, Some(json!({ "name": "Okta" })));
    fixture.cleanup().await;
}

#[tokio::test]
async fn revoking_a_token_ends_it() {
    let fixture = Fixture::new(PREFIX).await;
    let (id, secret) = issued(&fixture, "Okta").await;
    let (kept, kept_secret) = issued(&fixture, "Entra").await;

    revoke(&fixture, &fixture.owner.token, id)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    assert!(!authenticates(&fixture, &secret).await);
    assert!(authenticates(&fixture, &kept_secret).await);
    let body = read(&fixture, &fixture.owner.token).await.json_value();
    assert_eq!(body["tokens"].as_array().unwrap().len(), 1);
    assert_eq!(body["tokens"][0]["id"], json!(kept));
    let audits = fixture.audits(actions::SCIM_TOKEN_REVOKED).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor_id, Some(fixture.owner.id));
    assert_eq!(audits[0].resource_type, resources::SCIM_TOKEN);
    assert_eq!(audits[0].resource_id, Some(id));
    assert_eq!(audits[0].old_values, Some(json!({ "name": "Okta" })));
    assert_eq!(audits[0].new_values, None);

    refused(
        &revoke(&fixture, &fixture.owner.token, id).await,
        StatusCode::NOT_FOUND,
        TOKEN_NOT_FOUND,
    );
    refused(
        &rotate(&fixture, &fixture.owner.token, id).await,
        StatusCode::NOT_FOUND,
        TOKEN_NOT_FOUND,
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_token_of_another_organization_or_none_at_all_is_not_found() {
    let fixture = Fixture::new(PREFIX).await;
    let other = fixture.another_organization().await;
    let foreign = fixture.token_for(other).await;

    for id in [
        foreign.id.to_string(),
        Uuid::new_v4().to_string(),
        "not-a-token".to_owned(),
    ] {
        refused(
            &rotate(&fixture, &fixture.owner.token, &id).await,
            StatusCode::NOT_FOUND,
            TOKEN_NOT_FOUND,
        );
        refused(
            &revoke(&fixture, &fixture.owner.token, &id).await,
            StatusCode::NOT_FOUND,
            TOKEN_NOT_FOUND,
        );
    }

    let survivors = tokens::active(fixture.pool(), other).await.unwrap();
    assert_eq!(survivors.len(), 1);
    assert!(secret::matches(&survivors[0].token_hash, &foreign.secret));
    assert!(fixture.audits(actions::SCIM_TOKEN_ROTATED).await.is_empty());
    assert!(fixture.audits(actions::SCIM_TOKEN_REVOKED).await.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn without_an_active_enterprise_license_only_reading_and_revoking_work() {
    let fixture = Fixture::new(PREFIX).await;
    let engineering = group(&fixture, fixture.organization, "Engineering", &[]).await;

    for (license, edition, status) in [
        (Some(lapsed()), "enterprise", "lapsed"),
        (Some(team()), "team", "active"),
        (None, "community", "community"),
    ] {
        let (id, secret) = issued(&fixture, edition).await;
        match &license {
            Some(key) => fixture.inner.store_license(key).await,
            None => fixture.inner.remove_license(&fixture.owner).await,
        }

        let response = read(&fixture, &fixture.owner.token).await;
        response.assert_status(StatusCode::OK);
        let body = response.json_value();
        assert_eq!(body["entitled"], json!(false), "{edition} {status}");
        assert_eq!(body["edition"], json!(edition), "{edition} {status}");
        assert_eq!(body["status"], json!(status), "{edition} {status}");
        for response in [
            create(&fixture, &fixture.owner.token, &json!({ "name": "Entra" })).await,
            rotate(&fixture, &fixture.owner.token, id).await,
            map(
                &fixture,
                &fixture.owner.token,
                engineering,
                &json!({ "role": "admin" }),
            )
            .await,
            unmap(&fixture, &fixture.owner.token, engineering).await,
        ] {
            refused(&response, StatusCode::FORBIDDEN, LICENSE_REQUIRED);
        }
        assert!(authenticates(&fixture, &secret).await, "{edition} {status}");

        revoke(&fixture, &fixture.owner.token, id)
            .await
            .assert_status(StatusCode::NO_CONTENT);
        assert!(
            !authenticates(&fixture, &secret).await,
            "{edition} {status}"
        );
        fixture.inner.license(&fixture.owner).await;
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn mapping_a_group_sets_its_linked_members_roles_and_unmapping_the_last_drops_them_to_member()
{
    let fixture = Fixture::new(PREFIX).await;
    let first = linked(&fixture, OrgRole::Member).await;
    let second = linked(&fixture, OrgRole::Member).await;
    let unlinked = fixture.member(OrgRole::Member).await;
    let engineering = group(
        &fixture,
        fixture.organization,
        "Engineering",
        &[first.id, second.id, unlinked.id, fixture.owner.id],
    )
    .await;

    let response = map(
        &fixture,
        &fixture.owner.token,
        engineering,
        &json!({ "role": "admin" }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["id"], json!(engineering));
    assert_eq!(body["display_name"], json!("Engineering"));
    assert_eq!(body["external_id"], Value::Null);
    assert_eq!(body["members"], json!(4));
    assert_eq!(body["role"], json!("admin"));
    assert_eq!(body["custom_role"], Value::Null);
    assert!(body["updated_at"].is_string(), "{body}");
    for user in [first.id, second.id] {
        assert_eq!(held(&fixture, user).await, ("admin".to_owned(), None));
    }
    assert_eq!(
        held(&fixture, unlinked.id).await,
        ("member".to_owned(), None)
    );
    assert_eq!(
        held(&fixture, fixture.owner.id).await,
        ("owner".to_owned(), None)
    );
    let audits = fixture.audits(actions::SCIM_GROUP_MAPPED).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor_id, Some(fixture.owner.id));
    assert_eq!(audits[0].resource_type, resources::SCIM_GROUP);
    assert_eq!(audits[0].resource_id, Some(engineering));
    assert_eq!(
        audits[0].old_values,
        Some(json!({ "role": null, "custom_role": null }))
    );
    let mut changed = vec![
        json!({ "user_id": first.id, "role": "admin", "custom_role": null }),
        json!({ "user_id": second.id, "role": "admin", "custom_role": null }),
    ];
    changed.sort_by_key(|change| change["user_id"].as_str().unwrap().to_owned());
    assert_eq!(
        audits[0].new_values,
        Some(json!({ "role": "admin", "custom_role": null, "changed": changed }))
    );

    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
    )
    .await;
    let response = map(
        &fixture,
        &fixture.owner.token,
        engineering,
        &json!({ "custom_role_id": auditor.id }),
    )
    .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["role"], Value::Null);
    assert_eq!(
        response.json_value()["custom_role"],
        json!({ "id": auditor.id, "name": "Auditor" })
    );
    for user in [first.id, second.id] {
        assert_eq!(
            held(&fixture, user).await,
            ("member".to_owned(), Some(auditor.id))
        );
    }

    let response = unmap(&fixture, &fixture.owner.token, engineering).await;
    response.assert_status(StatusCode::OK);
    assert_eq!(response.json_value()["role"], Value::Null);
    assert_eq!(response.json_value()["custom_role"], Value::Null);
    for user in [first.id, second.id] {
        assert_eq!(held(&fixture, user).await, ("member".to_owned(), None));
    }
    assert_eq!(
        held(&fixture, fixture.owner.id).await,
        ("owner".to_owned(), None)
    );
    let audits = fixture.audits(actions::SCIM_GROUP_UNMAPPED).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].old_values,
        Some(json!({ "role": null, "custom_role": { "id": auditor.id, "name": "Auditor" } }))
    );
    assert_eq!(audits[0].new_values.as_ref().unwrap()["role"], Value::Null);
    assert_eq!(
        audits[0].new_values.as_ref().unwrap()["changed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fixture.audits(actions::SCIM_GROUP_MAPPED).await.len(), 2);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_member_keeps_the_role_of_another_mapped_group_when_one_is_unmapped() {
    let fixture = Fixture::new(PREFIX).await;
    let both = linked(&fixture, OrgRole::Member).await;
    let only = linked(&fixture, OrgRole::Member).await;
    let engineering = group(
        &fixture,
        fixture.organization,
        "Engineering",
        &[both.id, only.id],
    )
    .await;
    let operations = group(&fixture, fixture.organization, "Operations", &[both.id]).await;
    for group in [engineering, operations] {
        map(
            &fixture,
            &fixture.owner.token,
            group,
            &json!({ "role": "admin" }),
        )
        .await
        .assert_status(StatusCode::OK);
    }
    assert_eq!(held(&fixture, both.id).await, ("admin".to_owned(), None));
    assert_eq!(held(&fixture, only.id).await, ("admin".to_owned(), None));

    unmap(&fixture, &fixture.owner.token, engineering)
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(held(&fixture, both.id).await, ("admin".to_owned(), None));
    assert_eq!(held(&fixture, only.id).await, ("member".to_owned(), None));
    fixture.cleanup().await;
}

#[tokio::test]
async fn saving_the_same_mapping_recomputes_roles_and_audits_only_what_changed() {
    let fixture = Fixture::new(PREFIX).await;
    let drifted = linked(&fixture, OrgRole::Member).await;
    let engineering = group(&fixture, fixture.organization, "Engineering", &[drifted.id]).await;
    let body = json!({ "role": "admin" });
    map(&fixture, &fixture.owner.token, engineering, &body)
        .await
        .assert_status(StatusCode::OK);
    map(&fixture, &fixture.owner.token, engineering, &body)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(fixture.audits(actions::SCIM_GROUP_MAPPED).await.len(), 1);

    sqlx::query(
        "UPDATE organization_members SET role = 'member' \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(drifted.id)
    .execute(fixture.pool())
    .await
    .unwrap();
    map(&fixture, &fixture.owner.token, engineering, &body)
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(held(&fixture, drifted.id).await, ("admin".to_owned(), None));
    let audits = fixture.audits(actions::SCIM_GROUP_MAPPED).await;
    assert_eq!(audits.len(), 2);
    assert_eq!(
        audits[1].old_values,
        Some(json!({ "role": "admin", "custom_role": null }))
    );
    assert_eq!(
        audits[1].new_values.as_ref().unwrap()["changed"],
        json!([{ "user_id": drifted.id, "role": "admin", "custom_role": null }])
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_mapping_is_refused_with_the_code_of_what_is_wrong() {
    let fixture = Fixture::new(PREFIX).await;
    let member = linked(&fixture, OrgRole::Member).await;
    let engineering = group(&fixture, fixture.organization, "Engineering", &[member.id]).await;
    let other = fixture.another_organization().await;
    let foreign_group = group(&fixture, other, "Elsewhere", &[]).await;
    let foreign_role = custom_role(&fixture, other, "Foreign", Scope::Organization).await;
    let workspace_role =
        custom_role(&fixture, fixture.organization, "Builder", Scope::Workspace).await;
    let owner = &fixture.owner.token;

    refused(
        &map(&fixture, owner, engineering, &json!({ "role": "owner" })).await,
        StatusCode::BAD_REQUEST,
        OWNER_UNMAPPABLE,
    );
    for body in [
        json!({}),
        json!({ "role": null, "custom_role_id": null }),
        json!({ "role": "admin", "custom_role_id": workspace_role.id }),
        json!({ "role": "superuser" }),
        json!({ "role": "admin", "members": [] }),
        json!({ "custom_role_id": "not-a-role" }),
    ] {
        refused(
            &map(&fixture, owner, engineering, &body).await,
            StatusCode::BAD_REQUEST,
            MAPPING_INVALID,
        );
    }
    for role in [foreign_role.id, workspace_role.id, Uuid::new_v4()] {
        refused(
            &map(
                &fixture,
                owner,
                engineering,
                &json!({ "custom_role_id": role }),
            )
            .await,
            StatusCode::NOT_FOUND,
            ROLE_NOT_FOUND,
        );
    }
    for group in [
        foreign_group.to_string(),
        Uuid::new_v4().to_string(),
        "not-a-group".to_owned(),
    ] {
        refused(
            &map(&fixture, owner, &group, &json!({ "role": "admin" })).await,
            StatusCode::NOT_FOUND,
            GROUP_NOT_FOUND,
        );
        refused(
            &unmap(&fixture, owner, &group).await,
            StatusCode::NOT_FOUND,
            GROUP_NOT_FOUND,
        );
    }

    let untouched = groups::get(fixture.pool(), fixture.organization, engineering)
        .await
        .unwrap()
        .unwrap();
    assert!(!untouched.mapping.is_mapped());
    let foreign = groups::get(fixture.pool(), other, foreign_group)
        .await
        .unwrap()
        .unwrap();
    assert!(!foreign.mapping.is_mapped());
    assert_eq!(held(&fixture, member.id).await, ("member".to_owned(), None));
    assert!(fixture.audits(actions::SCIM_GROUP_MAPPED).await.is_empty());
    assert!(
        fixture
            .audits(actions::SCIM_GROUP_UNMAPPED)
            .await
            .is_empty()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_mapping_never_grants_more_than_the_editor_holds() {
    let fixture = Fixture::new(PREFIX).await;
    let member = linked(&fixture, OrgRole::Member).await;
    let engineering = group(&fixture, fixture.organization, "Engineering", &[member.id]).await;
    let auditor = custom_role(
        &fixture,
        fixture.organization,
        "Auditor",
        Scope::Organization,
    )
    .await;
    let editor = OrganizationMember {
        organization_id: fixture.organization,
        user_id: member.id,
        email: member.email.clone(),
        role: OrgRole::Member,
        custom_role: None,
        grant: Grant::organization(OrgRole::Member),
    };

    for assignment in [
        Assignment::BuiltIn(OrgRole::Admin),
        Assignment::Custom(auditor.id),
    ] {
        assert_eq!(
            console::map(
                fixture.pool(),
                fixture.organization,
                engineering,
                assignment,
                &editor
            )
            .await
            .unwrap(),
            Remapping::Exceeds,
            "{assignment:?}"
        );
    }
    assert_eq!(
        console::map(
            fixture.pool(),
            fixture.organization,
            engineering,
            Assignment::BuiltIn(OrgRole::Owner),
            &editor
        )
        .await
        .unwrap(),
        Remapping::OwnerUnmappable
    );
    let untouched = groups::get(fixture.pool(), fixture.organization, engineering)
        .await
        .unwrap()
        .unwrap();
    assert!(!untouched.mapping.is_mapped());
    assert_eq!(held(&fixture, member.id).await, ("member".to_owned(), None));
    fixture.cleanup().await;
}
