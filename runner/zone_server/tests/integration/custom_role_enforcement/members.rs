//! Custom roles through the member routes: adding and reactivating members,
//! the rank member lists report, escalation, and what the license gates.

use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use zone_enterprise::LICENSE_REQUIRED;
use zone_server::auth::{Permission, Scope};
use zone_server::db::custom_roles::EXCEEDS;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use super::{
    Call, composed, describe, holding_organization, holding_workspace, licensed,
    organization_holder, refused_with, role, seat_organization, seat_workspace, user,
    workspace_holder,
};
use crate::common::policy::Fixture;
use crate::common::sso::lapsed;

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
        fixture.owner.id,
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

    let reading = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
        Scope::Workspace,
        &[],
    )
    .await;
    let writing_permissions = [Permission::WorkspaceWrite, Permission::ProjectSyncsUpdate];
    let writing = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
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
        fixture.owner.id,
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let workspace_updater = workspace_holder(&fixture, &[Permission::WorkspaceMembersUpdate]).await;
    let writers = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
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
        fixture.owner.id,
        Scope::Organization,
        &[],
    )
    .await;
    let workspace_role = role(
        fixture.pool(),
        fixture.organization,
        fixture.owner.id,
        Scope::Workspace,
        &[],
    )
    .await;
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
