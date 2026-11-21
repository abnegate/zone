//! What only custom roles reach in the guarded role changes: assignments that
//! exceed the caller, roles that are missing for the membership, custom
//! holders reaching for admins and owners, and the floors under a custom
//! assignment.

use sqlx::PgPool;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::Assignment;
use zone_server::db::organization_members::{self, OrgRole, RoleChange};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use super::{
    assign_organization, assign_workspace, composable, composed, fixture, organization, role,
    seat_organization, seat_workspace, user, workspace,
};

async fn organization_grant(pool: &PgPool, organization: Uuid, member: Uuid) -> Grant {
    organization_members::membership(pool, organization, member)
        .await
        .expect("the membership is read")
        .expect("an active member")
        .grant
}

async fn workspace_grant(pool: &PgPool, workspace: Uuid, member: Uuid) -> Grant {
    workspace_members::membership(pool, workspace, member)
        .await
        .expect("the membership is read")
        .expect("an active member")
        .grant
}

/// The grant of someone holding a custom organization role with `permissions`.
async fn organization_caller(
    pool: &PgPool,
    organization: Uuid,
    creator: Uuid,
    permissions: &[Permission],
) -> Grant {
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Member).await;
    let held = role(
        pool,
        organization,
        creator,
        Scope::Organization,
        permissions,
    )
    .await;
    assign_organization(pool, organization, caller, held).await;
    organization_grant(pool, organization, caller).await
}

/// The grant of someone holding a custom workspace role with `permissions`.
async fn workspace_caller(
    pool: &PgPool,
    organization: Uuid,
    workspace: Uuid,
    creator: Uuid,
    permissions: &[Permission],
) -> Grant {
    let caller = user(pool).await;
    seat_workspace(pool, workspace, caller, WorkspaceRole::Viewer).await;
    let held = role(pool, organization, creator, Scope::Workspace, permissions).await;
    assign_workspace(pool, workspace, caller, held).await;
    workspace_grant(pool, workspace, caller).await
}

async fn change_organization(
    pool: &PgPool,
    organization: Uuid,
    member: Uuid,
    assignment: Assignment<OrgRole>,
    caller: &Grant,
) -> RoleChange {
    organization_members::change_role(pool, organization, member, assignment, caller)
        .await
        .expect("the change is answered")
}

async fn change_workspace(
    pool: &PgPool,
    workspace: Uuid,
    member: Uuid,
    assignment: Assignment<WorkspaceRole>,
    caller: &Grant,
) -> workspace_members::RoleChange {
    workspace_members::change_role(pool, workspace, member, assignment, caller)
        .await
        .expect("the change is answered")
}

#[tokio::test]
async fn an_organization_assignment_never_exceeds_the_caller() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let member = user(pool).await;
    seat_organization(pool, organization, member, OrgRole::Member).await;
    let caller =
        organization_caller(pool, organization, creator, &[Permission::MembersUpdate]).await;
    let auditors = role(
        pool,
        organization,
        creator,
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let updaters = role(
        pool,
        organization,
        creator,
        Scope::Organization,
        &[Permission::MembersUpdate],
    )
    .await;

    let exceeding = change_organization(
        pool,
        organization,
        member,
        Assignment::Custom(auditors),
        &caller,
    )
    .await;
    assert!(matches!(exceeding, RoleChange::Exceeds), "{exceeding:?}");
    let promoting = change_organization(
        pool,
        organization,
        member,
        Assignment::BuiltIn(OrgRole::Admin),
        &caller,
    )
    .await;
    assert!(matches!(promoting, RoleChange::Exceeds), "{promoting:?}");
    assert!(
        organization_grant(pool, organization, member).await
            == Grant::organization(OrgRole::Member)
    );

    let within = change_organization(
        pool,
        organization,
        member,
        Assignment::Custom(updaters),
        &caller,
    )
    .await;
    assert!(matches!(within, RoleChange::Applied(_)), "{within:?}");
    assert!(organization_grant(pool, organization, member).await == caller);

    let back = change_organization(
        pool,
        organization,
        member,
        Assignment::BuiltIn(OrgRole::Member),
        &caller,
    )
    .await;
    assert!(matches!(back, RoleChange::Applied(_)), "{back:?}");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_assignment_never_exceeds_the_caller() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let workspace = workspace(pool, organization).await;
    let member = user(pool).await;
    seat_workspace(pool, workspace, member, WorkspaceRole::Viewer).await;
    let caller = workspace_caller(
        pool,
        organization,
        workspace,
        creator,
        &[Permission::WorkspaceMembersUpdate],
    )
    .await;
    let writers = role(
        pool,
        organization,
        creator,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let readers = role(pool, organization, creator, Scope::Workspace, &[]).await;

    let exceeding = change_workspace(
        pool,
        workspace,
        member,
        Assignment::Custom(writers),
        &caller,
    )
    .await;
    assert!(
        matches!(exceeding, workspace_members::RoleChange::Exceeds),
        "{exceeding:?}"
    );
    let promoting = change_workspace(
        pool,
        workspace,
        member,
        Assignment::BuiltIn(WorkspaceRole::Member),
        &caller,
    )
    .await;
    assert!(
        matches!(promoting, workspace_members::RoleChange::Exceeds),
        "{promoting:?}"
    );
    let seating = change_workspace(
        pool,
        workspace,
        member,
        Assignment::BuiltIn(WorkspaceRole::Admin),
        &caller,
    )
    .await;
    assert!(
        matches!(seating, workspace_members::RoleChange::Forbidden),
        "seating an admin needs the administrators permission: {seating:?}"
    );
    assert!(
        workspace_grant(pool, workspace, member).await == Grant::workspace(WorkspaceRole::Viewer)
    );

    let within = change_workspace(
        pool,
        workspace,
        member,
        Assignment::Custom(readers),
        &caller,
    )
    .await;
    assert!(
        matches!(within, workspace_members::RoleChange::Applied(_)),
        "{within:?}"
    );
    assert!(workspace_grant(pool, workspace, member).await == composed(Scope::Workspace, &[]));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_role_of_another_organization_or_scope_is_missing() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let elsewhere = self::organization(pool).await;
    let workspace = workspace(pool, organization).await;
    let member = user(pool).await;
    seat_organization(pool, organization, member, OrgRole::Member).await;
    seat_workspace(pool, workspace, member, WorkspaceRole::Viewer).await;
    let owners = Grant::organization(OrgRole::Owner);
    let workspace_owners = Grant::workspace(WorkspaceRole::Owner);

    for missing in [
        role(pool, elsewhere, creator, Scope::Organization, &[]).await,
        role(pool, organization, creator, Scope::Workspace, &[]).await,
        Uuid::new_v4(),
    ] {
        let outcome = change_organization(
            pool,
            organization,
            member,
            Assignment::Custom(missing),
            &owners,
        )
        .await;
        assert!(matches!(outcome, RoleChange::RoleMissing), "{outcome:?}");
    }
    for missing in [
        role(pool, elsewhere, creator, Scope::Workspace, &[]).await,
        role(pool, organization, creator, Scope::Organization, &[]).await,
        Uuid::new_v4(),
    ] {
        let outcome = change_workspace(
            pool,
            workspace,
            member,
            Assignment::Custom(missing),
            &workspace_owners,
        )
        .await;
        assert!(
            matches!(outcome, workspace_members::RoleChange::RoleMissing),
            "{outcome:?}"
        );
    }

    assert!(
        organization_grant(pool, organization, member).await
            == Grant::organization(OrgRole::Member)
    );
    assert!(
        workspace_grant(pool, workspace, member).await == Grant::workspace(WorkspaceRole::Viewer)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_holder_never_touches_an_organization_admin_or_owner() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let caller = organization_caller(
        pool,
        organization,
        creator,
        &composable(Scope::Organization),
    )
    .await;
    let fellow = role(pool, organization, creator, Scope::Organization, &[]).await;

    for held in [OrgRole::Admin, OrgRole::Owner] {
        let target = user(pool).await;
        seat_organization(pool, organization, target, held).await;
        for assignment in [
            Assignment::BuiltIn(OrgRole::Member),
            Assignment::BuiltIn(OrgRole::Admin),
            Assignment::Custom(fellow),
        ] {
            let outcome =
                change_organization(pool, organization, target, assignment, &caller).await;
            assert!(
                matches!(outcome, RoleChange::Forbidden),
                "{held:?} given {assignment:?}: {outcome:?}"
            );
        }
        let removal = organization_members::remove_guarded(pool, organization, target, &caller)
            .await
            .expect("the removal is answered");
        assert!(
            matches!(removal, organization_members::Removal::Forbidden),
            "{held:?}: {removal:?}"
        );
        assert!(organization_grant(pool, organization, target).await == Grant::organization(held));
    }

    let target = user(pool).await;
    seat_organization(pool, organization, target, OrgRole::Member).await;
    let seating = change_organization(
        pool,
        organization,
        target,
        Assignment::BuiltIn(OrgRole::Owner),
        &caller,
    )
    .await;
    assert!(matches!(seating, RoleChange::Forbidden), "{seating:?}");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_holder_never_touches_a_workspace_admin_or_owner() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let workspace = workspace(pool, organization).await;
    let caller = workspace_caller(
        pool,
        organization,
        workspace,
        creator,
        &composable(Scope::Workspace),
    )
    .await;
    let fellow = role(pool, organization, creator, Scope::Workspace, &[]).await;

    for held in [WorkspaceRole::Admin, WorkspaceRole::Owner] {
        let target = user(pool).await;
        seat_workspace(pool, workspace, target, held).await;
        for assignment in [
            Assignment::BuiltIn(WorkspaceRole::Viewer),
            Assignment::BuiltIn(WorkspaceRole::Member),
            Assignment::Custom(fellow),
        ] {
            let outcome = change_workspace(pool, workspace, target, assignment, &caller).await;
            assert!(
                matches!(outcome, workspace_members::RoleChange::Forbidden),
                "{held:?} given {assignment:?}: {outcome:?}"
            );
        }
        let removal = workspace_members::remove_guarded(pool, workspace, target, &caller)
            .await
            .expect("the removal is answered");
        assert!(
            matches!(removal, workspace_members::Removal::Forbidden),
            "{held:?}: {removal:?}"
        );
        assert!(workspace_grant(pool, workspace, target).await == Grant::workspace(held));
    }

    let target = user(pool).await;
    seat_workspace(pool, workspace, target, WorkspaceRole::Viewer).await;
    for seated in [WorkspaceRole::Admin, WorkspaceRole::Owner] {
        let outcome = change_workspace(
            pool,
            workspace,
            target,
            Assignment::BuiltIn(seated),
            &caller,
        )
        .await;
        assert!(
            matches!(outcome, workspace_members::RoleChange::Forbidden),
            "{seated:?}: {outcome:?}"
        );
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_last_organization_owner_is_never_given_a_custom_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let owner = user(pool).await;
    seat_organization(pool, organization, owner, OrgRole::Owner).await;
    let owners = Grant::organization(OrgRole::Owner);
    let everything = role(
        pool,
        organization,
        creator,
        Scope::Organization,
        &composable(Scope::Organization),
    )
    .await;

    let alone = change_organization(
        pool,
        organization,
        owner,
        Assignment::Custom(everything),
        &owners,
    )
    .await;
    assert!(matches!(alone, RoleChange::LastOwner), "{alone:?}");
    assert!(organization_grant(pool, organization, owner).await == owners);

    seat_organization(pool, organization, user(pool).await, OrgRole::Owner).await;
    let accompanied = change_organization(
        pool,
        organization,
        owner,
        Assignment::Custom(everything),
        &owners,
    )
    .await;
    assert!(
        matches!(accompanied, RoleChange::Applied(_)),
        "{accompanied:?}"
    );
    let membership = organization_members::membership(pool, organization, owner)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(
        membership.role,
        OrgRole::Member,
        "a custom holder is stored as a member"
    );
    assert_eq!(
        membership.custom_role.map(|summary| summary.id),
        Some(everything)
    );
    assert!(membership.grant == composed(Scope::Organization, &composable(Scope::Organization)));

    fixture.cleanup().await;
}

#[tokio::test]
async fn the_last_workspace_owner_or_admin_is_never_given_a_custom_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let creator = fixture.owner.id;
    let organization = organization(pool).await;
    let owners = Grant::workspace(WorkspaceRole::Owner);
    let everything = role(
        pool,
        organization,
        creator,
        Scope::Workspace,
        &composable(Scope::Workspace),
    )
    .await;

    let owned = workspace(pool, organization).await;
    let owner = user(pool).await;
    seat_workspace(pool, owned, owner, WorkspaceRole::Owner).await;
    seat_workspace(pool, owned, user(pool).await, WorkspaceRole::Admin).await;
    let outcome =
        change_workspace(pool, owned, owner, Assignment::Custom(everything), &owners).await;
    assert!(
        matches!(outcome, workspace_members::RoleChange::LastOwner),
        "{outcome:?}"
    );

    let administered = workspace(pool, organization).await;
    let admin = user(pool).await;
    seat_workspace(pool, administered, admin, WorkspaceRole::Admin).await;
    let outcome = change_workspace(
        pool,
        administered,
        admin,
        Assignment::Custom(everything),
        &owners,
    )
    .await;
    assert!(
        matches!(outcome, workspace_members::RoleChange::LastAdmin),
        "{outcome:?}"
    );

    seat_workspace(pool, administered, user(pool).await, WorkspaceRole::Owner).await;
    let outcome = change_workspace(
        pool,
        administered,
        admin,
        Assignment::Custom(everything),
        &owners,
    )
    .await;
    assert!(
        matches!(outcome, workspace_members::RoleChange::Applied(_)),
        "{outcome:?}"
    );
    let membership = workspace_members::membership(pool, administered, admin)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(
        membership.role,
        WorkspaceRole::Viewer,
        "a custom holder is stored as a viewer"
    );
    assert_eq!(
        membership.custom_role.map(|summary| summary.id),
        Some(everything)
    );

    fixture.cleanup().await;
}
