use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::Permission;
use zone_server::db::organization_members::{self, OrgRole};

use crate::common::TestResponse;
use crate::common::scim::{
    self, Fixture, GROUP_SCHEMA, IssuedToken, PATCH_SCHEMA, USER_SCHEMA, assert_refused,
    assert_scim, resource_id,
};
use crate::common::sso::Account;
use crate::common::test_email;

const PREFIX: &str = "scim_isolation";
const NEIGHBOUR_GROUP: &str = "Neighbours";
const NEIGHBOUR_EXTERNAL_ID: &str = "00u-neighbour";

struct Neighbour {
    owner: Account,
    organization: Uuid,
    token: IssuedToken,
    user: Uuid,
    user_name: String,
    member: Account,
    group: Uuid,
    role: Uuid,
}

fn patch(operations: Value) -> Value {
    json!({ "schemas": [PATCH_SCHEMA], "Operations": operations })
}

fn user(user_name: &str, external_id: &str) -> Value {
    json!({
        "schemas": [USER_SCHEMA],
        "userName": user_name,
        "externalId": external_id,
        "displayName": "Neighbour",
        "emails": [{ "value": user_name, "type": "work", "primary": true }],
        "active": true,
    })
}

fn filtered(attribute: &str, value: &str) -> String {
    format!("{attribute}%20eq%20%22{value}%22")
}

async fn neighbour(fixture: &Fixture) -> Neighbour {
    let owner = fixture.stranger().await;
    let organization = fixture.organization_of(&owner).await;
    let token = fixture.token_for(organization).await;
    let base = scim::base(organization);
    let user_name = test_email();
    let created = fixture
        .scim(
            Method::POST,
            &format!("{base}/Users"),
            &token.secret,
            Some(&user(&user_name, NEIGHBOUR_EXTERNAL_ID)),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let user = resource_id(&created);
    let member = fixture.member_of(organization, OrgRole::Member).await;
    let group = fixture
        .scim(
            Method::POST,
            &format!("{base}/Groups"),
            &token.secret,
            Some(&json!({
                "schemas": [GROUP_SCHEMA],
                "displayName": NEIGHBOUR_GROUP,
                "externalId": NEIGHBOUR_EXTERNAL_ID,
                "members": [{ "value": user }],
            })),
        )
        .await;
    assert_scim(&group, StatusCode::CREATED);
    let group = resource_id(&group);
    let role = fixture
        .custom_role(organization, "Neighbourly", &[Permission::AuditLogsRead])
        .await;
    fixture
        .inner
        .client
        .put_json_auth(
            &format!("/api/organizations/{organization}/scim/groups/{group}/mapping"),
            &json!({ "custom_role_id": role }),
            &owner.token,
        )
        .await
        .assert_status(StatusCode::OK);
    Neighbour {
        owner,
        organization,
        token,
        user,
        user_name,
        member,
        group,
        role,
    }
}

async fn held_in(
    fixture: &Fixture,
    organization: Uuid,
    user: Uuid,
) -> Option<(String, Option<Uuid>, bool)> {
    sqlx::query_as(
        "SELECT role, custom_role_id, is_active FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(user)
    .fetch_optional(fixture.pool())
    .await
    .unwrap()
}

async fn account(fixture: &Fixture, user: Uuid) -> (String, Option<String>) {
    sqlx::query_as("SELECT email, display_name FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

async fn errors(fixture: &Fixture, organization: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM scim_errors WHERE organization_id = $1")
        .bind(organization)
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

fn invalid(response: &TestResponse) {
    assert_refused(response, StatusCode::BAD_REQUEST, Some("invalidValue"));
}

#[tokio::test]
async fn a_token_never_reaches_another_organizations_users_groups_or_roles() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let ours = fixture.provision(&token.secret, &test_email()).await;
    let created = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &json!({ "schemas": [GROUP_SCHEMA], "displayName": "Ours", "members": [{ "value": ours }] }),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let group = resource_id(&created);
    let neighbour = neighbour(&fixture).await;
    let secret = token.secret.as_str();
    let mut refusals = 0;

    for theirs in [neighbour.user, neighbour.member.id, neighbour.owner.id] {
        let path = format!("/Users/{theirs}");
        assert_refused(
            &fixture.scim_get(&path, secret).await,
            StatusCode::NOT_FOUND,
            None,
        );
        assert_refused(
            &fixture
                .scim_patch(
                    &path,
                    secret,
                    &patch(json!([{ "op": "replace", "path": "active", "value": false }])),
                )
                .await,
            StatusCode::NOT_FOUND,
            None,
        );
        assert_refused(
            &fixture
                .scim_put(&path, secret, &user(&test_email(), "taken-over"))
                .await,
            StatusCode::NOT_FOUND,
            None,
        );
        assert_refused(
            &fixture.scim_delete(&path, secret).await,
            StatusCode::NOT_FOUND,
            None,
        );
        refusals += 4;
    }

    let theirs = format!("/Groups/{}", neighbour.group);
    assert_refused(
        &fixture.scim_get(&theirs, secret).await,
        StatusCode::NOT_FOUND,
        None,
    );
    assert_refused(
        &fixture
            .scim_patch(
                &theirs,
                secret,
                &patch(json!([{ "op": "remove", "path": "members" }])),
            )
            .await,
        StatusCode::NOT_FOUND,
        None,
    );
    assert_refused(
        &fixture
            .scim_put(
                &theirs,
                secret,
                &json!({ "schemas": [GROUP_SCHEMA], "displayName": "Taken", "members": [] }),
            )
            .await,
        StatusCode::NOT_FOUND,
        None,
    );
    assert_refused(
        &fixture.scim_delete(&theirs, secret).await,
        StatusCode::NOT_FOUND,
        None,
    );
    refusals += 4;

    let listed = fixture.scim_get("/Users", secret).await.json_value();
    assert_eq!(listed["totalResults"], json!(2), "{listed}");
    let listed: Vec<Value> = listed["Resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["id"].clone())
        .collect();
    assert_eq!(listed, vec![json!(fixture.owner.id), json!(ours)]);
    let groups = fixture.scim_get("/Groups", secret).await.json_value();
    assert_eq!(groups["totalResults"], json!(1));
    assert_eq!(groups["Resources"][0]["id"], json!(group));

    for lookup in [
        format!(
            "/Users?filter={}",
            filtered("userName", &neighbour.user_name)
        ),
        format!(
            "/Users?filter={}",
            filtered("userName", &neighbour.member.email)
        ),
        format!(
            "/Users?filter={}",
            filtered("userName", &neighbour.owner.email)
        ),
        format!(
            "/Users?filter={}",
            filtered("id", &neighbour.user.to_string())
        ),
        format!(
            "/Users?filter={}",
            filtered("externalId", NEIGHBOUR_EXTERNAL_ID)
        ),
        format!("/Users?filter={}", filtered("displayName", "Neighbour")),
        format!(
            "/Groups?filter={}",
            filtered("displayName", NEIGHBOUR_GROUP)
        ),
        format!(
            "/Groups?filter={}",
            filtered("externalId", NEIGHBOUR_EXTERNAL_ID)
        ),
        format!(
            "/Groups?filter={}",
            filtered("id", &neighbour.group.to_string())
        ),
        format!(
            "/Groups?filter={}",
            filtered("members", &neighbour.user.to_string())
        ),
    ] {
        let found = fixture.scim_get(&lookup, secret).await;
        assert_scim(&found, StatusCode::OK);
        assert_eq!(found.json_value()["totalResults"], json!(0), "{lookup}");
    }

    let ours_path = format!("/Groups/{group}");
    for theirs in [neighbour.user, neighbour.member.id] {
        invalid(
            &fixture
                .scim_post(
                    "/Groups",
                    secret,
                    &json!({
                        "schemas": [GROUP_SCHEMA],
                        "displayName": format!("Poached {theirs}"),
                        "members": [{ "value": theirs }],
                    }),
                )
                .await,
        );
        invalid(
            &fixture
                .scim_patch(
                    &ours_path,
                    secret,
                    &patch(
                        json!([{ "op": "add", "path": "members", "value": [{ "value": theirs }] }]),
                    ),
                )
                .await,
        );
        invalid(
            &fixture
                .scim_patch(
                    &ours_path,
                    secret,
                    &patch(json!([{
                        "op": "replace",
                        "path": "members",
                        "value": [{ "value": ours }, { "value": theirs }],
                    }])),
                )
                .await,
        );
        invalid(
            &fixture
                .scim_put(
                    &ours_path,
                    secret,
                    &json!({
                        "schemas": [GROUP_SCHEMA],
                        "displayName": "Ours",
                        "members": [{ "value": theirs }],
                    }),
                )
                .await,
        );
        refusals += 4;
        let removed = fixture
            .scim_patch(
                &ours_path,
                secret,
                &patch(
                    json!([{ "op": "remove", "path": format!("members[value eq \"{theirs}\"]") }]),
                ),
            )
            .await;
        assert_scim(&removed, StatusCode::OK);
    }
    let roster = fixture.scim_get(&ours_path, secret).await.json_value();
    assert_eq!(roster["members"][0]["value"], json!(ours.to_string()));
    assert_eq!(roster["members"].as_array().unwrap().len(), 1);
    assert_eq!(
        fixture.scim_get("/Groups", secret).await.json_value()["totalResults"],
        json!(1),
        "a refused group was created"
    );

    let borrowed = fixture
        .map(group, &json!({ "custom_role_id": neighbour.role }))
        .await;
    borrowed.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(borrowed.json_value()["code"], json!("scim_role_not_found"));
    assert_eq!(
        fixture.holding(ours).await,
        Some((OrgRole::Member.as_str().to_owned(), None, true))
    );

    for (owner, organization) in [
        (&fixture.owner, neighbour.organization),
        (&neighbour.owner, fixture.organization),
    ] {
        fixture
            .inner
            .client
            .get_auth(
                &format!("/api/organizations/{organization}/scim"),
                &owner.token,
            )
            .await
            .assert_status(StatusCode::FORBIDDEN);
    }
    fixture
        .inner
        .client
        .put_json_auth(
            &format!(
                "/api/organizations/{}/scim/groups/{}/mapping",
                neighbour.organization, neighbour.group
            ),
            &json!({ "role": "admin" }),
            &fixture.owner.token,
        )
        .await
        .assert_status(StatusCode::FORBIDDEN);

    assert_eq!(
        held_in(&fixture, neighbour.organization, neighbour.user).await,
        Some((
            OrgRole::Member.as_str().to_owned(),
            Some(neighbour.role),
            true
        ))
    );
    assert_eq!(
        held_in(&fixture, neighbour.organization, neighbour.member.id).await,
        Some((OrgRole::Member.as_str().to_owned(), None, true))
    );
    assert!(fixture.signed_in(&neighbour.member.token).await);
    let base = scim::base(neighbour.organization);
    let roster = fixture
        .scim(
            Method::GET,
            &format!("{base}/Groups/{}", neighbour.group),
            &neighbour.token.secret,
            None,
        )
        .await
        .json_value();
    assert_eq!(roster["displayName"], json!(NEIGHBOUR_GROUP));
    assert_eq!(
        roster["members"][0]["value"],
        json!(neighbour.user.to_string())
    );
    let mapping: Option<Uuid> =
        sqlx::query_scalar("SELECT custom_role_id FROM scim_groups WHERE id = $1")
            .bind(neighbour.group)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(mapping, Some(neighbour.role));

    assert_eq!(errors(&fixture, fixture.organization).await, refusals);
    assert_eq!(errors(&fixture, neighbour.organization).await, 0);

    fixture.cleanup().await;
}

#[tokio::test]
async fn scim_renames_an_account_only_while_no_other_organization_shares_it() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let neighbour = fixture.stranger().await;
    let other = fixture.organization_of(&neighbour).await;
    let shared = fixture.provision(&token.secret, &test_email()).await;
    let departed = fixture.provision(&token.secret, &test_email()).await;
    let alone = fixture.provision(&token.secret, &test_email()).await;
    let returned = fixture.provision(&token.secret, &test_email()).await;
    for user in [shared, departed, returned] {
        organization_members::add_member(fixture.pool(), other, user, OrgRole::Member, None)
            .await
            .unwrap();
    }
    let theirs = fixture.token_for(other).await;
    fixture
        .scim(
            Method::DELETE,
            &format!("{}/Users/{returned}", scim::base(other)),
            &theirs.secret,
            None,
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    sqlx::query(
        "UPDATE organization_members SET is_active = FALSE \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(other)
    .bind(departed)
    .execute(fixture.pool())
    .await
    .unwrap();

    for user in [shared, departed, alone, returned] {
        let before = account(&fixture, user).await;
        let email = test_email();
        let renamed = fixture
            .scim_patch(
                &format!("/Users/{user}"),
                &token.secret,
                &patch(json!([
                    { "op": "replace", "path": "displayName", "value": "Renamed" },
                    { "op": "replace", "path": "emails[type eq \"work\"].value", "value": email },
                ])),
            )
            .await;
        assert_scim(&renamed, StatusCode::OK);
        let body = renamed.json_value();
        assert_eq!(body["displayName"], json!("Renamed"));
        assert_eq!(body["emails"][0]["value"], json!(email));

        let after = account(&fixture, user).await;
        if user == alone || user == returned {
            assert_eq!(after, (email, Some("Renamed".to_owned())));
        } else {
            assert_eq!(
                after, before,
                "an account another organization shares was renamed"
            );
        }
    }

    let read = fixture
        .scim(
            Method::GET,
            &format!("{}/Users/{shared}", scim::base(other)),
            &theirs.secret,
            None,
        )
        .await;
    assert_scim(&read, StatusCode::OK);
    let (email, _) = account(&fixture, shared).await;
    assert_eq!(read.json_value()["userName"], json!(email));
    assert_eq!(read.json_value()["emails"][0]["value"], json!(email));

    fixture.cleanup().await;
}
