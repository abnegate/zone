use http_body_util::BodyExt;
use serde_json::{Value, json};

use super::*;
use crate::auth::Scope;
use crate::db::custom_roles::Summary;

fn member(role: WorkspaceRole, custom_role: Option<Summary>, grant: Grant) -> WorkspaceMember {
    WorkspaceMember {
        workspace_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        email: "holder@example.com".to_string(),
        role,
        custom_role,
        grant,
    }
}

fn seated(role: WorkspaceRole) -> WorkspaceMember {
    member(role, None, Grant::workspace(role))
}

fn holder(permissions: &[Permission]) -> WorkspaceMember {
    let summary = Summary {
        id: Uuid::new_v4(),
        name: "Holders".to_string(),
    };
    member(WorkspaceRole::Viewer, Some(summary), composed(permissions))
}

fn composed(permissions: &[Permission]) -> Grant {
    let names: Vec<String> = permissions
        .iter()
        .map(|permission| permission.name().to_string())
        .collect();
    Grant::compose(Scope::Workspace, &names).expect("a composable workspace grant")
}

async fn refused(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("the refusal body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&body).expect("the refusal is JSON"),
    )
}

fn workspace() -> WorkspaceResponse {
    WorkspaceResponse {
        id: Uuid::new_v4(),
        organization_id: Uuid::new_v4(),
        name: "Workspace".to_string(),
        slug: "workspace".to_string(),
        description: None,
        is_active: true,
        membership: None,
        timestamps: Timestamps::default(),
    }
}

#[test]
fn a_built_in_membership_reports_its_role_and_permissions() {
    let membership =
        serde_json::to_value(built_in(WorkspaceRole::Admin)).expect("a membership serializes");

    assert_eq!(membership["role"], json!("admin"));
    assert_eq!(membership["custom_role"], Value::Null);
    assert_eq!(
        membership["permissions"],
        json!(Grant::workspace(WorkspaceRole::Admin).names()),
    );
}

#[test]
fn a_custom_membership_reports_the_rank_its_grant_covers() {
    let summary = Summary {
        id: Uuid::new_v4(),
        name: "Writers".to_string(),
    };
    let grant = composed(&[Permission::WorkspaceWrite]);
    let membership =
        serde_json::to_value(MembershipResponse::workspace(Some(summary.clone()), grant))
            .expect("a membership serializes");

    assert_eq!(membership["role"], json!("member"));
    assert_eq!(
        membership["custom_role"],
        json!({ "id": summary.id, "name": "Writers" }),
    );
    assert_eq!(
        membership["permissions"],
        json!(["workspace:read", "workspace:write"]),
    );
}

#[test]
fn a_custom_membership_short_of_member_reports_viewer() {
    let grant = composed(&[Permission::WorkspaceMembersCreate]);
    let membership = serde_json::to_value(MembershipResponse::workspace(None, grant))
        .expect("a membership serializes");

    assert_eq!(membership["role"], json!("viewer"));
}

#[test]
fn a_workspace_without_a_membership_carries_none() {
    let serialized = serde_json::to_value(workspace()).expect("a workspace serializes");

    for key in ["role", "custom_role", "permissions"] {
        assert!(
            serialized.get(key).is_none(),
            "a workspace row carried {key}: {serialized}"
        );
    }
}

#[test]
fn a_held_workspace_carries_the_membership_beside_its_fields() {
    let response = workspace().held(built_in(WorkspaceRole::Viewer));
    let serialized = serde_json::to_value(&response).expect("a workspace serializes");

    assert_eq!(serialized["role"], json!("viewer"));
    assert_eq!(serialized["custom_role"], Value::Null);
    assert_eq!(serialized["permissions"], json!(["workspace:read"]));
}

#[test]
fn an_admin_cannot_seat_an_admin() {
    assert!(matches!(
        seatable(&seated(WorkspaceRole::Admin), WorkspaceRole::Admin),
        Err(Refusal::Promotion)
    ));
}

#[test]
fn an_owner_can_seat_every_role() {
    let owner = seated(WorkspaceRole::Owner);

    for role in [
        WorkspaceRole::Viewer,
        WorkspaceRole::Member,
        WorkspaceRole::Admin,
        WorkspaceRole::Owner,
    ] {
        assert!(
            seatable(&owner, role).is_ok(),
            "an owner could not seat {role:?}"
        );
    }
}

#[test]
fn a_custom_adder_cannot_seat_a_role_beyond_its_grant() {
    let adder = holder(&[Permission::WorkspaceMembersCreate]);

    assert!(matches!(
        seatable(&adder, WorkspaceRole::Member),
        Err(Refusal::Exceeds)
    ));
    assert!(seatable(&adder, WorkspaceRole::Viewer).is_ok());
}

#[test]
fn the_promotion_refusal_comes_before_the_coverage_refusal() {
    let adder = holder(&[Permission::WorkspaceMembersCreate]);

    assert!(matches!(
        seatable(&adder, WorkspaceRole::Admin),
        Err(Refusal::Promotion)
    ));
}

#[test]
fn only_the_administrators_permission_promotes() {
    let admin = seated(WorkspaceRole::Admin);
    let owner = seated(WorkspaceRole::Owner);

    for role in [WorkspaceRole::Admin, WorkspaceRole::Owner] {
        assert!(matches!(promotable(&admin, role), Err(Refusal::Promotion)));
        assert!(promotable(&owner, role).is_ok());
    }
    assert!(promotable(&admin, WorkspaceRole::Member).is_ok());
}

#[tokio::test]
async fn a_refused_promotion_carries_no_code() {
    let (status, body) = refused(Refusal::Promotion.into_response()).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!(PROMOTION_REFUSED));
    assert!(body.get("code").is_none(), "{body}");
}

#[tokio::test]
async fn a_member_who_vanished_is_named_by_how_they_were_seated() {
    for (seating, message) in [
        (
            Seating::Reactivated,
            "Failed to retrieve reactivated member",
        ),
        (Seating::Created, "Failed to retrieve created member"),
    ] {
        let (status, body) = refused(Refusal::Vanished(seating).into_response()).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], json!(message), "{seating:?}");
    }
}
