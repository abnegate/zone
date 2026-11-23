use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::entitlement::ENTERPRISE_MESSAGE;
use zone_server::db::audit::actions;
use zone_server::db::organization_members::OrgRole;
use zone_server::db::scim::{Mapping, groups};

use crate::common::TestResponse;
use crate::common::scim::{CONTENT_TYPE, ERROR_SCHEMA, Fixture, GROUP_SCHEMA, PATCH_SCHEMA};
use crate::common::sso;

const PREFIX: &str = "scim_groups";

fn address() -> String {
    format!("{}@acme.test", Uuid::new_v4())
}

fn group(display_name: &str, external_id: &str, members: &[Uuid]) -> Value {
    json!({
        "schemas": [GROUP_SCHEMA],
        "displayName": display_name,
        "externalId": external_id,
        "members": members.iter().map(|member| json!({ "value": member })).collect::<Vec<_>>(),
    })
}

fn patch(operations: Value) -> Value {
    json!({ "schemas": [PATCH_SCHEMA], "Operations": operations })
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

fn members(body: &Value) -> Vec<Uuid> {
    let mut members: Vec<Uuid> = body["members"]
        .as_array()
        .expect("the group lists its members")
        .iter()
        .map(|member| member["value"].as_str().unwrap().parse().unwrap())
        .collect();
    members.sort();
    members
}

fn both(first: Uuid, second: Uuid) -> Vec<Uuid> {
    let mut both = vec![first, second];
    both.sort();
    both
}

async fn create(fixture: &Fixture, token: &str, display_name: &str, users: &[Uuid]) -> Uuid {
    let response = fixture
        .scim_post(
            "/Groups",
            token,
            &group(display_name, &Uuid::new_v4().to_string(), users),
        )
        .await;
    response.assert_status(StatusCode::CREATED);
    id(&response)
}

async fn roster(fixture: &Fixture, token: &str, group: Uuid) -> Vec<Uuid> {
    let response = fixture.scim_get(&format!("/Groups/{group}"), token).await;
    assert_scim(&response, StatusCode::OK);
    members(&response.json_value())
}

#[tokio::test]
async fn a_group_is_created_read_filtered_paged_and_deleted() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ada = fixture.provision(&token.secret, &address()).await;
    let grace = fixture.provision(&token.secret, &address()).await;

    let created = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &group("Engineering", "eng", &[ada]),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let group_id = id(&created);
    let body = created.json_value();
    let location = format!("http://localhost:3000{}/Groups/{group_id}", fixture.base());
    assert_eq!(created.header("location"), Some(location.as_str()));
    assert_eq!(body["schemas"], json!([GROUP_SCHEMA]));
    assert_eq!(body["displayName"], json!("Engineering"));
    assert_eq!(body["externalId"], json!("eng"));
    assert_eq!(body["meta"]["resourceType"], json!("Group"));
    assert_eq!(members(&body), [ada]);
    assert_eq!(body["members"][0]["display"], json!("Ada Lovelace"));
    assert_eq!(
        body["members"][0]["$ref"],
        json!(format!(
            "http://localhost:3000{}/Users/{ada}",
            fixture.base()
        ))
    );

    let read = fixture
        .scim_get(&format!("/Groups/{group_id}"), &token.secret)
        .await;
    assert_scim(&read, StatusCode::OK);
    assert_eq!(read.json_value(), body);
    let slim = fixture
        .scim_get(
            &format!("/Groups/{group_id}?excludedAttributes=members"),
            &token.secret,
        )
        .await
        .json_value();
    assert!(slim.get("members").is_none(), "{slim}");
    assert_eq!(slim["displayName"], json!("Engineering"));

    let user = fixture
        .scim_get(&format!("/Users/{ada}"), &token.secret)
        .await
        .json_value();
    assert_eq!(user["groups"][0]["value"], json!(group_id));
    assert_eq!(user["groups"][0]["display"], json!("Engineering"));

    let second = create(&fixture, &token.secret, "Design", &[grace]).await;

    let named = fixture
        .scim_get(
            "/Groups?excludedAttributes=members&filter=displayName%20eq%20%22engineering%22",
            &token.secret,
        )
        .await;
    assert_scim(&named, StatusCode::OK);
    let named = named.json_value();
    assert_eq!(named["totalResults"], json!(1));
    assert_eq!(named["Resources"][0]["id"], json!(group_id));
    assert!(named["Resources"][0].get("members").is_none());

    for (user_id, expected) in [(ada, 1), (grace, 0)] {
        let membership = fixture
            .scim_get(
                &format!(
                    "/Groups?filter=id%20eq%20%22{group_id}%22%20and%20members%20eq%20%22{user_id}%22&attributes=id"
                ),
                &token.secret,
            )
            .await
            .json_value();
        assert_eq!(membership["totalResults"], json!(expected), "{user_id}");
        if expected == 1 {
            assert!(membership["Resources"][0].get("members").is_none());
        }
    }

    let everything = fixture
        .scim_get("/Groups", &token.secret)
        .await
        .json_value();
    assert_eq!(everything["totalResults"], json!(2));
    assert_eq!(members(&everything["Resources"][0]), [ada]);
    assert_eq!(members(&everything["Resources"][1]), [grace]);
    let page = fixture
        .scim_get("/Groups?startIndex=2&count=1", &token.secret)
        .await
        .json_value();
    assert_eq!(page["totalResults"], json!(2));
    assert_eq!(page["itemsPerPage"], json!(1));
    assert_eq!(page["Resources"][0]["id"], json!(second));

    let unsupported = fixture
        .scim_get("/Groups?filter=displayName%20sw%20%22E%22", &token.secret)
        .await;
    assert_refused(&unsupported, StatusCode::BAD_REQUEST, Some("invalidFilter"));

    fixture
        .scim_delete(&format!("/Groups/{group_id}"), &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let gone = fixture
        .scim_get(&format!("/Groups/{group_id}"), &token.secret)
        .await;
    assert_refused(&gone, StatusCode::NOT_FOUND, None);
    let again = fixture
        .scim_delete(&format!("/Groups/{group_id}"), &token.secret)
        .await;
    assert_refused(&again, StatusCode::NOT_FOUND, None);

    for action in [actions::SCIM_GROUP_CREATED, actions::SCIM_GROUP_DELETED] {
        let audits = fixture.audits(action).await;
        assert!(!audits.is_empty(), "{action}");
        for audit in audits {
            assert_eq!(audit.actor_id, None);
            assert_eq!(audit.organization_id, Some(fixture.organization));
            assert_eq!(audit.resource_type, "scim_group");
            assert_eq!(
                audit.new_values.as_ref().unwrap()["scim_token_id"],
                json!(token.id)
            );
        }
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn members_change_the_ways_okta_and_entra_send_them() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ada = fixture.provision(&token.secret, &address()).await;
    let grace = fixture.provision(&token.secret, &address()).await;
    let group_id = create(&fixture, &token.secret, "Engineering", &[ada]).await;
    let path = format!("/Groups/{group_id}");

    let added = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{ "op": "add", "path": "members", "value": [{ "value": grace }] }])),
        )
        .await;
    assert_scim(&added, StatusCode::OK);
    assert_eq!(members(&added.json_value()), both(ada, grace));

    let filtered = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{ "op": "Remove", "path": format!("members[value eq \"{ada}\"]") }])),
        )
        .await;
    assert_scim(&filtered, StatusCode::OK);
    assert_eq!(members(&filtered.json_value()), [grace]);

    let listed = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{
                "op": "remove",
                "path": "members",
                "value": [{ "value": grace }, { "value": Uuid::new_v4() }, { "value": "jyoung" }],
            }])),
        )
        .await;
    assert_scim(&listed, StatusCode::OK);
    assert!(members(&listed.json_value()).is_empty());

    let replaced = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([
                { "op": "replace", "path": "members", "value": [{ "value": ada }, { "value": grace }] },
                { "op": "replace", "value": { "displayName": "Platform", "externalId": "platform" } },
            ])),
        )
        .await;
    assert_scim(&replaced, StatusCode::OK);
    let body = replaced.json_value();
    assert_eq!(members(&body), both(ada, grace));
    assert_eq!(body["displayName"], json!("Platform"));
    assert_eq!(body["externalId"], json!("platform"));

    let renamed = fixture
        .scim_put(
            &path,
            &token.secret,
            &json!({ "schemas": [GROUP_SCHEMA], "displayName": "Platform Engineering" }),
        )
        .await;
    assert_scim(&renamed, StatusCode::OK);
    let body = renamed.json_value();
    assert_eq!(body["displayName"], json!("Platform Engineering"));
    assert!(body.get("externalId").is_none(), "PUT replaces externalId");
    assert_eq!(
        members(&body),
        both(ada, grace),
        "an absent members changes nothing"
    );

    let emptied = fixture
        .scim_put(
            &path,
            &token.secret,
            &group("Platform Engineering", "pe", &[]),
        )
        .await;
    assert_scim(&emptied, StatusCode::OK);
    assert!(members(&emptied.json_value()).is_empty());

    let cleared = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([
                { "op": "add", "path": "members", "value": [{ "value": ada }] },
                { "op": "remove", "path": "members" },
            ])),
        )
        .await;
    assert_scim(&cleared, StatusCode::OK);
    assert!(members(&cleared.json_value()).is_empty());

    let updates = fixture.audits(actions::SCIM_GROUP_UPDATED).await;
    assert_eq!(updates.len(), 7);
    assert!(updates.iter().all(|audit| audit.actor_id.is_none()));
    assert_eq!(
        updates[0].new_values.as_ref().unwrap()["members"],
        json!([grace])
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn strangers_duplicates_and_other_organizations_are_refused() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ada = fixture.provision(&token.secret, &address()).await;
    let stranger = fixture.stranger().await;
    let group_id = create(&fixture, &token.secret, "Engineering", &[ada]).await;
    let path = format!("/Groups/{group_id}");

    for value in [json!(stranger.id), json!("jyoung")] {
        let refused = fixture
            .scim_patch(
                &path,
                &token.secret,
                &patch(json!([{ "op": "add", "path": "members", "value": [{ "value": value }] }])),
            )
            .await;
        assert_refused(&refused, StatusCode::BAD_REQUEST, Some("invalidValue"));
    }
    let created = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &group("Strangers", "strangers", &[stranger.id]),
        )
        .await;
    assert_refused(&created, StatusCode::BAD_REQUEST, Some("invalidValue"));
    assert_eq!(roster(&fixture, &token.secret, group_id).await, [ada]);
    let listed = fixture
        .scim_get("/Groups", &token.secret)
        .await
        .json_value();
    assert_eq!(
        listed["totalResults"],
        json!(1),
        "a refused group is never created"
    );

    let named = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &group(" engineering ", "other", &[]),
        )
        .await;
    assert_refused(&named, StatusCode::CONFLICT, Some("uniqueness"));
    let other = create(&fixture, &token.secret, "Design", &[]).await;
    let renamed = fixture
        .scim_patch(
            &format!("/Groups/{other}"),
            &token.secret,
            &patch(json!([{ "op": "replace", "path": "displayName", "value": "Engineering" }])),
        )
        .await;
    assert_refused(&renamed, StatusCode::CONFLICT, Some("uniqueness"));
    let blank = fixture
        .scim_post("/Groups", &token.secret, &group("  ", "blank", &[]))
        .await;
    assert_refused(&blank, StatusCode::BAD_REQUEST, Some("invalidValue"));

    let elsewhere = fixture.another_organization().await;
    let foreign_token = fixture.token_for(elsewhere).await;
    for response in [
        fixture.scim_get(&path, &foreign_token.secret).await,
        fixture
            .scim_patch(
                &path,
                &foreign_token.secret,
                &patch(json!([{ "op": "remove", "path": "members" }])),
            )
            .await,
        fixture.scim_delete(&path, &foreign_token.secret).await,
    ] {
        assert_refused(&response, StatusCode::UNAUTHORIZED, None);
    }
    let foreign_base = crate::common::scim::base(elsewhere);
    let foreign = fixture
        .scim(
            axum::http::Method::GET,
            &format!("{foreign_base}/Groups/{group_id}"),
            &foreign_token.secret,
            None,
        )
        .await;
    assert_refused(&foreign, StatusCode::NOT_FOUND, None);
    let none = fixture
        .scim(
            axum::http::Method::GET,
            &format!("{foreign_base}/Groups"),
            &foreign_token.secret,
            None,
        )
        .await;
    assert_eq!(none.json_value()["totalResults"], json!(0));
    assert_eq!(roster(&fixture, &token.secret, group_id).await, [ada]);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_mapped_group_settles_its_members_roles() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ada = fixture.provision(&token.secret, &address()).await;
    let group_id = create(&fixture, &token.secret, "Administrators", &[]).await;
    groups::map(
        fixture.pool(),
        fixture.organization,
        group_id,
        &Mapping::BuiltIn(OrgRole::Admin),
        fixture.owner.id,
    )
    .await
    .unwrap()
    .unwrap();
    let path = format!("/Groups/{group_id}");
    let role = |holding: Option<(String, Option<Uuid>, bool)>| holding.unwrap().0;

    fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{ "op": "add", "path": "members", "value": [{ "value": ada }] }])),
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(role(fixture.holding(ada).await), OrgRole::Admin.as_str());
    let updates = fixture.audits(actions::SCIM_GROUP_UPDATED).await;
    assert_eq!(
        updates[0].new_values.as_ref().unwrap()["roles"],
        json!([{ "user_id": ada, "role": "admin", "custom_role": null }])
    );

    fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{ "op": "remove", "path": format!("members[value eq \"{ada}\"]") }])),
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(role(fixture.holding(ada).await), OrgRole::Member.as_str());

    fixture
        .scim_put(
            &path,
            &token.secret,
            &group("Administrators", "admins", &[ada]),
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(role(fixture.holding(ada).await), OrgRole::Admin.as_str());

    let owner_added = fixture
        .scim_patch(
            &path,
            &token.secret,
            &patch(json!([{ "op": "add", "path": "members", "value": [{ "value": fixture.owner.id }] }])),
        )
        .await;
    assert_scim(&owner_added, StatusCode::OK);
    assert_eq!(
        role(fixture.holding(fixture.owner.id).await),
        OrgRole::Owner.as_str(),
        "SCIM never re-roles an owner"
    );

    fixture
        .scim_delete(&path, &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(role(fixture.holding(ada).await), OrgRole::Member.as_str());
    let deleted = fixture.audits(actions::SCIM_GROUP_DELETED).await;
    assert_eq!(
        deleted[0].new_values.as_ref().unwrap()["roles"],
        json!([{ "user_id": ada, "role": "member", "custom_role": null }])
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_retried_group_removal_still_drops_the_role() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ada = fixture.provision(&token.secret, &address()).await;
    let group_id = create(&fixture, &token.secret, "Administrators", &[ada]).await;
    fixture
        .map(group_id, &json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        fixture.holding(ada).await.unwrap().0,
        OrgRole::Admin.as_str()
    );

    sqlx::query("DELETE FROM scim_group_members WHERE group_id = $1 AND user_id = $2")
        .bind(group_id)
        .bind(ada)
        .execute(fixture.pool())
        .await
        .unwrap();
    assert_eq!(
        fixture.holding(ada).await.unwrap().0,
        OrgRole::Admin.as_str(),
        "a removal whose role change never landed"
    );

    fixture
        .scim_patch(
            &format!("/Groups/{group_id}"),
            &token.secret,
            &patch(json!([{ "op": "remove", "path": format!("members[value eq \"{ada}\"]") }])),
        )
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(
        fixture.holding(ada).await.unwrap().0,
        OrgRole::Member.as_str(),
        "the retry settles the role the first attempt left behind"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_refuses_every_group_write_but_reads() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ada = fixture.provision(&token.secret, &address()).await;
    let group_id = create(&fixture, &token.secret, "Engineering", &[ada]).await;
    fixture.inner.store_license(&sso::lapsed()).await;
    let path = format!("/Groups/{group_id}");

    for response in [
        fixture
            .scim_post("/Groups", &token.secret, &group("Design", "design", &[]))
            .await,
        fixture
            .scim_put(&path, &token.secret, &group("Renamed", "renamed", &[]))
            .await,
        fixture
            .scim_patch(
                &path,
                &token.secret,
                &patch(json!([{ "op": "remove", "path": "members" }])),
            )
            .await,
        fixture.scim_delete(&path, &token.secret).await,
    ] {
        assert_refused(&response, StatusCode::FORBIDDEN, None);
        assert_eq!(response.json_value()["detail"], json!(ENTERPRISE_MESSAGE));
    }

    assert_eq!(roster(&fixture, &token.secret, group_id).await, [ada]);
    let listed = fixture.scim_get("/Groups", &token.secret).await;
    assert_scim(&listed, StatusCode::OK);
    assert_eq!(listed.json_value()["totalResults"], json!(1));

    let recorded: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scim_errors WHERE organization_id = $1 AND status = 403",
    )
    .bind(fixture.organization)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    assert_eq!(recorded, 4);

    fixture.cleanup().await;
}
