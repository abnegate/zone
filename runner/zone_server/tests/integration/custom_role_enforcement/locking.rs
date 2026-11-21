//! An assignment racing an edit of the role it assigns, or a demotion of the
//! member who makes it.
//!
//! `change_role` share-locks the role it assigns and reads its permissions
//! under that lock; `custom_roles::update` takes the role for update before it
//! counts holders and rewrites permissions. Whichever goes first, the other
//! sees what it left: an assignment is checked against the permissions the
//! holder ends up with, and an edit counts every holder an assignment added.
//!
//! An edit share-locks its editor's membership before the role, so a demotion
//! in flight lands first and the edit is judged by what the editor holds after.
//! `change_role` and `remove_guarded` also read their caller's membership
//! under a share lock, so a demotion or removal of the caller already in
//! flight lands first and the change is judged by what the caller holds
//! afterwards: a caller who no longer holds the permission that admitted the
//! request is revoked.
//!
//! Every side locks member rows before role rows, so a change that waits for
//! its caller's role never holds that role while an edit by its target waits
//! for it.

use std::sync::Arc;
use std::time::Duration;

use sqlx::{Encode, PgPool, Postgres, Type};
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::{self, Assignment, Change, Deletion, Update};
use zone_server::db::organization_members::{self, OrgRole, RoleChange};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use super::{
    assign_organization, assign_workspace, composed, fixture, organization, role,
    seat_organization, seat_workspace, user, workspace,
};
use crate::common::eventually::eventually;

const ITERATIONS: usize = 8;
const DEADLOCK: Duration = Duration::from_secs(15);

const WIDE: [Permission; 2] = [Permission::MembersCreate, Permission::MembersUpdate];
const NARROW: [Permission; 1] = [Permission::MembersUpdate];

/// Waits until `count` backends of this database wait on a lock.
pub(super) async fn waiting(pool: &PgPool, count: i64) -> bool {
    eventually(move || async move {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("the activity is read");
        waiting >= count
    })
    .await
}

async fn finished<T>(task: JoinHandle<T>) -> T {
    tokio::time::timeout(DEADLOCK, task)
        .await
        .expect("neither side waits on the other forever")
        .expect("the task completes")
}

fn assign(
    pool: &PgPool,
    organization: Uuid,
    member: Uuid,
    role: Uuid,
    caller: Uuid,
) -> JoinHandle<RoleChange> {
    let pool = pool.clone();
    tokio::spawn(async move {
        organization_members::change_role(
            &pool,
            organization,
            member,
            Assignment::Custom(role),
            caller,
        )
        .await
        .expect("the assignment is answered")
    })
}

fn narrow(pool: &PgPool, organization: Uuid, role: Uuid, editor: Uuid) -> JoinHandle<Update> {
    let pool = pool.clone();
    tokio::spawn(async move {
        let narrowed = composed(Scope::Organization, &NARROW);
        let change = Change {
            grant: Some(&narrowed),
            ..Change::default()
        };
        custom_roles::update(&pool, organization, role, &change, editor)
            .await
            .expect("the edit is answered")
    })
}

/// Someone seated in `organization` with the built-in `role`, to edit its custom roles.
async fn editor(pool: &PgPool, organization: Uuid, role: OrgRole) -> Uuid {
    let editor = user(pool).await;
    seat_organization(pool, organization, editor, role).await;
    editor
}

async fn held(pool: &PgPool, organization: Uuid, member: Uuid) -> Grant {
    organization_members::membership(pool, organization, member)
        .await
        .expect("the membership is read")
        .expect("an active member")
        .grant
}

/// The people and roles an assignment race needs.
struct Arranged {
    organization: Uuid,
    member: Uuid,
    role: Uuid,
    caller: Uuid,
}

/// An organization with a member, a wide role nobody holds yet, and a caller
/// holding only the narrow permissions.
async fn arranged(pool: &PgPool, creator: Uuid) -> Arranged {
    let organization = organization(pool).await;
    let owner = user(pool).await;
    seat_organization(pool, organization, owner, OrgRole::Owner).await;
    let member = user(pool).await;
    seat_organization(pool, organization, member, OrgRole::Member).await;
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Member).await;
    let narrow = role(pool, organization, creator, Scope::Organization, &NARROW).await;
    assign_organization(pool, organization, owner, caller, narrow).await;
    let role = role(pool, organization, creator, Scope::Organization, &WIDE).await;
    Arranged {
        organization,
        member,
        role,
        caller,
    }
}

#[tokio::test]
async fn an_assignment_held_at_the_member_lock_is_checked_against_the_narrowed_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let arranged = arranged(pool, fixture.owner.id).await;
    let narrowed = composed(Scope::Organization, &NARROW);

    let mut blocker = pool.begin().await.expect("a transaction");
    sqlx::query(
        "SELECT 1 FROM organization_members WHERE organization_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(arranged.organization)
    .bind(arranged.member)
    .execute(&mut *blocker)
    .await
    .expect("the member row is held");
    let assignment = assign(
        pool,
        arranged.organization,
        arranged.member,
        arranged.role,
        arranged.caller,
    );
    assert!(
        waiting(pool, 1).await,
        "the assignment waits for the member row"
    );

    let editor = editor(pool, arranged.organization, OrgRole::Admin).await;
    let edit = finished(narrow(pool, arranged.organization, arranged.role, editor)).await;
    assert!(
        matches!(edit, Update::Saved { .. }),
        "nobody holds the role yet: {edit:?}"
    );
    blocker.commit().await.expect("the member row is let go");

    let outcome = finished(assignment).await;
    assert!(
        matches!(outcome, RoleChange::Applied(_)),
        "a caller holding the narrowed permissions assigns the narrowed role: {outcome:?}"
    );
    assert!(held(pool, arranged.organization, arranged.member).await == narrowed);

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_assignment_behind_an_edit_in_flight_reads_what_the_edit_saved() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let arranged = arranged(pool, fixture.owner.id).await;
    let narrowed = composed(Scope::Organization, &NARROW);

    let mut blocker = pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM custom_role_permissions WHERE role_id = $1 FOR UPDATE")
        .bind(arranged.role)
        .execute(&mut *blocker)
        .await
        .expect("the stored permissions are held");
    let editor = editor(pool, arranged.organization, OrgRole::Admin).await;
    let edit = narrow(pool, arranged.organization, arranged.role, editor);
    assert!(
        waiting(pool, 1).await,
        "the edit holds the role and waits to rewrite it"
    );
    let assignment = assign(
        pool,
        arranged.organization,
        arranged.member,
        arranged.role,
        arranged.caller,
    );
    assert!(
        waiting(pool, 2).await,
        "the assignment waits for the edit's lock on the role"
    );
    blocker
        .commit()
        .await
        .expect("the stored permissions are let go");

    let edit = finished(edit).await;
    assert!(matches!(edit, Update::Saved { .. }), "{edit:?}");
    let outcome = finished(assignment).await;
    assert!(
        matches!(outcome, RoleChange::Applied(_)),
        "the assignment read the narrowed permissions, not the wide ones it queued behind: {outcome:?}"
    );
    assert!(held(pool, arranged.organization, arranged.member).await == narrowed);

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_assignment_racing_a_narrowing_is_checked_against_what_the_holder_gets() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let narrowed = composed(Scope::Organization, &NARROW);

    for iteration in 0..ITERATIONS {
        let arranged = arranged(pool, fixture.owner.id).await;
        let editor = editor(pool, arranged.organization, OrgRole::Admin).await;
        let start = Arc::new(Barrier::new(2));

        let assignment = {
            let (pool, start) = (pool.clone(), start.clone());
            let (organization, member, role, caller) = (
                arranged.organization,
                arranged.member,
                arranged.role,
                arranged.caller,
            );
            tokio::spawn(async move {
                start.wait().await;
                organization_members::change_role(
                    &pool,
                    organization,
                    member,
                    Assignment::Custom(role),
                    caller,
                )
                .await
                .expect("the assignment is answered")
            })
        };
        let edit = {
            let (pool, start) = (pool.clone(), start.clone());
            let (organization, role) = (arranged.organization, arranged.role);
            tokio::spawn(async move {
                start.wait().await;
                finished(narrow(&pool, organization, role, editor)).await
            })
        };

        let edit = finished(edit).await;
        let outcome = finished(assignment).await;
        assert!(
            matches!(edit, Update::Saved { .. }),
            "iteration {iteration}: {edit:?}"
        );
        let holds = held(pool, arranged.organization, arranged.member).await;
        match outcome {
            RoleChange::Applied(_) => assert!(
                holds == narrowed,
                "iteration {iteration}: the holder got {holds:?}"
            ),
            RoleChange::Exceeds => assert!(
                holds == Grant::organization(OrgRole::Member),
                "iteration {iteration}: a refused assignment leaves {holds:?}"
            ),
            other => panic!("iteration {iteration}: {other:?}"),
        }
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn an_assignment_racing_a_widening_never_leaves_a_holder_beyond_its_assigner() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let assigning = [
        Permission::WorkspaceWrite,
        Permission::WorkspaceMembersUpdate,
    ];
    let assigner = composed(Scope::Workspace, &assigning);
    let widened = composed(
        Scope::Workspace,
        &[Permission::WorkspaceWrite, Permission::WorkspaceUpdate],
    );

    for iteration in 0..ITERATIONS {
        let organization = organization(pool).await;
        let workspace = workspace(pool, organization).await;
        let member = user(pool).await;
        seat_workspace(pool, workspace, member, WorkspaceRole::Viewer).await;
        let owner = user(pool).await;
        seat_workspace(pool, workspace, owner, WorkspaceRole::Owner).await;
        let caller = user(pool).await;
        seat_workspace(pool, workspace, caller, WorkspaceRole::Viewer).await;
        let granted = role(
            pool,
            organization,
            fixture.owner.id,
            Scope::Workspace,
            &assigning,
        )
        .await;
        assign_workspace(pool, workspace, owner, caller, granted).await;
        let editor = editor(pool, organization, OrgRole::Admin).await;
        let role = role(pool, organization, fixture.owner.id, Scope::Workspace, &[]).await;
        let start = Arc::new(Barrier::new(2));

        let assignment = {
            let (pool, start) = (pool.clone(), start.clone());
            tokio::spawn(async move {
                start.wait().await;
                workspace_members::change_role(
                    &pool,
                    workspace,
                    member,
                    Assignment::Custom(role),
                    caller,
                )
                .await
                .expect("the assignment is answered")
            })
        };
        let edit = {
            let (pool, start, widened) = (pool.clone(), start.clone(), widened.clone());
            tokio::spawn(async move {
                start.wait().await;
                let change = Change {
                    grant: Some(&widened),
                    ..Change::default()
                };
                custom_roles::update(&pool, organization, role, &change, editor)
                    .await
                    .expect("the edit is answered")
            })
        };

        let edit = finished(edit).await;
        let outcome = finished(assignment).await;
        let holds = workspace_members::membership(pool, workspace, member)
            .await
            .expect("the membership is read")
            .expect("an active member")
            .grant;
        match (&outcome, &edit) {
            (workspace_members::RoleChange::Applied(_), Update::Exceeds) => assert!(
                assigner.covers(&holds),
                "iteration {iteration}: the holder got {holds:?}"
            ),
            (workspace_members::RoleChange::Exceeds, Update::Saved { .. }) => assert!(
                holds == Grant::workspace(WorkspaceRole::Viewer),
                "iteration {iteration}: a refused assignment leaves {holds:?}"
            ),
            _ => {
                panic!("iteration {iteration}: exactly one side wins, got {outcome:?} and {edit:?}")
            }
        }
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_widening_behind_its_editors_demotion_is_judged_by_the_demoted_grant() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    let workspace = workspace(pool, organization).await;
    let assigner = user(pool).await;
    seat_workspace(pool, workspace, assigner, WorkspaceRole::Owner).await;
    let holder = user(pool).await;
    seat_workspace(pool, workspace, holder, WorkspaceRole::Viewer).await;
    let role = role(pool, organization, fixture.owner.id, Scope::Workspace, &[]).await;
    assign_workspace(pool, workspace, assigner, holder, role).await;
    let owner = editor(pool, organization, OrgRole::Owner).await;

    let mut demotion = pool.begin().await.expect("a transaction");
    sqlx::query(
        "UPDATE organization_members SET role = $3 WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(owner)
    .bind(OrgRole::Admin.as_str())
    .execute(&mut *demotion)
    .await
    .expect("the owner is demoted");
    let edit = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let widened = composed(Scope::Workspace, &[Permission::WorkspaceWrite]);
            let change = Change {
                grant: Some(&widened),
                ..Change::default()
            };
            custom_roles::update(&pool, organization, role, &change, owner)
                .await
                .expect("the edit is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the edit waits for its editor's membership"
    );
    demotion.commit().await.expect("the demotion lands");

    let edit = finished(edit).await;
    assert!(
        matches!(edit, Update::Exceeds),
        "an editor demoted to admin cannot widen a role people hold: {edit:?}"
    );
    let kept = custom_roles::get(pool, organization, role)
        .await
        .expect("the role is read")
        .expect("the role exists");
    assert!(!kept.grant.allows(Permission::WorkspaceWrite));

    fixture.cleanup().await;
}

const ORGANIZATION_ROLE: &str =
    "UPDATE organization_members SET role = $3 WHERE organization_id = $1 AND user_id = $2";
const ORGANIZATION_ACTIVE: &str =
    "UPDATE organization_members SET is_active = $3 WHERE organization_id = $1 AND user_id = $2";
const WORKSPACE_ROLE: &str =
    "UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2";
const WORKSPACE_ACTIVE: &str =
    "UPDATE workspace_members SET is_active = $3 WHERE workspace_id = $1 AND user_id = $2";

/// Sets one column of a member's row to `value` in a transaction left open,
/// so anything that locks the row waits for it.
async fn pending<T>(
    pool: &PgPool,
    statement: &'static str,
    scope: Uuid,
    member: Uuid,
    value: T,
) -> sqlx::Transaction<'static, Postgres>
where
    T: Encode<'static, Postgres> + Type<Postgres>,
{
    let mut transaction = pool.begin().await.expect("a transaction");
    sqlx::query(statement)
        .bind(scope)
        .bind(member)
        .bind(value)
        .execute(&mut *transaction)
        .await
        .expect("the member row is rewritten");
    transaction
}

fn organization_change(
    pool: &PgPool,
    organization: Uuid,
    target: Uuid,
    role: OrgRole,
    caller: Uuid,
) -> JoinHandle<RoleChange> {
    let pool = pool.clone();
    tokio::spawn(async move {
        organization_members::change_role(
            &pool,
            organization,
            target,
            Assignment::BuiltIn(role),
            caller,
        )
        .await
        .expect("the change is answered")
    })
}

fn workspace_change(
    pool: &PgPool,
    workspace: Uuid,
    target: Uuid,
    role: WorkspaceRole,
    caller: Uuid,
) -> JoinHandle<workspace_members::RoleChange> {
    let pool = pool.clone();
    tokio::spawn(async move {
        workspace_members::change_role(&pool, workspace, target, Assignment::BuiltIn(role), caller)
            .await
            .expect("the change is answered")
    })
}

async fn workspace_seat(pool: &PgPool, workspace: Uuid, member: Uuid) -> Option<WorkspaceRole> {
    workspace_members::membership(pool, workspace, member)
        .await
        .expect("the membership is read")
        .map(|membership| membership.role)
}

#[tokio::test]
async fn a_change_behind_its_callers_demotion_is_judged_by_the_demoted_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    seat_organization(pool, organization, user(pool).await, OrgRole::Owner).await;
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Owner).await;
    let target = user(pool).await;
    seat_organization(pool, organization, target, OrgRole::Member).await;

    let demotion = pending(
        pool,
        ORGANIZATION_ROLE,
        organization,
        caller,
        OrgRole::Admin.as_str(),
    )
    .await;
    let change = organization_change(pool, organization, target, OrgRole::Owner, caller);
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's row"
    );
    demotion.commit().await.expect("the demotion lands");

    let outcome = finished(change).await;
    assert!(
        matches!(outcome, RoleChange::Forbidden),
        "a caller demoted to admin cannot seat an owner: {outcome:?}"
    );
    assert!(held(pool, organization, target).await == Grant::organization(OrgRole::Member));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_change_behind_its_callers_deactivation_is_revoked() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    seat_organization(pool, organization, user(pool).await, OrgRole::Owner).await;
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Admin).await;
    let target = user(pool).await;
    seat_organization(pool, organization, target, OrgRole::Member).await;

    let deactivation = pending(pool, ORGANIZATION_ACTIVE, organization, caller, false).await;
    let change = organization_change(pool, organization, target, OrgRole::Admin, caller);
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's row"
    );
    deactivation.commit().await.expect("the deactivation lands");

    let outcome = finished(change).await;
    assert!(
        matches!(outcome, RoleChange::Revoked),
        "a caller removed from the organization no longer holds members:update: {outcome:?}"
    );
    assert!(held(pool, organization, target).await == Grant::organization(OrgRole::Member));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_removal_behind_its_callers_demotion_to_member_is_revoked() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    seat_organization(pool, organization, user(pool).await, OrgRole::Owner).await;
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Admin).await;
    let target = user(pool).await;
    seat_organization(pool, organization, target, OrgRole::Member).await;

    let demotion = pending(
        pool,
        ORGANIZATION_ROLE,
        organization,
        caller,
        OrgRole::Member.as_str(),
    )
    .await;
    let removal = {
        let pool = pool.clone();
        tokio::spawn(async move {
            organization_members::remove_guarded(&pool, organization, target, caller)
                .await
                .expect("the removal is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the removal waits for the caller's row"
    );
    demotion.commit().await.expect("the demotion lands");

    let outcome = finished(removal).await;
    assert!(
        matches!(outcome, organization_members::Removal::Revoked),
        "a caller demoted to member no longer holds members:delete: {outcome:?}"
    );
    assert!(held(pool, organization, target).await == Grant::organization(OrgRole::Member));

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_change_behind_its_callers_demotion_is_judged_by_the_demoted_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let workspace = workspace(pool, organization(pool).await).await;
    seat_workspace(pool, workspace, user(pool).await, WorkspaceRole::Owner).await;
    let caller = user(pool).await;
    seat_workspace(pool, workspace, caller, WorkspaceRole::Owner).await;
    let target = user(pool).await;
    seat_workspace(pool, workspace, target, WorkspaceRole::Viewer).await;

    let demotion = pending(
        pool,
        WORKSPACE_ROLE,
        workspace,
        caller,
        WorkspaceRole::Admin.as_str(),
    )
    .await;
    let change = workspace_change(pool, workspace, target, WorkspaceRole::Admin, caller);
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's row"
    );
    demotion.commit().await.expect("the demotion lands");

    let outcome = finished(change).await;
    assert!(
        matches!(outcome, workspace_members::RoleChange::Forbidden),
        "a caller demoted to admin cannot seat an admin: {outcome:?}"
    );
    assert_eq!(
        workspace_seat(pool, workspace, target).await,
        Some(WorkspaceRole::Viewer)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_change_behind_its_callers_demotion_to_member_is_revoked() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let workspace = workspace(pool, organization(pool).await).await;
    seat_workspace(pool, workspace, user(pool).await, WorkspaceRole::Owner).await;
    let caller = user(pool).await;
    seat_workspace(pool, workspace, caller, WorkspaceRole::Admin).await;
    let target = user(pool).await;
    seat_workspace(pool, workspace, target, WorkspaceRole::Viewer).await;

    let demotion = pending(
        pool,
        WORKSPACE_ROLE,
        workspace,
        caller,
        WorkspaceRole::Member.as_str(),
    )
    .await;
    let change = workspace_change(pool, workspace, target, WorkspaceRole::Member, caller);
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's row"
    );
    demotion.commit().await.expect("the demotion lands");

    let outcome = finished(change).await;
    assert!(
        matches!(outcome, workspace_members::RoleChange::Revoked),
        "a caller demoted to member no longer holds workspace_members:update: {outcome:?}"
    );
    assert_eq!(
        workspace_seat(pool, workspace, target).await,
        Some(WorkspaceRole::Viewer)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_workspace_removal_behind_its_callers_deactivation_is_revoked() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let workspace = workspace(pool, organization(pool).await).await;
    seat_workspace(pool, workspace, user(pool).await, WorkspaceRole::Owner).await;
    let caller = user(pool).await;
    seat_workspace(pool, workspace, caller, WorkspaceRole::Admin).await;
    let target = user(pool).await;
    seat_workspace(pool, workspace, target, WorkspaceRole::Viewer).await;

    let deactivation = pending(pool, WORKSPACE_ACTIVE, workspace, caller, false).await;
    let removal = {
        let pool = pool.clone();
        tokio::spawn(async move {
            workspace_members::remove_guarded(&pool, workspace, target, caller)
                .await
                .expect("the removal is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the removal waits for the caller's row"
    );
    deactivation.commit().await.expect("the deactivation lands");

    let outcome = finished(removal).await;
    assert!(
        matches!(outcome, workspace_members::Removal::Revoked),
        "a caller removed from the workspace no longer holds workspace_members:delete: {outcome:?}"
    );
    assert_eq!(
        workspace_seat(pool, workspace, target).await,
        Some(WorkspaceRole::Viewer)
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_custom_caller_changing_an_editor_of_its_role_never_deadlocks_with_the_edit() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    let owner = editor(pool, organization, OrgRole::Owner).await;
    let admin = editor(pool, organization, OrgRole::Admin).await;
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Member).await;
    let held = role(pool, organization, owner, Scope::Organization, &WIDE).await;
    assign_organization(pool, organization, owner, caller, held).await;

    let mut blocker = pool.begin().await.expect("a transaction");
    sqlx::query("SELECT 1 FROM custom_roles WHERE id = $1 FOR UPDATE")
        .bind(held)
        .execute(&mut *blocker)
        .await
        .expect("the caller's role is held");
    let change = {
        let pool = pool.clone();
        tokio::spawn(async move {
            organization_members::change_role(
                &pool,
                organization,
                admin,
                Assignment::BuiltIn(OrgRole::Member),
                caller,
            )
            .await
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's role"
    );
    let edit = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let narrowed = composed(Scope::Organization, &NARROW);
            let change = Change {
                grant: Some(&narrowed),
                ..Change::default()
            };
            custom_roles::update(&pool, organization, held, &change, admin).await
        })
    };
    assert!(waiting(pool, 2).await, "the edit queues behind the change");
    blocker.commit().await.expect("the caller's role is let go");

    let outcome = finished(change)
        .await
        .expect("the change is answered without a deadlock");
    assert!(
        matches!(outcome, RoleChange::Forbidden),
        "a custom caller never touches an admin: {outcome:?}"
    );
    let edit = finished(edit)
        .await
        .expect("the edit is answered without a deadlock");
    assert!(matches!(edit, Update::Saved { .. }), "{edit:?}");

    fixture.cleanup().await;
}

/// Demotes `editor` to member in a transaction left open, so an edit or a
/// delete by them waits for their row.
async fn demotion(
    pool: &PgPool,
    organization: Uuid,
    editor: Uuid,
) -> sqlx::Transaction<'static, Postgres> {
    pending(
        pool,
        ORGANIZATION_ROLE,
        organization,
        editor,
        OrgRole::Member.as_str(),
    )
    .await
}

#[tokio::test]
async fn an_edit_behind_its_editors_demotion_to_member_is_refused() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    let admin = editor(pool, organization, OrgRole::Admin).await;
    let role = role(pool, organization, admin, Scope::Organization, &NARROW).await;

    let demotion = demotion(pool, organization, admin).await;
    let edit = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let change = Change {
                name: Some("Renamed"),
                ..Change::default()
            };
            custom_roles::update(&pool, organization, role, &change, admin)
                .await
                .expect("the edit is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the edit waits for its editor's membership"
    );
    demotion.commit().await.expect("the demotion lands");

    let edit = finished(edit).await;
    assert!(
        matches!(edit, Update::Forbidden),
        "an editor demoted to member no longer holds roles:update: {edit:?}"
    );
    let kept = custom_roles::get(pool, organization, role)
        .await
        .expect("the role is read")
        .expect("the role exists");
    assert_ne!(kept.name, "Renamed");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_delete_behind_its_editors_demotion_to_member_is_refused() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    let admin = editor(pool, organization, OrgRole::Admin).await;
    let role = role(pool, organization, admin, Scope::Organization, &NARROW).await;

    let demotion = demotion(pool, organization, admin).await;
    let deletion = {
        let pool = pool.clone();
        tokio::spawn(async move {
            custom_roles::delete(&pool, organization, role, admin)
                .await
                .expect("the delete is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the delete waits for its editor's membership"
    );
    demotion.commit().await.expect("the demotion lands");

    let deletion = finished(deletion).await;
    assert!(
        matches!(deletion, Deletion::Forbidden),
        "an editor demoted to member no longer holds roles:update: {deletion:?}"
    );
    let kept = custom_roles::get(pool, organization, role)
        .await
        .expect("the role is read");
    assert!(kept.is_some(), "the role is kept");

    fixture.cleanup().await;
}
