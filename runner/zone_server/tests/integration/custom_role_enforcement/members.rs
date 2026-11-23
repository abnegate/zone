//! Custom roles through the member routes: adding and reactivating members,
//! the rank member lists report, escalation, what the license gates, what an
//! assignment answers and audits, invitations, and the refusals a former
//! member reads.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::{LICENSE_REQUIRED, MEMBER_CAP_REACHED};
use zone_server::auth::{Permission, Scope};
use zone_server::db::audit::actions;
use zone_server::db::custom_roles::{AMBIGUOUS_ASSIGNMENT, EXCEEDS};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use super::locking::waiting;
use super::{
    Call, composed, describe, fixture, holding_organization, holding_workspace, licensed,
    organization_holder, refused_with, role, seat_organization, seat_workspace, user,
    workspace_holder,
};
use crate::common::TestResponse;
use crate::common::policy::Fixture;
use crate::common::sso::{Account, lapsed};

fn organization_members_path(fixture: &Fixture) -> String {
    format!("/api/organizations/{}/members", fixture.organization)
}

fn workspace_members_path(fixture: &Fixture) -> String {
    format!("/api/workspaces/{}/members", fixture.workspace)
}

async fn organization_member(fixture: &Fixture) -> Uuid {
    let member = user(fixture.pool()).await;
    seat_organization(
        fixture.pool(),
        fixture.organization,
        member,
        OrgRole::Member,
    )
    .await;
    member
}

async fn workspace_viewer(fixture: &Fixture) -> Uuid {
    let member = organization_member(fixture).await;
    seat_workspace(
        fixture.pool(),
        fixture.workspace,
        member,
        WorkspaceRole::Viewer,
    )
    .await;
    member
}

fn row(listing: &Value, member: Uuid) -> Value {
    listing["members"]
        .as_array()
        .expect("a member list")
        .iter()
        .find(|row| row["user_id"] == member.to_string())
        .cloned()
        .unwrap_or_else(|| panic!("{member} is listed: {listing}"))
}

fn permission_names(row: &Value) -> Vec<String> {
    row["permissions"]
        .as_array()
        .unwrap_or_else(|| panic!("the row lists its permissions: {row}"))
        .iter()
        .map(|name| name.as_str().expect("a permission name").to_string())
        .collect()
}

fn names_of(scope: Scope, permissions: &[Permission]) -> Vec<String> {
    composed(scope, permissions)
        .names()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn an_organization_holder_adds_built_in_members_but_never_admins() {
    let fixture = licensed().await;
    let holder = organization_holder(&fixture, &[Permission::MembersCreate]).await;

    let member = user(fixture.pool()).await;
    let call = Call::post(
        organization_members_path(&fixture),
        json!({ "user_id": member, "role": OrgRole::Member.as_str() }),
    );
    let response = call.send(&fixture, &holder.token).await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "{}",
        describe(&call, &response)
    );

    for refused in [OrgRole::Admin, OrgRole::Owner] {
        let target = user(fixture.pool()).await;
        let call = Call::post(
            organization_members_path(&fixture),
            json!({ "user_id": target, "role": refused.as_str() }),
        );
        let response = call.send(&fixture, &holder.token).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{}",
            describe(&call, &response)
        );
        assert!(
            organization_members::membership(fixture.pool(), fixture.organization, target)
                .await
                .expect("the membership is read")
                .is_none(),
            "a refused add seats nobody"
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_holder_without_write_adds_viewers_but_never_members() {
    let fixture = licensed().await;
    let holder = workspace_holder(&fixture, &[Permission::WorkspaceMembersCreate]).await;

    let newcomer = organization_member(&fixture).await;
    let former = workspace_viewer(&fixture).await;
    workspace_members::remove_member(fixture.pool(), fixture.workspace, former)
        .await
        .expect("the former member is removed");

    for (path, target) in [("new member", newcomer), ("reactivation", former)] {
        let call = Call::post(
            workspace_members_path(&fixture),
            json!({ "user_id": target, "role": WorkspaceRole::Member.as_str() }),
        );
        let response = call.send(&fixture, &holder.token).await;
        assert!(
            refused_with(&response, EXCEEDS),
            "{path}: {}",
            describe(&call, &response)
        );
        assert!(
            workspace_members::membership(fixture.pool(), fixture.workspace, target)
                .await
                .expect("the membership is read")
                .is_none(),
            "{path}: a refused add seats nobody"
        );

        let call = Call::post(
            workspace_members_path(&fixture),
            json!({ "user_id": target, "role": WorkspaceRole::Viewer.as_str() }),
        );
        let response = call.send(&fixture, &holder.token).await;
        assert!(
            response.status.is_success(),
            "{path}: {}",
            describe(&call, &response)
        );
        let seated = workspace_members::membership(fixture.pool(), fixture.workspace, target)
            .await
            .expect("the membership is read")
            .expect("the viewer is seated");
        assert_eq!(seated.role, WorkspaceRole::Viewer, "{path}");
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_role_with_write_chats_and_writes_tasks() {
    let fixture = licensed().await;
    let writer = workspace_holder(&fixture, &[Permission::WorkspaceWrite]).await;
    let reader = workspace_holder(&fixture, &[]).await;
    assert!(
        workspace_members::can_write(fixture.pool(), fixture.workspace, writer.id)
            .await
            .expect("the permission is read")
    );

    let task = Call::post(
        format!("/api/workspaces/{}/tasks", fixture.workspace),
        json!({ "description": "Written by a custom role" }),
    );
    let chat = Call::post(
        "/api/chats",
        json!({
            "workspace_id": fixture.workspace,
            "title": "Started by a custom role",
            "model_name": "probe",
        }),
    );
    for call in [&task, &chat] {
        let response = call.send(&fixture, &writer.token).await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "{}",
            describe(call, &response)
        );
        let response = call.send(&fixture, &reader.token).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{}",
            describe(call, &response)
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn member_lists_rank_a_custom_role_by_the_built_in_role_it_covers() {
    let fixture = licensed().await;
    let organization_permissions = [Permission::MembersCreate, Permission::AuditLogsRead];
    let organization_role = role(
        fixture.pool(),
        fixture.organization,
        Scope::Organization,
        &organization_permissions,
    )
    .await;
    let organization_holder = holding_organization(&fixture, organization_role).await;

    let listing = Call::get(organization_members_path(&fixture))
        .send(&fixture, &fixture.owner.token)
        .await;
    listing.assert_status(StatusCode::OK);
    let listed = row(&listing.json_value(), organization_holder.id);
    assert_eq!(listed["role"], OrgRole::Member.as_str(), "{listed}");
    assert_eq!(
        listed["custom_role"]["id"],
        organization_role.to_string(),
        "{listed}"
    );
    assert_eq!(
        permission_names(&listed),
        names_of(Scope::Organization, &organization_permissions)
    );

    let reading = role(fixture.pool(), fixture.organization, Scope::Workspace, &[]).await;
    let writing_permissions = [Permission::WorkspaceWrite, Permission::ProjectSyncsUpdate];
    let writing = role(
        fixture.pool(),
        fixture.organization,
        Scope::Workspace,
        &writing_permissions,
    )
    .await;
    let reader = holding_workspace(&fixture, reading).await;
    let writer = holding_workspace(&fixture, writing).await;

    let listing = Call::get(workspace_members_path(&fixture))
        .send(&fixture, &fixture.owner.token)
        .await;
    listing.assert_status(StatusCode::OK);
    let listing = listing.json_value();
    for (holder, role, rank, permissions) in [
        (&reader, reading, WorkspaceRole::Viewer, &[][..]),
        (
            &writer,
            writing,
            WorkspaceRole::Member,
            &writing_permissions[..],
        ),
    ] {
        let listed = row(&listing, holder.id);
        assert_eq!(listed["role"], rank.as_str(), "{listed}");
        assert_eq!(listed["custom_role"]["id"], role.to_string(), "{listed}");
        assert_eq!(
            permission_names(&listed),
            names_of(Scope::Workspace, permissions)
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn escalation_through_the_member_routes_is_refused_with_role_exceeds() {
    let fixture = licensed().await;
    let organization_updater = organization_holder(&fixture, &[Permission::MembersUpdate]).await;
    let auditors = role(
        fixture.pool(),
        fixture.organization,
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let workspace_updater = workspace_holder(&fixture, &[Permission::WorkspaceMembersUpdate]).await;
    let writers = role(
        fixture.pool(),
        fixture.organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;

    let member = organization_member(&fixture).await;
    let path = format!("{}/{member}", organization_members_path(&fixture));
    for body in [
        json!({ "role": OrgRole::Admin.as_str() }),
        json!({ "custom_role_id": auditors }),
    ] {
        let call = Call::patch(path.clone(), body);
        let response = call.send(&fixture, &organization_updater.token).await;
        assert!(
            refused_with(&response, EXCEEDS),
            "{}",
            describe(&call, &response)
        );
    }

    let viewer = workspace_viewer(&fixture).await;
    let path = format!("{}/{viewer}", workspace_members_path(&fixture));
    for body in [
        json!({ "role": WorkspaceRole::Member.as_str() }),
        json!({ "custom_role_id": writers }),
    ] {
        let call = Call::patch(path.clone(), body);
        let response = call.send(&fixture, &workspace_updater.token).await;
        assert!(
            refused_with(&response, EXCEEDS),
            "{}",
            describe(&call, &response)
        );
    }

    let membership = organization_members::membership(fixture.pool(), fixture.organization, member)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(
        (membership.role, membership.custom_role),
        (OrgRole::Member, None)
    );
    let membership = workspace_members::membership(fixture.pool(), fixture.workspace, viewer)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(
        (membership.role, membership.custom_role),
        (WorkspaceRole::Viewer, None)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_holder_never_changes_or_removes_an_admin_or_owner_through_the_routes() {
    let fixture = licensed().await;
    let organization_manager = organization_holder(
        &fixture,
        &[Permission::MembersUpdate, Permission::MembersDelete],
    )
    .await;
    let workspace_manager = workspace_holder(
        &fixture,
        &[
            Permission::WorkspaceMembersUpdate,
            Permission::WorkspaceMembersDelete,
        ],
    )
    .await;
    let admin = fixture
        .member(OrgRole::Admin, Some(WorkspaceRole::Admin))
        .await;

    for target in [admin.id, fixture.owner.id] {
        for (call, manager) in [
            (
                Call::patch(
                    format!("{}/{target}", organization_members_path(&fixture)),
                    json!({ "role": OrgRole::Member.as_str() }),
                ),
                &organization_manager,
            ),
            (
                Call::delete(format!("{}/{target}", organization_members_path(&fixture))),
                &organization_manager,
            ),
            (
                Call::patch(
                    format!("{}/{target}", workspace_members_path(&fixture)),
                    json!({ "role": WorkspaceRole::Viewer.as_str() }),
                ),
                &workspace_manager,
            ),
            (
                Call::delete(format!("{}/{target}", workspace_members_path(&fixture))),
                &workspace_manager,
            ),
        ] {
            let response = call.send(&fixture, &manager.token).await;
            assert_eq!(
                response.status,
                StatusCode::FORBIDDEN,
                "{}",
                describe(&call, &response)
            );
        }
    }

    let membership =
        organization_members::membership(fixture.pool(), fixture.organization, admin.id)
            .await
            .expect("the membership is read")
            .expect("the admin stays");
    assert_eq!(membership.role, OrgRole::Admin);
    let membership = workspace_members::membership(fixture.pool(), fixture.workspace, admin.id)
        .await
        .expect("the membership is read")
        .expect("the admin stays");
    assert_eq!(membership.role, WorkspaceRole::Admin);

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_refuses_custom_assignments_but_not_built_in_ones() {
    let fixture = licensed().await;
    let organization_role = role(
        fixture.pool(),
        fixture.organization,
        Scope::Organization,
        &[],
    )
    .await;
    let workspace_role = role(fixture.pool(), fixture.organization, Scope::Workspace, &[]).await;
    let member = organization_member(&fixture).await;
    let viewer = workspace_viewer(&fixture).await;
    fixture.store_license(&lapsed()).await;

    let organization_path = format!("{}/{member}", organization_members_path(&fixture));
    let workspace_path = format!("{}/{viewer}", workspace_members_path(&fixture));
    for call in [
        Call::patch(
            organization_path.clone(),
            json!({ "custom_role_id": organization_role }),
        ),
        Call::patch(
            workspace_path.clone(),
            json!({ "custom_role_id": workspace_role }),
        ),
    ] {
        let response = call.send(&fixture, &fixture.owner.token).await;
        assert!(
            refused_with(&response, LICENSE_REQUIRED),
            "{}",
            describe(&call, &response)
        );
    }
    for call in [
        Call::patch(
            organization_path,
            json!({ "role": OrgRole::Admin.as_str() }),
        ),
        Call::patch(
            workspace_path,
            json!({ "role": WorkspaceRole::Member.as_str() }),
        ),
    ] {
        let response = call.send(&fixture, &fixture.owner.token).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{}",
            describe(&call, &response)
        );
    }

    let membership = organization_members::membership(fixture.pool(), fixture.organization, member)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(
        (membership.role, membership.custom_role),
        (OrgRole::Admin, None)
    );
    let membership = workspace_members::membership(fixture.pool(), fixture.workspace, viewer)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(
        (membership.role, membership.custom_role),
        (WorkspaceRole::Member, None)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_lapsed_license_keeps_enforcing_custom_roles() {
    let fixture = licensed().await;
    let updater = organization_holder(&fixture, &[Permission::OrganizationUpdate]).await;
    let organization_reader = organization_holder(&fixture, &[]).await;
    let writer = workspace_holder(&fixture, &[Permission::WorkspaceWrite]).await;
    let workspace_reader = workspace_holder(&fixture, &[]).await;
    fixture.store_license(&lapsed()).await;

    let update = Call::patch(
        format!("/api/organizations/{}", fixture.organization),
        json!({ "description": "Edited after the license lapsed" }),
    );
    let task = Call::post(
        format!("/api/workspaces/{}/tasks", fixture.workspace),
        json!({ "description": "Written after the license lapsed" }),
    );
    for (call, permitted, refused) in [
        (&update, &updater, &organization_reader),
        (&task, &writer, &workspace_reader),
    ] {
        let response = call.send(&fixture, &permitted.token).await;
        assert!(
            response.status.is_success(),
            "{}",
            describe(call, &response)
        );
        let response = call.send(&fixture, &refused.token).await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{}",
            describe(call, &response)
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn community_keeps_enforcing_existing_custom_roles() {
    let fixture = fixture().await;
    let writer = workspace_holder(&fixture, &[Permission::WorkspaceWrite]).await;
    let reader = workspace_holder(&fixture, &[]).await;

    let task = Call::post(
        format!("/api/workspaces/{}/tasks", fixture.workspace),
        json!({ "description": "Written on Community by a custom role" }),
    );
    let response = task.send(&fixture, &writer.token).await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "{}",
        describe(&task, &response)
    );
    let response = task.send(&fixture, &reader.token).await;
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        describe(&task, &response)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_unparsable_assignment_keeps_each_routes_own_refusal() {
    let fixture = fixture().await;
    let member = organization_member(&fixture).await;
    let viewer = workspace_viewer(&fixture).await;

    for (path, unknown) in [
        (
            format!("{}/{member}", organization_members_path(&fixture)),
            "Invalid role. Must be one of: member, admin, owner",
        ),
        (
            format!("{}/{viewer}", workspace_members_path(&fixture)),
            "Invalid role. Must be one of: viewer, member, admin, owner",
        ),
    ] {
        for (body, refusal) in [
            (json!({ "role": "boss" }), unknown),
            (
                json!({ "role": "member", "custom_role_id": Uuid::new_v4() }),
                AMBIGUOUS_ASSIGNMENT,
            ),
            (json!({}), AMBIGUOUS_ASSIGNMENT),
        ] {
            let call = Call::patch(path.clone(), body);
            let response = call.send(&fixture, &fixture.owner.token).await;
            assert_eq!(
                response.status,
                StatusCode::BAD_REQUEST,
                "{}",
                describe(&call, &response)
            );
            assert_eq!(
                response.json_value()["error"],
                refusal,
                "{}",
                describe(&call, &response)
            );
        }
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_assignment_answers_and_audits_the_role_it_assigned() {
    let fixture = licensed().await;
    let organization_role = role(
        fixture.pool(),
        fixture.organization,
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let workspace_role = role(
        fixture.pool(),
        fixture.organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let member = organization_member(&fixture).await;
    let viewer = workspace_viewer(&fixture).await;
    let assignments = [
        (
            format!("{}/{member}", organization_members_path(&fixture)),
            member,
            organization_role,
            OrgRole::Member.as_str(),
        ),
        (
            format!("{}/{viewer}", workspace_members_path(&fixture)),
            viewer,
            workspace_role,
            WorkspaceRole::Member.as_str(),
        ),
    ];

    for (path, _, role, rank) in &assignments {
        let call = Call::patch(path.clone(), json!({ "custom_role_id": role }));
        let response = call.send(&fixture, &fixture.owner.token).await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "{}",
            describe(&call, &response)
        );
        let answered = response.json_value();
        assert_eq!(
            answered["custom_role"]["id"],
            role.to_string(),
            "{answered}"
        );
        assert_eq!(answered["role"], *rank, "{answered}");
    }

    let audits = fixture.audits(actions::MEMBER_ROLE_CHANGED).await;
    for (_, member, role, rank) in &assignments {
        let recorded = audits
            .iter()
            .find(|audit| audit.resource_id == Some(*member))
            .and_then(|audit| audit.new_values.clone())
            .unwrap_or_else(|| panic!("the change of {member} is audited"));
        assert_eq!(
            recorded["custom_role"]["id"],
            role.to_string(),
            "{recorded}"
        );
        let name: String = sqlx::query_scalar("SELECT name FROM custom_roles WHERE id = $1")
            .bind(role)
            .fetch_one(fixture.pool())
            .await
            .expect("the role is stored");
        assert_eq!(
            recorded["custom_role"]["name"], name,
            "the audit names the role: {recorded}"
        );
        assert_eq!(recorded["role"], *rank, "{recorded}");
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_added_member_is_answered_and_audited_from_the_seat_the_add_wrote() {
    let fixture = fixture().await;
    let added = fixture.stranger().await;

    let call = Call::post(
        organization_members_path(&fixture),
        json!({ "user_id": added.id, "role": OrgRole::Admin.as_str() }),
    );
    let response = call.send(&fixture, &fixture.owner.token).await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "{}",
        describe(&call, &response)
    );
    let answered = response.json_value();
    assert_eq!(answered["role"], OrgRole::Admin.as_str(), "{answered}");
    assert_eq!(answered["custom_role"], Value::Null, "{answered}");
    assert!(answered["email"].is_string(), "{answered}");

    let audits = fixture.audits(actions::MEMBER_ADDED).await;
    let recorded = audits
        .iter()
        .find(|audit| audit.resource_id == Some(added.id))
        .and_then(|audit| audit.new_values.clone())
        .expect("the add is audited");
    assert_eq!(recorded["role"], OrgRole::Admin.as_str(), "{recorded}");
    assert_eq!(recorded["custom_role"], Value::Null, "{recorded}");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_holder_who_does_not_cover_a_custom_target_is_refused_with_role_exceeds() {
    let fixture = licensed().await;
    let organization_manager = organization_holder(
        &fixture,
        &[Permission::MembersUpdate, Permission::MembersDelete],
    )
    .await;
    let organization_target =
        organization_holder(&fixture, &[Permission::OrganizationUpdate]).await;
    let workspace_manager = workspace_holder(
        &fixture,
        &[
            Permission::WorkspaceMembersUpdate,
            Permission::WorkspaceMembersDelete,
        ],
    )
    .await;
    let workspace_target = workspace_holder(&fixture, &[Permission::WorkspaceUpdate]).await;

    let organization_path = format!(
        "{}/{}",
        organization_members_path(&fixture),
        organization_target.id
    );
    let workspace_path = format!(
        "{}/{}",
        workspace_members_path(&fixture),
        workspace_target.id
    );
    for (call, manager) in [
        (
            Call::patch(
                organization_path.clone(),
                json!({ "role": OrgRole::Member.as_str() }),
            ),
            &organization_manager,
        ),
        (Call::delete(organization_path), &organization_manager),
        (
            Call::patch(
                workspace_path.clone(),
                json!({ "role": WorkspaceRole::Viewer.as_str() }),
            ),
            &workspace_manager,
        ),
        (Call::delete(workspace_path), &workspace_manager),
    ] {
        let response = call.send(&fixture, &manager.token).await;
        assert!(
            refused_with(&response, EXCEEDS),
            "{}",
            describe(&call, &response)
        );
    }

    let membership = organization_members::membership(
        fixture.pool(),
        fixture.organization,
        organization_target.id,
    )
    .await
    .expect("the membership is read")
    .expect("the target stays");
    assert!(membership.custom_role.is_some(), "{membership:?}");
    let membership =
        workspace_members::membership(fixture.pool(), fixture.workspace, workspace_target.id)
            .await
            .expect("the membership is read")
            .expect("the target stays");
    assert!(membership.custom_role.is_some(), "{membership:?}");

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_inviter_never_invites_beyond_its_grant() {
    let fixture = licensed().await;
    let inviter = organization_holder(&fixture, &[Permission::InvitationsCreate]).await;
    let path = format!("/api/organizations/{}/invitations", fixture.organization);
    let refused_email = format!("{}@example.test", Uuid::new_v4());

    let call = Call::post(
        path.clone(),
        json!({ "email": refused_email, "org_role": OrgRole::Admin.as_str() }),
    );
    let response = call.send(&fixture, &inviter.token).await;
    assert!(
        refused_with(&response, EXCEEDS),
        "{}",
        describe(&call, &response)
    );

    let call = Call::post(
        path.clone(),
        json!({
            "email": refused_email,
            "org_role": OrgRole::Member.as_str(),
            "workspace_id": fixture.workspace,
            "workspace_role": WorkspaceRole::Admin.as_str(),
        }),
    );
    let response = call.send(&fixture, &inviter.token).await;
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        describe(&call, &response)
    );
    assert_eq!(
        response.json_value()["error"],
        "Only owners can invite owners or workspace admins",
        "{}",
        describe(&call, &response)
    );

    let invited: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invitations WHERE email = $1")
        .bind(&refused_email)
        .fetch_one(fixture.pool())
        .await
        .expect("the invitations are counted");
    assert_eq!(invited, 0, "a refused invitation is never stored");

    let call = Call::post(
        path,
        json!({
            "email": format!("{}@example.test", Uuid::new_v4()),
            "org_role": OrgRole::Member.as_str(),
        }),
    );
    let response = call.send(&fixture, &inviter.token).await;
    assert!(
        response.status.is_success(),
        "an invitation within the inviter's grant is sent: {}",
        describe(&call, &response)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_holder_inviting_beyond_its_grant_to_a_full_community_server_reads_role_exceeds() {
    let fixture = fixture().await;
    let holder = organization_holder(
        &fixture,
        &[Permission::InvitationsCreate, Permission::MembersCreate],
    )
    .await;
    fixture.member(OrgRole::Member, None).await;
    fixture.state().cap_members();
    let invitations = format!("/api/organizations/{}/invitations", fixture.organization);
    let stranger = format!("{}@example.test", Uuid::new_v4());

    let call = Call::post(
        invitations.clone(),
        json!({ "email": stranger, "org_role": OrgRole::Admin.as_str() }),
    );
    let response = call.send(&fixture, &holder.token).await;
    assert!(
        refused_with(&response, EXCEEDS),
        "the role checks come before the cap: {}",
        describe(&call, &response)
    );

    let call = Call::post(
        invitations,
        json!({ "email": stranger, "org_role": OrgRole::Member.as_str() }),
    );
    let response = call.send(&fixture, &holder.token).await;
    assert!(
        refused_with(&response, MEMBER_CAP_REACHED),
        "{}",
        describe(&call, &response)
    );

    let existing = user(fixture.pool()).await;
    let call = Call::post(
        organization_members_path(&fixture),
        json!({ "user_id": existing, "role": OrgRole::Admin.as_str() }),
    );
    let response = call.send(&fixture, &holder.token).await;
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        describe(&call, &response)
    );
    assert_eq!(
        response.json_value(),
        json!({ "error": "Only owners can grant admin or owner role" }),
        "an admin grant by a non-owner is refused before the cap: {}",
        describe(&call, &response)
    );

    let invited: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM invitations WHERE organization_id = $1")
            .bind(fixture.organization)
            .fetch_one(fixture.pool())
            .await
            .expect("the invitations are counted");
    assert_eq!(invited, 0, "a refused invitation is never stored");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_removed_member_is_told_the_membership_is_inactive() {
    let fixture = fixture().await;
    let removed = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Viewer))
        .await;
    organization_members::remove_member(fixture.pool(), fixture.organization, removed.id)
        .await
        .expect("the organization member is removed");
    workspace_members::remove_member(fixture.pool(), fixture.workspace, removed.id)
        .await
        .expect("the workspace member is removed");
    let stranger = fixture.stranger().await;

    for (call, inactive, absent) in [
        (
            Call::get(format!("/api/organizations/{}", fixture.organization)),
            "Membership inactive",
            Permission::OrganizationRead.refusal(),
        ),
        (
            Call::get(format!("/api/workspaces/{}", fixture.workspace)),
            "Workspace membership is inactive",
            Permission::WorkspaceRead.refusal(),
        ),
    ] {
        for (account, refusal) in [(&removed, inactive.to_string()), (&stranger, absent)] {
            let response = call.send(&fixture, &account.token).await;
            assert_eq!(
                response.status,
                StatusCode::FORBIDDEN,
                "{}",
                describe(&call, &response)
            );
            assert_eq!(
                response.json_value()["error"],
                refusal,
                "{}",
                describe(&call, &response)
            );
        }
    }

    fixture.cleanup().await;
}

/// Sends `call` as `account` while a transaction setting the account's own
/// role in `scope` to `role` is open, and lets it land once the route waits
/// for that row: the guard read the caller before the demotion, the member
/// change reads them after it.
async fn behind_a_demotion(
    fixture: &Fixture,
    call: &Call,
    account: &Account,
    statement: &'static str,
    scope: Uuid,
    role: &'static str,
) -> TestResponse {
    let mut demotion = fixture.pool().begin().await.expect("a transaction");
    sqlx::query(statement)
        .bind(scope)
        .bind(account.id)
        .bind(role)
        .execute(&mut *demotion)
        .await
        .expect("the caller is demoted");
    let (response, ()) = tokio::join!(call.send(fixture, &account.token), async {
        assert!(
            waiting(fixture.pool(), 1).await,
            "the route waits for the caller's row"
        );
        demotion.commit().await.expect("the demotion lands");
    });
    response
}

#[tokio::test]
async fn an_organization_admin_demoted_mid_change_reads_the_guards_refusal() {
    let fixture = fixture().await;
    let admin = fixture.member(OrgRole::Admin, None).await;
    let target = organization_member(&fixture).await;

    let call = Call::patch(
        format!("{}/{target}", organization_members_path(&fixture)),
        json!({ "role": OrgRole::Member.as_str() }),
    );
    let response = behind_a_demotion(
        &fixture,
        &call,
        &admin,
        "UPDATE organization_members SET role = $3 WHERE organization_id = $1 AND user_id = $2",
        fixture.organization,
        OrgRole::Member.as_str(),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        describe(&call, &response)
    );
    assert_eq!(
        response.json_value()["error"],
        Permission::MembersUpdate.refusal(),
        "{}",
        describe(&call, &response)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_admin_demoted_mid_removal_reads_the_guards_refusal() {
    let fixture = fixture().await;
    let admin = fixture
        .member(OrgRole::Member, Some(WorkspaceRole::Admin))
        .await;
    let target = workspace_viewer(&fixture).await;

    let call = Call::delete(format!("{}/{target}", workspace_members_path(&fixture)));
    let response = behind_a_demotion(
        &fixture,
        &call,
        &admin,
        "UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2",
        fixture.workspace,
        WorkspaceRole::Member.as_str(),
    )
    .await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        describe(&call, &response)
    );
    assert_eq!(
        response.json_value()["error"],
        Permission::WorkspaceMembersDelete.refusal(),
        "{}",
        describe(&call, &response)
    );
    assert!(
        workspace_members::membership(fixture.pool(), fixture.workspace, target)
            .await
            .expect("the membership is read")
            .is_some(),
        "the target keeps their seat"
    );

    fixture.cleanup().await;
}
