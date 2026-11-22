//! Microsoft Entra ID's provisioning requests, replayed as Entra records them: its lookups, user
//! creation, the attribute updates it sends with and without paths, deactivation with `"False"`
//! and with the compliant boolean, permanent deletion, and a pushed group whose members change in
//! both of Entra's styles.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::db::audit::actions;
use zone_server::db::organization_members::OrgRole;
use zone_server::db::workspace_members::{self, WorkspaceRole};

use crate::common::scim::{
    Fixture, GROUP_SCHEMA, LIST_SCHEMA, Placeholders, USER_SCHEMA, assert_refused, assert_scim,
    entra_lookup, replay, resource_id,
};
use crate::common::test_email;

const PREFIX: &str = "scim_entra";
const EXTERNAL_ID: &str = "jyoung";
const GROUP_EXTERNAL_ID: &str = "8aa1a0c0-c4c3-4bc0-b4a5-2ef676900159";

fn person(user_name: &str, external_id: &str) -> Placeholders {
    Placeholders {
        user_name: Some(user_name.to_owned()),
        external_id: Some(external_id.to_owned()),
        ..Placeholders::default()
    }
}

fn expected(actions: &[&str]) -> Vec<String> {
    let mut actions: Vec<String> = actions.iter().map(|action| (*action).to_owned()).collect();
    actions.sort();
    actions
}

fn held(role: OrgRole) -> Option<(String, Option<Uuid>, bool)> {
    Some((role.as_str().to_owned(), None, true))
}

async fn create(fixture: &Fixture, token: &str, placeholders: &Placeholders) -> Uuid {
    let created = fixture
        .scim_post("/Users", token, &replay("entra/create_user", placeholders))
        .await;
    assert_scim(&created, StatusCode::CREATED);
    resource_id(&created)
}

async fn found(fixture: &Fixture, token: &str, lookup: &str) -> Value {
    let response = fixture.scim_get(lookup, token).await;
    assert_scim(&response, StatusCode::OK);
    let body = response.json_value();
    assert_eq!(body["schemas"], json!([LIST_SCHEMA]), "{lookup}");
    body
}

async fn total(fixture: &Fixture, token: &str, lookup: &str) -> u64 {
    found(fixture, token, lookup).await["totalResults"]
        .as_u64()
        .unwrap()
}

async fn patched(fixture: &Fixture, token: &str, path: &str, body: &Value) -> Value {
    let response = fixture.scim_patch(path, token, body).await;
    assert_scim(&response, StatusCode::OK);
    response.json_value()
}

async fn email(fixture: &Fixture, user: Uuid) -> String {
    sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(fixture.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn entra_looks_up_creates_updates_and_deactivates_a_member() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let account = fixture.stranger().await;
    let mut placeholders = person(&account.email, EXTERNAL_ID);

    for line in [1, 2] {
        let lookup = entra_lookup(line, &placeholders);
        assert_eq!(total(&fixture, &token.secret, &lookup).await, 0, "{lookup}");
    }

    let created = fixture
        .scim_post(
            "/Users",
            &token.secret,
            &replay("entra/create_user", &placeholders),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let user = resource_id(&created);
    assert_eq!(user, account.id);
    placeholders.user = Some(user);
    let body = created.json_value();
    assert_eq!(body["schemas"], json!([USER_SCHEMA]));
    assert_eq!(body["userName"], json!(account.email));
    assert_eq!(body["externalId"], json!(EXTERNAL_ID));
    assert_eq!(
        body["name"],
        json!({ "givenName": "givenName", "familyName": "familyName" }),
        "name.formatted is accepted and not stored"
    );
    assert_eq!(body["active"], json!(true));

    for line in [1, 3] {
        let lookup = entra_lookup(line, &placeholders);
        let listed = found(&fixture, &token.secret, &lookup).await;
        assert_eq!(listed["totalResults"], json!(1), "{lookup}");
        assert_eq!(listed["Resources"][0]["id"], json!(user), "{lookup}");
    }
    assert_eq!(
        total(&fixture, &token.secret, &entra_lookup(2, &placeholders)).await,
        0
    );

    let path = format!("/Users/{user}");
    let updated = patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/update_user", &placeholders),
    )
    .await;
    assert_eq!(
        updated["emails"][0]["value"],
        json!("updatedEmail@microsoft.com")
    );
    assert_eq!(updated["name"]["familyName"], json!("updatedFamilyName"));
    assert_eq!(updated["name"]["givenName"], json!("givenName"));

    let pathless = patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/pathless_replace", &placeholders),
    )
    .await;
    assert_eq!(
        pathless["emails"][0]["value"],
        json!("TestMhvaes@test.microsoft.com")
    );
    assert_eq!(pathless["displayName"], json!("Bjfe"));
    assert_eq!(
        pathless["name"],
        json!({ "givenName": "Kkom", "familyName": "Unua" })
    );

    let replaced = patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/replace_attributes", &placeholders),
    )
    .await;
    assert_eq!(replaced["displayName"], json!("Pvlo"));
    assert_eq!(
        replaced["emails"][0]["value"],
        json!("TestBcwqnm@test.microsoft.com")
    );
    assert_eq!(
        replaced["name"],
        json!({ "givenName": "Gtfd", "familyName": "Pkqf" })
    );
    assert_eq!(replaced["externalId"], json!("Eqpj"));

    let ignored = patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/add_attribute", &placeholders),
    )
    .await;
    assert_eq!(ignored["displayName"], json!("Pvlo"));
    assert!(ignored.get("nickName").is_none());

    let renamed_to = test_email();
    let renamed = patched(
        &fixture,
        &token.secret,
        &path,
        &replay(
            "entra/replace_user_name",
            &Placeholders {
                user_name: Some(renamed_to.clone()),
                ..placeholders.clone()
            },
        ),
    )
    .await;
    assert_eq!(renamed["userName"], json!(renamed_to));
    assert_eq!(
        total(&fixture, &token.secret, &entra_lookup(1, &placeholders)).await,
        0,
        "the old userName no longer matches"
    );
    placeholders.user_name = Some(renamed_to);
    assert_eq!(
        total(&fixture, &token.secret, &entra_lookup(1, &placeholders)).await,
        1
    );
    assert_eq!(
        email(&fixture, user).await,
        account.email,
        "an account SCIM didn't create keeps its email"
    );

    let workspace = fixture.workspace().await;
    workspace_members::add_member(fixture.pool(), workspace, user, WorkspaceRole::Member, None)
        .await
        .unwrap();
    assert!(fixture.signed_in(&account.token).await);

    let deactivated = patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/deactivate_user", &placeholders),
    )
    .await;
    assert_eq!(deactivated["active"], json!(false), "\"False\" deactivates");
    assert!(!fixture.signed_in(&account.token).await);
    assert_eq!(
        fixture.seat(workspace, user).await,
        Some((WorkspaceRole::Member.as_str().to_owned(), false))
    );

    fixture
        .scim_delete(&path, &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.holding(user).await, None);
    assert_eq!(fixture.seat(workspace, user).await, None);
    let gone = fixture.scim_get(&path, &token.secret).await;
    assert_refused(&gone, StatusCode::NOT_FOUND, None);
    assert_eq!(
        total(&fixture, &token.secret, &entra_lookup(3, &placeholders)).await,
        0,
        "the link is gone"
    );

    assert_eq!(
        fixture.scim_actions().await,
        expected(&[
            actions::SCIM_USER_CREATED,
            actions::SCIM_USER_UPDATED,
            actions::SCIM_USER_UPDATED,
            actions::SCIM_USER_UPDATED,
            actions::SCIM_USER_UPDATED,
            actions::SCIM_USER_UPDATED,
            actions::SCIM_USER_DEACTIVATED,
            actions::SCIM_USER_DELETED,
        ])
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn deleting_a_member_seats_successors_and_leaves_an_empty_workspace_ownerless() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let account = fixture.stranger().await;
    let leaving = create(&fixture, &token.secret, &person(&account.email, "leaving")).await;
    let staying_name = test_email();
    let staying = create(&fixture, &token.secret, &person(&staying_name, "staying")).await;
    let compliant = create(&fixture, &token.secret, &person(&test_email(), "compliant")).await;

    let shared = fixture.workspace().await;
    let alone = fixture.workspace().await;
    for workspace in [shared, alone] {
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
            .bind(workspace)
            .execute(fixture.pool())
            .await
            .unwrap();
        workspace_members::add_member(
            fixture.pool(),
            workspace,
            leaving,
            WorkspaceRole::Owner,
            None,
        )
        .await
        .unwrap();
    }
    workspace_members::add_member(fixture.pool(), shared, staying, WorkspaceRole::Viewer, None)
        .await
        .unwrap();

    let group = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &replay("entra/create_group", &Placeholders::default()),
        )
        .await;
    assert_scim(&group, StatusCode::CREATED);
    let group = resource_id(&group);
    let added = patched(
        &fixture,
        &token.secret,
        &format!("/Groups/{group}"),
        &replay(
            "entra/add_members",
            &Placeholders {
                user: Some(leaving),
                ..Placeholders::default()
            },
        ),
    )
    .await;
    assert_eq!(added["members"][0]["value"], json!(leaving.to_string()));

    let deactivated = patched(
        &fixture,
        &token.secret,
        &format!("/Users/{compliant}"),
        &replay("entra/deactivate_user_compliant", &Placeholders::default()),
    )
    .await;
    assert_eq!(deactivated["active"], json!(false));

    let deleted = fixture
        .scim_delete(&format!("/Users/{leaving}"), &token.secret)
        .await;
    deleted.assert_status(StatusCode::NO_CONTENT);
    assert!(deleted.text().is_empty());
    assert!(!fixture.signed_in(&account.token).await);
    assert_eq!(fixture.holding(leaving).await, None);
    for workspace in [shared, alone] {
        assert_eq!(fixture.seat(workspace, leaving).await, None);
    }
    assert_eq!(
        fixture.seat(shared, staying).await,
        Some((WorkspaceRole::Owner.as_str().to_owned(), true)),
        "the remaining member is seated as owner"
    );
    let owners: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workspace_members \
         WHERE workspace_id = $1 AND is_active AND role = 'owner'",
    )
    .bind(alone)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    assert_eq!(owners, 0, "nobody was left to seat");
    let roster = fixture
        .scim_get(&format!("/Groups/{group}"), &token.secret)
        .await
        .json_value();
    assert_eq!(roster["members"], json!([]));
    let links: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scim_users WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(leaving)
    .fetch_one(fixture.pool())
    .await
    .unwrap();
    assert_eq!(links, 0);

    let seated = fixture.audits(actions::WORKSPACE_OWNER_SEATED).await;
    assert_eq!(seated.len(), 1);
    assert_eq!(seated[0].actor_id, None);
    assert_eq!(seated[0].resource_id, Some(shared));
    assert_eq!(
        seated[0].new_values.as_ref().unwrap()["user_id"],
        json!(staying)
    );
    assert_eq!(
        fixture.scim_actions().await,
        expected(&[
            actions::SCIM_USER_CREATED,
            actions::SCIM_USER_CREATED,
            actions::SCIM_USER_CREATED,
            actions::SCIM_GROUP_CREATED,
            actions::SCIM_GROUP_UPDATED,
            actions::SCIM_USER_DEACTIVATED,
            actions::SCIM_USER_DELETED,
            actions::WORKSPACE_OWNER_SEATED,
        ])
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_pushed_group_is_looked_up_and_its_members_change_in_both_entra_styles() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let user_name = test_email();
    let user = create(&fixture, &token.secret, &person(&user_name, EXTERNAL_ID)).await;

    let created = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &replay("entra/create_group", &Placeholders::default()),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let body = created.json_value();
    assert_eq!(body["schemas"], json!([GROUP_SCHEMA]));
    assert_eq!(body["displayName"], json!("displayName"));
    assert_eq!(body["externalId"], json!(GROUP_EXTERNAL_ID));
    let group = resource_id(&created);
    let path = format!("/Groups/{group}");
    fixture
        .map(group, &json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    let placeholders = Placeholders {
        user: Some(user),
        group: Some(group),
        ..person(&user_name, EXTERNAL_ID)
    };

    let excluded = fixture
        .scim_get(&entra_lookup(4, &placeholders), &token.secret)
        .await;
    assert_scim(&excluded, StatusCode::OK);
    assert_eq!(excluded.json_value()["id"], json!(group));
    assert!(excluded.json_value().get("members").is_none());
    let named = found(&fixture, &token.secret, &entra_lookup(5, &placeholders)).await;
    assert_eq!(named["totalResults"], json!(1));
    assert_eq!(named["Resources"][0]["id"], json!(group));
    assert!(named["Resources"][0].get("members").is_none());
    for line in [6, 7] {
        let lookup = entra_lookup(line, &placeholders);
        assert_eq!(total(&fixture, &token.secret, &lookup).await, 0, "{lookup}");
    }

    let membership = |line: usize| entra_lookup(line, &placeholders);
    let removals = [
        "entra/remove_members",
        "entra/remove_member_path",
        "entra/remove_members",
    ];
    for leave in removals {
        let added = patched(
            &fixture,
            &token.secret,
            &path,
            &replay("entra/add_members", &placeholders),
        )
        .await;
        assert_eq!(added["members"][0]["value"], json!(user.to_string()));
        assert_eq!(fixture.holding(user).await, held(OrgRole::Admin));
        for line in [6, 7] {
            let listed = found(&fixture, &token.secret, &membership(line)).await;
            assert_eq!(listed["totalResults"], json!(1), "line {line}");
            assert_eq!(listed["Resources"][0]["id"], json!(group));
            assert!(
                listed["Resources"][0].get("members").is_none(),
                "attributes=id leaves members out"
            );
        }

        let left = patched(
            &fixture,
            &token.secret,
            &path,
            &replay(leave, &placeholders),
        )
        .await;
        assert_eq!(left["members"], json!([]), "{leave}");
        assert_eq!(
            fixture.holding(user).await,
            held(OrgRole::Member),
            "{leave} leaves the member in no mapped group"
        );
        for line in [6, 7] {
            assert_eq!(
                total(&fixture, &token.secret, &membership(line)).await,
                0,
                "line {line} after {leave}"
            );
        }
    }

    patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/add_members", &placeholders),
    )
    .await;
    let renamed = patched(
        &fixture,
        &token.secret,
        &path,
        &replay("entra/rename_group", &placeholders),
    )
    .await;
    assert_eq!(
        renamed["displayName"],
        json!("1879db59-3bdf-4490-ad68-ab880a269474updatedDisplayName")
    );
    assert_eq!(
        total(&fixture, &token.secret, &entra_lookup(5, &placeholders)).await,
        0
    );
    assert_eq!(fixture.holding(user).await, held(OrgRole::Admin));

    fixture
        .scim_delete(&path, &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        fixture.holding(user).await,
        held(OrgRole::Member),
        "deleting the last mapped group drops its members to Member"
    );
    let gone = fixture.scim_get(&path, &token.secret).await;
    assert_refused(&gone, StatusCode::NOT_FOUND, None);

    let mut writes = vec![actions::SCIM_USER_CREATED, actions::SCIM_GROUP_CREATED];
    writes.extend([actions::SCIM_GROUP_UPDATED; 8]);
    writes.push(actions::SCIM_GROUP_DELETED);
    assert_eq!(fixture.scim_actions().await, expected(&writes));

    fixture.cleanup().await;
}
