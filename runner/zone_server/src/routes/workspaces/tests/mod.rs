use http_body_util::BodyExt;
use serde_json::{Value, json};

use super::*;
use crate::auth::Scope;

fn holder(grant: Grant) -> WorkspaceMember {
    WorkspaceMember {
        workspace_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        email: "holder@example.com".to_string(),
        role: WorkspaceRole::Viewer,
        custom_role: None,
        grant,
    }
}

fn composed(names: &[Permission]) -> Grant {
    let names: Vec<String> = names
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

#[test]
fn a_built_in_standing_reports_its_role_and_permissions() {
    let standing = serde_json::to_value(Standing::built_in(WorkspaceRole::Admin))
        .expect("a standing serializes");

    assert_eq!(standing["role"], json!("admin"));
    assert_eq!(standing["custom_role"], Value::Null);
    assert_eq!(
        standing["permissions"],
        json!(Grant::workspace(WorkspaceRole::Admin).names()),
    );
}

#[test]
fn a_custom_standing_reports_the_rank_its_grant_covers() {
    let summary = Summary {
        id: Uuid::new_v4(),
        name: "Writers".to_string(),
    };
    let grant = composed(&[Permission::WorkspaceWrite]);
    let standing = serde_json::to_value(Standing::new(Some(summary.clone()), grant))
        .expect("a standing serializes");

    assert_eq!(standing["role"], json!("member"));
    assert_eq!(
        standing["custom_role"],
        json!({ "id": summary.id, "name": "Writers" }),
    );
    assert_eq!(
        standing["permissions"],
        json!(["workspace:read", "workspace:write"]),
    );
}

#[test]
fn a_custom_standing_short_of_member_reports_viewer() {
    let grant = composed(&[Permission::WorkspaceMembersCreate]);
    let standing = serde_json::to_value(Standing::new(None, grant)).expect("a standing serializes");

    assert_eq!(standing["role"], json!("viewer"));
}

#[test]
fn a_workspace_without_a_membership_carries_no_standing() {
    let response = WorkspaceResponse {
        id: Uuid::new_v4(),
        organization_id: Uuid::new_v4(),
        name: "Workspace".to_string(),
        slug: "workspace".to_string(),
        description: None,
        is_active: true,
        standing: None,
        timestamps: Timestamps::default(),
    };
    let serialized = serde_json::to_value(&response).expect("a workspace serializes");

    for key in ["role", "custom_role", "permissions"] {
        assert!(
            serialized.get(key).is_none(),
            "a workspace row carried {key}: {serialized}"
        );
    }
}

#[test]
fn a_held_workspace_carries_the_standing_beside_its_fields() {
    let response = WorkspaceResponse {
        id: Uuid::new_v4(),
        organization_id: Uuid::new_v4(),
        name: "Workspace".to_string(),
        slug: "workspace".to_string(),
        description: None,
        is_active: true,
        standing: None,
        timestamps: Timestamps::default(),
    }
    .held(Standing::built_in(WorkspaceRole::Viewer));
    let serialized = serde_json::to_value(&response).expect("a workspace serializes");

    assert_eq!(serialized["role"], json!("viewer"));
    assert_eq!(serialized["custom_role"], Value::Null);
    assert_eq!(serialized["permissions"], json!(["workspace:read"]));
}

#[test]
fn an_admin_cannot_seat_an_admin() {
    let admin = holder(Grant::workspace(WorkspaceRole::Admin));

    assert!(matches!(
        seatable(&admin, WorkspaceRole::Admin),
        Err(Refusal::Promotion)
    ));
}

#[test]
fn an_owner_can_seat_every_role() {
    let owner = holder(Grant::workspace(WorkspaceRole::Owner));

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
    let adder = holder(composed(&[Permission::WorkspaceMembersCreate]));

    assert!(matches!(
        seatable(&adder, WorkspaceRole::Member),
        Err(Refusal::Exceeds)
    ));
    assert!(seatable(&adder, WorkspaceRole::Viewer).is_ok());
}

#[test]
fn the_promotion_refusal_comes_before_the_coverage_refusal() {
    let adder = holder(composed(&[Permission::WorkspaceMembersCreate]));

    assert!(matches!(
        seatable(&adder, WorkspaceRole::Admin),
        Err(Refusal::Promotion)
    ));
}

#[test]
fn only_the_administrators_permission_promotes() {
    let admin = holder(Grant::workspace(WorkspaceRole::Admin));
    let owner = holder(Grant::workspace(WorkspaceRole::Owner));

    for role in [WorkspaceRole::Admin, WorkspaceRole::Owner] {
        assert!(matches!(promotable(&admin, role), Err(Refusal::Promotion)));
        assert!(promotable(&owner, role).is_ok());
    }
    assert!(promotable(&admin, WorkspaceRole::Member).is_ok());
}

#[tokio::test]
async fn an_exceeding_role_is_refused_with_the_exceeds_code() {
    let (status, body) = refused(Refusal::Exceeds.into_response()).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], json!(custom_roles::EXCEEDS));
}

#[tokio::test]
async fn a_refused_promotion_carries_no_code() {
    let (status, body) = refused(Refusal::Promotion.into_response()).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!(PROMOTION_REFUSED));
    assert!(body.get("code").is_none(), "{body}");
}

#[tokio::test]
async fn a_seated_member_is_a_conflict() {
    let (status, _) = refused(Refusal::Seated.into_response()).await;

    assert_eq!(status, StatusCode::CONFLICT);
}
