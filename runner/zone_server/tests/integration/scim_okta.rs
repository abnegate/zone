use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_server::auth::Permission;
use zone_server::db::audit::actions;
use zone_server::db::invitations;
use zone_server::db::organization_members::OrgRole;
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::services::scim::failure::LAST_OWNER;

use crate::common::scim::{
    Fixture, GROUP_SCHEMA, LIST_SCHEMA, Placeholders, USER_SCHEMA, assert_refused, assert_scim,
    recording, replay, resource_id,
};
use crate::common::test_email;

const PREFIX: &str = "scim_okta";
const OKTA_PASSWORD: &str = "1mz050nq";
const OKTA_USER: &str = "89bb1940-b905-4575-9e7f-6f887cfb368e";

fn person(user_name: &str, external_id: &str) -> Placeholders {
    Placeholders {
        user_name: Some(user_name.to_owned()),
        external_id: Some(external_id.to_owned()),
        ..Placeholders::default()
    }
}

fn member_of(group: Uuid, user: Uuid, user_name: &str) -> Placeholders {
    Placeholders {
        user: Some(user),
        user_name: Some(user_name.to_owned()),
        group: Some(group),
        ..Placeholders::default()
    }
}

fn lookup(user_name: &str) -> String {
    format!("/Users?filter=userName%20eq%20%22{user_name}%22&startIndex=1&count=100")
}

fn members(body: &Value) -> Vec<Uuid> {
    let mut members: Vec<Uuid> = body["members"]
        .as_array()
        .unwrap_or_else(|| panic!("no members in {body}"))
        .iter()
        .map(|member| member["value"].as_str().unwrap().parse().unwrap())
        .collect();
    members.sort();
    members
}

fn sorted(mut users: Vec<Uuid>) -> Vec<Uuid> {
    users.sort();
    users
}

fn expected(actions: &[&str]) -> Vec<String> {
    let mut actions: Vec<String> = actions.iter().map(|action| (*action).to_owned()).collect();
    actions.sort();
    actions
}

async fn create(fixture: &Fixture, token: &str, placeholders: &Placeholders) -> Uuid {
    let created = fixture
        .scim_post("/Users", token, &replay("okta/create_user", placeholders))
        .await;
    assert_scim(&created, StatusCode::CREATED);
    resource_id(&created)
}

async fn group(fixture: &Fixture, token: &str, display_name: &str, mapping: Value) -> Uuid {
    let created = fixture
        .scim_post(
            "/Groups",
            token,
            &json!({ "schemas": [GROUP_SCHEMA], "displayName": display_name, "members": [] }),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let group = resource_id(&created);
    fixture
        .map(group, &mapping)
        .await
        .assert_status(StatusCode::OK);
    group
}

async fn add(fixture: &Fixture, token: &str, placeholders: &Placeholders) {
    let group = placeholders.group.unwrap();
    let added = fixture
        .scim_patch(
            &format!("/Groups/{group}"),
            token,
            &replay("okta/add_members", placeholders),
        )
        .await;
    assert_scim(&added, StatusCode::OK);
}

async fn remove(fixture: &Fixture, token: &str, placeholders: &Placeholders) {
    let group = placeholders.group.unwrap();
    let removed = fixture
        .scim_patch(
            &format!("/Groups/{group}"),
            token,
            &replay("okta/remove_member", placeholders),
        )
        .await;
    assert_scim(&removed, StatusCode::OK);
}

fn held(role: OrgRole, custom_role: Option<Uuid>) -> Option<(String, Option<Uuid>, bool)> {
    Some((role.as_str().to_owned(), custom_role, true))
}

#[tokio::test]
async fn okta_creates_updates_deactivates_and_reactivates_a_member() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let account = fixture.stranger().await;
    let colleague = fixture.provision(&token.secret, &test_email()).await;
    let mut placeholders = person(&account.email, "00u1okta");

    let before = fixture
        .scim_get(&lookup(&account.email), &token.secret)
        .await;
    assert_scim(&before, StatusCode::OK);
    assert_eq!(before.json_value()["schemas"], json!([LIST_SCHEMA]));
    assert_eq!(before.json_value()["totalResults"], json!(0));

    let created = fixture
        .scim_post(
            "/Users",
            &token.secret,
            &replay("okta/create_user", &placeholders),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let user = resource_id(&created);
    assert_eq!(user, account.id, "the account joins as itself");
    placeholders.user = Some(user);
    let body = created.json_value();
    assert_eq!(body["schemas"], json!([USER_SCHEMA]));
    assert_eq!(body["userName"], json!(account.email));
    assert_eq!(body["externalId"], json!("00u1okta"));
    assert_eq!(body["displayName"], json!("Test User"));
    assert_eq!(
        body["name"],
        json!({ "givenName": "Test", "familyName": "User" })
    );
    assert_eq!(body["emails"][0]["value"], json!(account.email));
    assert_eq!(body["active"], json!(true));
    assert_eq!(body["groups"], json!([]));
    assert_eq!(
        created.header("location"),
        body["meta"]["location"].as_str()
    );

    let found = fixture
        .scim_get(&lookup(&account.email.to_uppercase()), &token.secret)
        .await;
    assert_scim(&found, StatusCode::OK);
    assert_eq!(found.json_value()["totalResults"], json!(1));
    assert_eq!(found.json_value()["Resources"][0], body);
    let read = fixture
        .scim_get(&format!("/Users/{user}"), &token.secret)
        .await;
    assert_scim(&read, StatusCode::OK);
    assert_eq!(read.json_value(), body);

    let workspace = fixture.workspace().await;
    workspace_members::add_member(fixture.pool(), workspace, user, WorkspaceRole::Admin, None)
        .await
        .unwrap();
    let owned = fixture.workspace().await;
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
        .bind(owned)
        .execute(fixture.pool())
        .await
        .unwrap();
    for (member, role) in [
        (user, WorkspaceRole::Owner),
        (colleague, WorkspaceRole::Member),
    ] {
        workspace_members::add_member(fixture.pool(), owned, member, role, None)
            .await
            .unwrap();
    }
    let auditors = fixture
        .custom_role(
            fixture.organization,
            "Auditors",
            &[Permission::AuditLogsRead],
        )
        .await;
    sqlx::query(
        "UPDATE organization_members SET custom_role_id = $3 \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(fixture.organization)
    .bind(user)
    .bind(auditors)
    .execute(fixture.pool())
    .await
    .unwrap();
    let (invitation, _) = invitations::create_invitation(
        fixture.pool(),
        &account.email,
        fixture.organization,
        vec![workspace],
        "member",
        "member",
        fixture.owner.id,
    )
    .await
    .unwrap();
    assert!(fixture.signed_in(&account.token).await);

    let path = format!("/Users/{user}");
    let replaced = fixture
        .scim_put(
            &path,
            &token.secret,
            &replay("okta/replace_user", &placeholders),
        )
        .await;
    assert_scim(&replaced, StatusCode::OK);
    let body = replaced.json_value();
    assert_eq!(
        body["name"],
        json!({ "givenName": "Another", "familyName": "User" })
    );
    assert!(body.get("externalId").is_none(), "PUT replaces externalId");
    assert_eq!(body["active"], json!(true));
    assert_eq!(
        fixture.holding(user).await,
        held(OrgRole::Member, Some(auditors)),
        "no mapping governs the member, so their custom role stays"
    );

    let deactivated = fixture
        .scim_patch(
            &path,
            &token.secret,
            &replay("okta/deactivate_user", &placeholders),
        )
        .await;
    assert_scim(&deactivated, StatusCode::OK);
    assert_eq!(deactivated.json_value()["active"], json!(false));
    assert!(
        !fixture.signed_in(&account.token).await,
        "the member's access token still works"
    );
    assert!(!fixture.holding(user).await.unwrap().2);
    assert_eq!(
        fixture.seat(workspace, user).await,
        Some((WorkspaceRole::Admin.as_str().to_owned(), false))
    );
    assert!(
        !workspace_members::can_read(fixture.pool(), workspace, user)
            .await
            .unwrap()
    );
    assert!(
        invitations::get_invitation_by_id(fixture.pool(), invitation.id)
            .await
            .unwrap()
            .is_none(),
        "the pending invitation survived"
    );
    assert_eq!(
        fixture.seat(owned, colleague).await,
        Some((WorkspaceRole::Owner.as_str().to_owned(), true)),
        "nobody was seated as owner"
    );
    let seated = fixture.audits(actions::WORKSPACE_OWNER_SEATED).await;
    assert_eq!(seated.len(), 1);
    assert_eq!(seated[0].actor_id, None);
    assert_eq!(seated[0].resource_id, Some(owned));
    assert_eq!(
        seated[0].new_values.as_ref().unwrap()["user_id"],
        json!(colleague)
    );
    let listed = fixture
        .scim_get(&lookup(&account.email), &token.secret)
        .await
        .json_value();
    assert_eq!(listed["Resources"][0]["active"], json!(false));

    let reactivated = fixture
        .scim_patch(
            &path,
            &token.secret,
            &replay("okta/reactivate_user", &placeholders),
        )
        .await;
    assert_scim(&reactivated, StatusCode::OK);
    assert_eq!(reactivated.json_value()["active"], json!(true));
    assert_eq!(
        fixture.holding(user).await,
        held(OrgRole::Member, Some(auditors))
    );
    assert_eq!(
        fixture.seat(workspace, user).await,
        Some((WorkspaceRole::Admin.as_str().to_owned(), true))
    );
    assert!(
        workspace_members::can_read(fixture.pool(), workspace, user)
            .await
            .unwrap()
    );
    assert_eq!(
        fixture.seat(owned, user).await,
        Some((WorkspaceRole::Owner.as_str().to_owned(), true)),
        "the member comes back as owner beside their successor"
    );
    assert!(
        !fixture.signed_in(&account.token).await,
        "sessions never come back"
    );

    assert_eq!(
        fixture.scim_actions().await,
        expected(&[
            actions::SCIM_USER_CREATED,
            actions::SCIM_USER_CREATED,
            actions::SCIM_USER_UPDATED,
            actions::SCIM_USER_DEACTIVATED,
            actions::WORKSPACE_OWNER_SEATED,
            actions::SCIM_USER_REACTIVATED,
        ])
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_account_okta_creates_is_named_by_okta_and_takes_no_password() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let user_name = test_email();
    let user = create(&fixture, &token.secret, &person(&user_name, "00u2okta")).await;

    let (email, display_name, verified): (String, Option<String>, bool) =
        sqlx::query_as("SELECT email, display_name, email_verified FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(email, user_name);
    assert_eq!(display_name.as_deref(), Some("Test User"));
    assert!(!verified);
    fixture
        .inner
        .login(&user_name, OKTA_PASSWORD)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    let memberships: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM organization_members WHERE user_id = $1")
            .bind(user)
            .fetch_one(fixture.pool())
            .await
            .unwrap();
    assert_eq!(
        memberships, 1,
        "the account gets no organization of its own"
    );

    let mut inactive = replay("okta/create_user", &person(&test_email(), "00u3okta"));
    inactive["active"] = json!(false);
    let created = fixture.scim_post("/Users", &token.secret, &inactive).await;
    assert_scim(&created, StatusCode::CREATED);
    assert_eq!(created.json_value()["active"], json!(false));
    assert!(!fixture.holding(resource_id(&created)).await.unwrap().2);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_pushed_group_mapped_to_admin_sets_and_a_filtered_removal_resets_the_role() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let first_name = test_email();
    let first = create(&fixture, &token.secret, &person(&first_name, "00u-first")).await;
    let second_name = test_email();
    let second = create(&fixture, &token.secret, &person(&second_name, "00u-second")).await;

    let created = fixture
        .scim_post(
            "/Groups",
            &token.secret,
            &replay("okta/create_group", &Placeholders::default()),
        )
        .await;
    assert_scim(&created, StatusCode::CREATED);
    let body = created.json_value();
    assert_eq!(body["schemas"], json!([GROUP_SCHEMA]));
    assert_eq!(body["displayName"], json!("Test SCIMv2"));
    assert_eq!(body["members"], json!([]));
    assert_eq!(
        created.header("location"),
        body["meta"]["location"].as_str()
    );
    let group = resource_id(&created);
    let path = format!("/Groups/{group}");
    fixture
        .map(group, &json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    let placeholders = member_of(group, first, &first_name);

    let renamed = fixture
        .scim_patch(
            &path,
            &token.secret,
            &replay("okta/rename_group", &placeholders),
        )
        .await;
    assert_scim(&renamed, StatusCode::OK);
    assert_eq!(renamed.json_value()["displayName"], json!("Test SCIMv2"));

    let added = fixture
        .scim_patch(
            &path,
            &token.secret,
            &replay("okta/add_members", &placeholders),
        )
        .await;
    assert_scim(&added, StatusCode::OK);
    assert_eq!(members(&added.json_value()), vec![first]);
    assert_eq!(fixture.holding(first).await, held(OrgRole::Admin, None));
    assert_eq!(fixture.holding(second).await, held(OrgRole::Member, None));
    let user = fixture
        .scim_get(&format!("/Users/{first}"), &token.secret)
        .await
        .json_value();
    assert_eq!(user["groups"][0]["value"], json!(group.to_string()));
    assert_eq!(user["groups"][0]["display"], json!("Test SCIMv2"));

    let replaced = fixture
        .scim_put(
            &path,
            &token.secret,
            &replay("okta/replace_group", &placeholders),
        )
        .await;
    assert_scim(&replaced, StatusCode::OK);
    assert_eq!(members(&replaced.json_value()), vec![first]);

    let unknown = fixture
        .scim_patch(
            &path,
            &token.secret,
            &replay("okta/replace_members", &placeholders),
        )
        .await;
    assert_refused(&unknown, StatusCode::BAD_REQUEST, Some("invalidValue"));
    assert!(
        unknown.json_value()["detail"]
            .as_str()
            .unwrap()
            .contains(OKTA_USER)
    );
    let unchanged = fixture.scim_get(&path, &token.secret).await.json_value();
    assert_eq!(
        members(&unchanged),
        vec![first],
        "a refusal changes nothing"
    );

    let both: Value = serde_json::from_str(
        &placeholders
            .substitute(&recording("okta/replace_members.json"))
            .replace(OKTA_USER, &second.to_string()),
    )
    .unwrap();
    let replaced = fixture.scim_patch(&path, &token.secret, &both).await;
    assert_scim(&replaced, StatusCode::OK);
    assert_eq!(members(&replaced.json_value()), sorted(vec![first, second]));
    assert_eq!(fixture.holding(second).await, held(OrgRole::Admin, None));

    let removed = fixture
        .scim_patch(
            &path,
            &token.secret,
            &replay("okta/remove_member", &placeholders),
        )
        .await;
    assert_scim(&removed, StatusCode::OK);
    assert_eq!(members(&removed.json_value()), vec![second]);
    assert_eq!(
        fixture.holding(first).await,
        held(OrgRole::Member, None),
        "a member in no mapped group falls back to Member"
    );
    assert_eq!(fixture.holding(second).await, held(OrgRole::Admin, None));

    let deleted = fixture.scim_delete(&path, &token.secret).await;
    deleted.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        fixture.holding(second).await,
        held(OrgRole::Member, None),
        "deleting the last mapped group drops its members to Member"
    );
    let gone = fixture.scim_get(&path, &token.secret).await;
    assert_refused(&gone, StatusCode::NOT_FOUND, None);

    let deletions = fixture.audits(actions::SCIM_GROUP_DELETED).await;
    assert_eq!(deletions.len(), 1);
    assert_eq!(
        deletions[0].new_values.as_ref().unwrap()["roles"],
        json!([{ "user_id": second, "role": "member", "custom_role": null }])
    );
    assert_eq!(
        fixture.scim_actions().await,
        expected(&[
            actions::SCIM_USER_CREATED,
            actions::SCIM_USER_CREATED,
            actions::SCIM_GROUP_CREATED,
            actions::SCIM_GROUP_UPDATED,
            actions::SCIM_GROUP_UPDATED,
            actions::SCIM_GROUP_UPDATED,
            actions::SCIM_GROUP_UPDATED,
            actions::SCIM_GROUP_UPDATED,
            actions::SCIM_GROUP_DELETED,
        ])
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_larger_grant_wins_then_a_built_in_role_then_the_name() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let user_name = test_email();
    let user = create(&fixture, &token.secret, &person(&user_name, "00u-ranked")).await;
    let organization = fixture.organization;
    let auditors = fixture
        .custom_role(organization, "auditors", &[Permission::AuditLogsRead])
        .await;
    let alpha = fixture
        .custom_role(organization, "Alpha", &[Permission::DevicesRead])
        .await;
    let billing = fixture
        .custom_role(
            organization,
            "Billing",
            &[Permission::SpendRead, Permission::AuditLogsRead],
        )
        .await;

    let everyone = group(
        &fixture,
        &token.secret,
        "Everyone",
        json!({ "role": "member" }),
    )
    .await;
    let auditing = group(
        &fixture,
        &token.secret,
        "Auditing",
        json!({ "custom_role_id": auditors }),
    )
    .await;
    let first = group(
        &fixture,
        &token.secret,
        "First",
        json!({ "custom_role_id": alpha }),
    )
    .await;
    let finance = group(
        &fixture,
        &token.secret,
        "Finance",
        json!({ "custom_role_id": billing }),
    )
    .await;
    let administrators = group(
        &fixture,
        &token.secret,
        "Administrators",
        json!({ "role": "admin" }),
    )
    .await;

    let steps = [
        (everyone, held(OrgRole::Member, None)),
        (auditing, held(OrgRole::Member, Some(auditors))),
        (first, held(OrgRole::Member, Some(alpha))),
        (finance, held(OrgRole::Member, Some(billing))),
        (administrators, held(OrgRole::Admin, None)),
    ];
    for (group, role) in &steps {
        add(
            &fixture,
            &token.secret,
            &member_of(*group, user, &user_name),
        )
        .await;
        assert_eq!(fixture.holding(user).await, *role, "joining {group}");
    }

    let leaving = [
        (administrators, held(OrgRole::Member, Some(billing))),
        (finance, held(OrgRole::Member, Some(alpha))),
        (first, held(OrgRole::Member, Some(auditors))),
        (auditing, held(OrgRole::Member, None)),
        (everyone, held(OrgRole::Member, None)),
    ];
    for (group, role) in &leaving {
        remove(
            &fixture,
            &token.secret,
            &member_of(*group, user, &user_name),
        )
        .await;
        assert_eq!(fixture.holding(user).await, *role, "leaving {group}");
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_member_reactivated_after_their_mapping_went_comes_back_as_member() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let deactivation = replay("okta/deactivate_user", &Placeholders::default());
    let reactivation = replay("okta/reactivate_user", &Placeholders::default());
    let deleted_name = test_email();
    let deleted = create(
        &fixture,
        &token.secret,
        &person(&deleted_name, "00u-deleted"),
    )
    .await;
    let unmapped_name = test_email();
    let unmapped = create(
        &fixture,
        &token.secret,
        &person(&unmapped_name, "00u-unmapped"),
    )
    .await;

    let doomed = group(
        &fixture,
        &token.secret,
        "Doomed",
        json!({ "role": "admin" }),
    )
    .await;
    let administrators = group(
        &fixture,
        &token.secret,
        "Administrators",
        json!({ "role": "admin" }),
    )
    .await;
    add(
        &fixture,
        &token.secret,
        &member_of(doomed, deleted, &deleted_name),
    )
    .await;
    add(
        &fixture,
        &token.secret,
        &member_of(administrators, unmapped, &unmapped_name),
    )
    .await;
    for user in [deleted, unmapped] {
        assert_eq!(fixture.holding(user).await, held(OrgRole::Admin, None));
        assert_scim(
            &fixture
                .scim_patch(&format!("/Users/{user}"), &token.secret, &deactivation)
                .await,
            StatusCode::OK,
        );
    }

    fixture
        .scim_delete(&format!("/Groups/{doomed}"), &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    fixture
        .inner
        .client
        .delete_auth(
            &format!(
                "/api/organizations/{}/scim/groups/{administrators}/mapping",
                fixture.organization
            ),
            &fixture.owner.token,
        )
        .await
        .assert_status(StatusCode::OK);

    for user in [deleted, unmapped] {
        let reactivated = fixture
            .scim_patch(&format!("/Users/{user}"), &token.secret, &reactivation)
            .await;
        assert_scim(&reactivated, StatusCode::OK);
        assert_eq!(
            fixture.holding(user).await,
            held(OrgRole::Member, None),
            "a member came back with the admin role a mapping no longer grants"
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_reactivated_member_takes_the_role_their_groups_now_give() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let deactivation = replay("okta/deactivate_user", &Placeholders::default());
    let reactivation = replay("okta/reactivate_user", &Placeholders::default());
    let ungrouped = create(
        &fixture,
        &token.secret,
        &person(&test_email(), "00u-ungrouped"),
    )
    .await;
    let promoted_name = test_email();
    let promoted = create(
        &fixture,
        &token.secret,
        &person(&promoted_name, "00u-promoted"),
    )
    .await;
    let auditors = fixture
        .custom_role(
            fixture.organization,
            "Auditors",
            &[Permission::AuditLogsRead],
        )
        .await;
    sqlx::query(
        "UPDATE organization_members SET custom_role_id = $2 \
         WHERE organization_id = $1 AND user_id = ANY($3)",
    )
    .bind(fixture.organization)
    .bind(auditors)
    .bind(vec![ungrouped, promoted])
    .execute(fixture.pool())
    .await
    .unwrap();
    let administrators = group(
        &fixture,
        &token.secret,
        "Administrators",
        json!({ "role": "admin" }),
    )
    .await;
    for user in [ungrouped, promoted] {
        assert_eq!(
            fixture.holding(user).await,
            held(OrgRole::Member, Some(auditors)),
            "mapping a group nobody is in re-roles nobody"
        );
        assert_scim(
            &fixture
                .scim_patch(&format!("/Users/{user}"), &token.secret, &deactivation)
                .await,
            StatusCode::OK,
        );
    }
    add(
        &fixture,
        &token.secret,
        &member_of(administrators, promoted, &promoted_name),
    )
    .await;
    assert!(
        !fixture.holding(promoted).await.unwrap().2,
        "joining a group reactivated the member"
    );

    for (user, role) in [
        (ungrouped, held(OrgRole::Member, None)),
        (promoted, held(OrgRole::Admin, None)),
    ] {
        assert_scim(
            &fixture
                .scim_patch(&format!("/Users/{user}"), &token.secret, &reactivation)
                .await,
            StatusCode::OK,
        );
        assert_eq!(fixture.holding(user).await, role);
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_console_member_comes_under_scim_at_the_first_attribute_change() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let admin = fixture.member(OrgRole::Admin).await;
    let path = format!("/Users/{}", admin.id);
    let everyone = group(
        &fixture,
        &token.secret,
        "Everyone",
        json!({ "role": "member" }),
    )
    .await;
    let placeholders = member_of(everyone, admin.id, &admin.email);
    add(&fixture, &token.secret, &placeholders).await;
    assert_eq!(
        fixture.holding(admin.id).await,
        held(OrgRole::Admin, None),
        "a member SCIM doesn't manage keeps their role"
    );

    let listed = fixture
        .scim_get(&lookup(&admin.email), &token.secret)
        .await
        .json_value();
    assert_eq!(listed["totalResults"], json!(1));
    assert_eq!(listed["Resources"][0]["userName"], json!(admin.email));

    for operation in ["okta/deactivate_user", "okta/reactivate_user"] {
        assert_scim(
            &fixture
                .scim_patch(&path, &token.secret, &replay(operation, &placeholders))
                .await,
            StatusCode::OK,
        );
        let links: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scim_users WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(fixture.organization)
        .bind(admin.id)
        .fetch_one(fixture.pool())
        .await
        .unwrap();
        assert_eq!(links, 0, "{operation} linked the member");
    }
    assert_eq!(fixture.holding(admin.id).await, held(OrgRole::Admin, None));

    let renamed = fixture
        .scim_put(
            &path,
            &token.secret,
            &replay(
                "okta/replace_user",
                &Placeholders {
                    user: Some(admin.id),
                    ..person(&admin.email, "00u-admin")
                },
            ),
        )
        .await;
    assert_scim(&renamed, StatusCode::OK);
    assert_eq!(
        fixture.holding(admin.id).await,
        held(OrgRole::Member, None),
        "once linked, the mapping governs the member"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn owners_are_never_re_roled_and_the_last_active_owner_stays() {
    let fixture = Fixture::new(PREFIX).await;
    let token = fixture.token().await;
    let second = fixture.member(OrgRole::Owner).await;
    let owners = [
        (fixture.owner.id, fixture.owner.email.clone()),
        (second.id, second.email.clone()),
    ];
    for (owner, email) in &owners {
        let placeholders = Placeholders {
            user: Some(*owner),
            ..person(email, "00u-owner")
        };
        let linked = fixture
            .scim_put(
                &format!("/Users/{owner}"),
                &token.secret,
                &replay("okta/replace_user", &placeholders),
            )
            .await;
        assert_scim(&linked, StatusCode::OK);
    }

    let everyone = group(
        &fixture,
        &token.secret,
        "Everyone",
        json!({ "role": "member" }),
    )
    .await;
    let administrators = group(
        &fixture,
        &token.secret,
        "Administrators",
        json!({ "role": "admin" }),
    )
    .await;
    for (owner, email) in &owners {
        for group in [everyone, administrators] {
            add(&fixture, &token.secret, &member_of(group, *owner, email)).await;
            assert_eq!(fixture.holding(*owner).await, held(OrgRole::Owner, None));
        }
        remove(
            &fixture,
            &token.secret,
            &member_of(administrators, *owner, email),
        )
        .await;
        remove(&fixture, &token.secret, &member_of(everyone, *owner, email)).await;
        assert_eq!(fixture.holding(*owner).await, held(OrgRole::Owner, None));
    }

    let deactivation = replay("okta/deactivate_user", &Placeholders::default());
    let reactivation = replay("okta/reactivate_user", &Placeholders::default());
    let second_path = format!("/Users/{}", second.id);
    let owner_path = format!("/Users/{}", fixture.owner.id);
    assert_scim(
        &fixture
            .scim_patch(&second_path, &token.secret, &deactivation)
            .await,
        StatusCode::OK,
    );
    let last = fixture
        .scim_patch(&owner_path, &token.secret, &deactivation)
        .await;
    assert_refused(&last, StatusCode::CONFLICT, None);
    assert_eq!(last.json_value()["detail"], json!(LAST_OWNER));
    let deleted = fixture.scim_delete(&owner_path, &token.secret).await;
    assert_refused(&deleted, StatusCode::CONFLICT, None);
    assert_eq!(
        fixture.holding(fixture.owner.id).await,
        held(OrgRole::Owner, None)
    );

    assert_scim(
        &fixture
            .scim_patch(&second_path, &token.secret, &reactivation)
            .await,
        StatusCode::OK,
    );
    assert_eq!(
        fixture.holding(second.id).await,
        held(OrgRole::Owner, None),
        "a reactivated owner is an owner again"
    );
    fixture
        .scim_delete(&second_path, &token.secret)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(fixture.holding(second.id).await, None);
    let alone = fixture.scim_delete(&owner_path, &token.secret).await;
    assert_refused(&alone, StatusCode::CONFLICT, None);
    assert_eq!(
        fixture.holding(fixture.owner.id).await,
        held(OrgRole::Owner, None)
    );

    fixture.cleanup().await;
}
