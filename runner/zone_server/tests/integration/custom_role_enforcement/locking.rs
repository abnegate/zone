//! An assignment racing an edit of the role it assigns, or a demotion of the
//! member who makes it.
//!
//! `change_role` share-locks the role it assigns and reads its permissions
//! under that lock; `custom_roles::update` takes the role for update before it
//! counts holders and rewrites permissions. Whichever goes first, the other
//! sees what it left: an assignment is checked against the permissions the
//! holder ends up with, and an edit counts every holder an assignment added.
//!
//! `change_role` also reads its caller's membership under a share lock, so a
//! demotion of the caller already in flight lands first and the change is
//! judged by what the caller holds afterwards.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::{self, Assignment, Change, Update};
use zone_server::db::organization_members::{self, OrgRole, RoleChange};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use super::{
    assign_organization, composed, fixture, organization, role, seat_organization, seat_workspace,
    user, workspace,
};
use crate::common::eventually::eventually;

const ITERATIONS: usize = 8;
const DEADLOCK: Duration = Duration::from_secs(15);

const WIDE: [Permission; 2] = [Permission::MembersCreate, Permission::MembersUpdate];
const NARROW: [Permission; 1] = [Permission::MembersCreate];

/// Waits until `count` backends of this database wait on a lock.
async fn waiting(pool: &PgPool, count: i64) -> bool {
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

fn narrow(pool: &PgPool, organization: Uuid, role: Uuid) -> JoinHandle<Update> {
    let pool = pool.clone();
    tokio::spawn(async move {
        let narrowed = composed(Scope::Organization, &NARROW);
        let change = Change {
            grant: Some(&narrowed),
            ..Change::default()
        };
        custom_roles::update(
            &pool,
            organization,
            role,
            &change,
            &Grant::organization(OrgRole::Owner),
        )
        .await
        .expect("the edit is answered")
    })
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

    let edit = finished(narrow(pool, arranged.organization, arranged.role)).await;
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
    let edit = narrow(pool, arranged.organization, arranged.role);
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
                finished(narrow(&pool, organization, role)).await
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
    let assigner = Grant::workspace(WorkspaceRole::Member);
    let widened = composed(
        Scope::Workspace,
        &[Permission::WorkspaceWrite, Permission::WorkspaceUpdate],
    );

    for iteration in 0..ITERATIONS {
        let organization = organization(pool).await;
        let workspace = workspace(pool, organization).await;
        let member = user(pool).await;
        seat_workspace(pool, workspace, member, WorkspaceRole::Viewer).await;
        let caller = user(pool).await;
        seat_workspace(pool, workspace, caller, WorkspaceRole::Member).await;
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
                custom_roles::update(
                    &pool,
                    organization,
                    role,
                    &change,
                    &Grant::organization(OrgRole::Admin),
                )
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
async fn a_change_behind_its_callers_demotion_is_judged_by_the_demoted_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();
    let organization = organization(pool).await;
    seat_organization(pool, organization, user(pool).await, OrgRole::Owner).await;
    let caller = user(pool).await;
    seat_organization(pool, organization, caller, OrgRole::Admin).await;
    let target = user(pool).await;
    seat_organization(pool, organization, target, OrgRole::Member).await;

    let mut demotion = pool.begin().await.expect("a transaction");
    sqlx::query(
        "UPDATE organization_members SET role = 'member' \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(caller)
    .execute(&mut *demotion)
    .await
    .expect("the caller is demoted");
    let change = {
        let pool = pool.clone();
        tokio::spawn(async move {
            organization_members::change_role(
                &pool,
                organization,
                target,
                Assignment::BuiltIn(OrgRole::Admin),
                caller,
            )
            .await
            .expect("the change is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's row"
    );
    demotion.commit().await.expect("the demotion lands");

    let outcome = finished(change).await;
    assert!(
        matches!(outcome, RoleChange::Exceeds),
        "a caller demoted to member cannot seat an admin: {outcome:?}"
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
    seat_workspace(pool, workspace, caller, WorkspaceRole::Admin).await;
    let target = user(pool).await;
    seat_workspace(pool, workspace, target, WorkspaceRole::Viewer).await;

    let mut demotion = pool.begin().await.expect("a transaction");
    sqlx::query(
        "UPDATE workspace_members SET role = 'viewer' \
         WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(caller)
    .execute(&mut *demotion)
    .await
    .expect("the caller is demoted");
    let change = {
        let pool = pool.clone();
        tokio::spawn(async move {
            workspace_members::change_role(
                &pool,
                workspace,
                target,
                Assignment::BuiltIn(WorkspaceRole::Member),
                caller,
            )
            .await
            .expect("the change is answered")
        })
    };
    assert!(
        waiting(pool, 1).await,
        "the change waits for the caller's row"
    );
    demotion.commit().await.expect("the demotion lands");

    let outcome = finished(change).await;
    assert!(
        matches!(outcome, workspace_members::RoleChange::Exceeds),
        "a caller demoted to viewer cannot seat a member: {outcome:?}"
    );
    let membership = workspace_members::membership(pool, workspace, target)
        .await
        .expect("the membership is read")
        .expect("an active member");
    assert_eq!(membership.role, WorkspaceRole::Viewer);

    fixture.cleanup().await;
}
