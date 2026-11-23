//! Built-in callers, targets and granted roles through the guarded role
//! changes and removals, against the rules as they stood at 75f7ec41, before
//! permissions existed.
//!
//! Those rules compared ranks: a caller touched an admin or owner only as an
//! owner, only an owner seated an organization owner or a workspace admin or
//! owner, and the last owner and last admin floors held. The member and
//! workspace routes then admitted only admins and owners, so the database
//! never saw a lower caller; it now refuses one itself, as revoked, the way
//! it refuses a caller demoted after the route admitted them. Permissions add
//! one rule, that a caller holds everything the target holds and everything
//! it is given. For every caller the old routes admitted, that rule never
//! decides anything; the matrix proves it before it compares the database
//! with the oracle.
//!
//! Every caller is seated, beside the target or as the target, because the
//! database judges the caller by the membership it reads under its own locks.
//! A caller seated as an owner or admin counts toward the floors.

use sqlx::PgPool;
use uuid::Uuid;
use zone_server::db::custom_roles::Assignment;
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};

use super::{fixture, organization, seat_organization, seat_workspace, user, workspace};

const ORGANIZATION_ROLES: [OrgRole; 3] = [OrgRole::Member, OrgRole::Admin, OrgRole::Owner];

const WORKSPACE_ROLES: [WorkspaceRole; 4] = [
    WorkspaceRole::Viewer,
    WorkspaceRole::Member,
    WorkspaceRole::Admin,
    WorkspaceRole::Owner,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Changed,
    Revoked,
    Forbidden,
    Uncovered,
    Missing,
    LastOwner,
    LastAdmin,
    Exceeds,
    RoleMissing,
}

impl From<organization_members::RoleChange> for Outcome {
    fn from(change: organization_members::RoleChange) -> Self {
        match change {
            organization_members::RoleChange::Applied(_) => Self::Changed,
            organization_members::RoleChange::Revoked => Self::Revoked,
            organization_members::RoleChange::Forbidden => Self::Forbidden,
            organization_members::RoleChange::Uncovered => Self::Uncovered,
            organization_members::RoleChange::Missing => Self::Missing,
            organization_members::RoleChange::LastOwner => Self::LastOwner,
            organization_members::RoleChange::Exceeds => Self::Exceeds,
            organization_members::RoleChange::RoleMissing => Self::RoleMissing,
        }
    }
}

impl From<workspace_members::RoleChange> for Outcome {
    fn from(change: workspace_members::RoleChange) -> Self {
        match change {
            workspace_members::RoleChange::Applied(_) => Self::Changed,
            workspace_members::RoleChange::Revoked => Self::Revoked,
            workspace_members::RoleChange::Forbidden => Self::Forbidden,
            workspace_members::RoleChange::Uncovered => Self::Uncovered,
            workspace_members::RoleChange::Missing => Self::Missing,
            workspace_members::RoleChange::LastAdmin => Self::LastAdmin,
            workspace_members::RoleChange::LastOwner => Self::LastOwner,
            workspace_members::RoleChange::Exceeds => Self::Exceeds,
            workspace_members::RoleChange::RoleMissing => Self::RoleMissing,
        }
    }
}

impl From<organization_members::Removal> for Outcome {
    fn from(removal: organization_members::Removal) -> Self {
        match removal {
            organization_members::Removal::Removed { .. } => Self::Changed,
            organization_members::Removal::Revoked => Self::Revoked,
            organization_members::Removal::Forbidden => Self::Forbidden,
            organization_members::Removal::Uncovered => Self::Uncovered,
            organization_members::Removal::Missing => Self::Missing,
            organization_members::Removal::LastOwner => Self::LastOwner,
        }
    }
}

impl From<workspace_members::Removal> for Outcome {
    fn from(removal: workspace_members::Removal) -> Self {
        match removal {
            workspace_members::Removal::Removed => Self::Changed,
            workspace_members::Removal::Revoked => Self::Revoked,
            workspace_members::Removal::Forbidden => Self::Forbidden,
            workspace_members::Removal::Uncovered => Self::Uncovered,
            workspace_members::Removal::Missing => Self::Missing,
            workspace_members::Removal::LastAdmin => Self::LastAdmin,
            workspace_members::Removal::LastOwner => Self::LastOwner,
        }
    }
}

/// Who else is seated beside the target, which decides the floors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Company {
    Alone,
    Owner,
    Admin,
}

const ORGANIZATION_COMPANY: [Company; 2] = [Company::Alone, Company::Owner];
const WORKSPACE_COMPANY: [Company; 3] = [Company::Alone, Company::Owner, Company::Admin];

/// Who asks: a member seated beside the target with a role of their own, or
/// the target themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller<R> {
    Beside(R),
    Itself,
}

impl<R: Copy> Caller<R> {
    fn role(self, target: R) -> R {
        match self {
            Self::Beside(role) => role,
            Self::Itself => target,
        }
    }

    fn beside(self) -> Option<R> {
        match self {
            Self::Beside(role) => Some(role),
            Self::Itself => None,
        }
    }

    fn every(roles: &[R]) -> impl Iterator<Item = Self> + '_ {
        roles
            .iter()
            .map(|role| Self::Beside(*role))
            .chain(std::iter::once(Self::Itself))
    }
}

/// The organization's active owners once the target is seated among `company`
/// and the caller beside them.
fn organization_owners(caller: Caller<OrgRole>, target: OrgRole, company: Company) -> usize {
    usize::from(target == OrgRole::Owner)
        + usize::from(company == Company::Owner)
        + usize::from(caller.beside() == Some(OrgRole::Owner))
}

/// The workspace's active owners, and its active admins and owners together.
fn workspace_privileged(
    caller: Caller<WorkspaceRole>,
    target: WorkspaceRole,
    company: Company,
) -> (usize, usize) {
    let beside = caller.beside();
    let owners = usize::from(target == WorkspaceRole::Owner)
        + usize::from(company == Company::Owner)
        + usize::from(beside == Some(WorkspaceRole::Owner));
    let privileged = usize::from(target >= WorkspaceRole::Admin)
        + usize::from(company != Company::Alone)
        + usize::from(beside.is_some_and(|role| role >= WorkspaceRole::Admin));
    (owners, privileged)
}

fn old_organization_outranks(caller: OrgRole, target: OrgRole) -> bool {
    target < OrgRole::Admin || caller == OrgRole::Owner
}

fn old_workspace_outranks(caller: WorkspaceRole, target: WorkspaceRole) -> bool {
    target < WorkspaceRole::Admin || caller == WorkspaceRole::Owner
}

fn old_organization_change(
    caller: OrgRole,
    target: OrgRole,
    granted: OrgRole,
    owners: usize,
) -> Outcome {
    if !old_organization_outranks(caller, target) {
        return Outcome::Forbidden;
    }
    if granted == OrgRole::Owner && caller != OrgRole::Owner {
        return Outcome::Forbidden;
    }
    if granted != OrgRole::Owner && owners <= 1 && target == OrgRole::Owner {
        return Outcome::LastOwner;
    }
    Outcome::Changed
}

fn old_organization_removal(caller: OrgRole, target: OrgRole, owners: usize) -> Outcome {
    if !old_organization_outranks(caller, target) {
        return Outcome::Forbidden;
    }
    if owners <= 1 && target == OrgRole::Owner {
        return Outcome::LastOwner;
    }
    Outcome::Changed
}

fn old_workspace_change(
    caller: WorkspaceRole,
    target: WorkspaceRole,
    granted: WorkspaceRole,
    (owners, privileged): (usize, usize),
) -> Outcome {
    if !old_workspace_outranks(caller, target) {
        return Outcome::Forbidden;
    }
    if granted >= WorkspaceRole::Admin && caller != WorkspaceRole::Owner {
        return Outcome::Forbidden;
    }
    if granted != WorkspaceRole::Owner && owners <= 1 && target == WorkspaceRole::Owner {
        return Outcome::LastOwner;
    }
    if granted < WorkspaceRole::Admin && privileged <= 1 && target >= WorkspaceRole::Admin {
        return Outcome::LastAdmin;
    }
    Outcome::Changed
}

fn old_workspace_removal(
    caller: WorkspaceRole,
    target: WorkspaceRole,
    (owners, privileged): (usize, usize),
) -> Outcome {
    if !old_workspace_outranks(caller, target) {
        return Outcome::Forbidden;
    }
    if owners <= 1 && target == WorkspaceRole::Owner {
        return Outcome::LastOwner;
    }
    if privileged <= 1 && target >= WorkspaceRole::Admin {
        return Outcome::LastAdmin;
    }
    Outcome::Changed
}

/// The member routes admitted only admins and owners.
fn old_organization_admits(caller: OrgRole) -> bool {
    caller >= OrgRole::Admin
}

fn old_workspace_admits(caller: WorkspaceRole) -> bool {
    caller >= WorkspaceRole::Admin
}

/// The old routes' admission, then the old rules plus the subset rule, in the
/// order the guarded changes ask them. Built-in sets nest, so holding another
/// role's permissions is outranking or equalling it.
fn expected_organization_change(
    caller: OrgRole,
    target: OrgRole,
    granted: OrgRole,
    owners: usize,
) -> Outcome {
    if !old_organization_admits(caller) {
        return Outcome::Revoked;
    }
    if !old_organization_outranks(caller, target) {
        return Outcome::Forbidden;
    }
    if target > caller {
        return Outcome::Uncovered;
    }
    if granted == OrgRole::Owner && caller != OrgRole::Owner {
        return Outcome::Forbidden;
    }
    if granted > caller {
        return Outcome::Exceeds;
    }
    old_organization_change(caller, target, granted, owners)
}

fn expected_organization_removal(caller: OrgRole, target: OrgRole, owners: usize) -> Outcome {
    if !old_organization_admits(caller) {
        return Outcome::Revoked;
    }
    if old_organization_outranks(caller, target) && target > caller {
        return Outcome::Uncovered;
    }
    old_organization_removal(caller, target, owners)
}

fn expected_workspace_change(
    caller: WorkspaceRole,
    target: WorkspaceRole,
    granted: WorkspaceRole,
    seats: (usize, usize),
) -> Outcome {
    if !old_workspace_admits(caller) {
        return Outcome::Revoked;
    }
    if !old_workspace_outranks(caller, target) {
        return Outcome::Forbidden;
    }
    if target > caller {
        return Outcome::Uncovered;
    }
    if granted >= WorkspaceRole::Admin && caller != WorkspaceRole::Owner {
        return Outcome::Forbidden;
    }
    if granted > caller {
        return Outcome::Exceeds;
    }
    old_workspace_change(caller, target, granted, seats)
}

fn expected_workspace_removal(
    caller: WorkspaceRole,
    target: WorkspaceRole,
    seats: (usize, usize),
) -> Outcome {
    if !old_workspace_admits(caller) {
        return Outcome::Revoked;
    }
    if old_workspace_outranks(caller, target) && target > caller {
        return Outcome::Uncovered;
    }
    old_workspace_removal(caller, target, seats)
}

#[derive(Debug, PartialEq, Eq)]
struct Seat {
    role: String,
    custom_role_id: Option<Uuid>,
    is_active: bool,
}

async fn organization_seat(pool: &PgPool, organization: Uuid, member: Uuid) -> Seat {
    let (role, custom_role_id, is_active) = sqlx::query_as(
        "SELECT role, custom_role_id, is_active FROM organization_members \
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(member)
    .fetch_one(pool)
    .await
    .expect("the organization seat");
    Seat {
        role,
        custom_role_id,
        is_active,
    }
}

async fn workspace_seat(pool: &PgPool, workspace: Uuid, member: Uuid) -> Seat {
    let (role, custom_role_id, is_active) = sqlx::query_as(
        "SELECT role, custom_role_id, is_active FROM workspace_members \
         WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(member)
    .fetch_one(pool)
    .await
    .expect("the workspace seat");
    Seat {
        role,
        custom_role_id,
        is_active,
    }
}

fn seat(role: &str, is_active: bool) -> Seat {
    Seat {
        role: role.to_string(),
        custom_role_id: None,
        is_active,
    }
}

/// The people a case seats: the organization or workspace, its target and the
/// caller asking about them.
#[derive(Debug, Clone, Copy)]
struct Seated {
    scope: Uuid,
    target: Uuid,
    caller: Uuid,
}

/// A fresh organization holding the target with `role`, among `company`, and
/// the caller.
async fn organization_with(
    pool: &PgPool,
    role: OrgRole,
    company: Company,
    caller: Caller<OrgRole>,
) -> Seated {
    let organization = organization(pool).await;
    let target = user(pool).await;
    seat_organization(pool, organization, target, role).await;
    if company == Company::Owner {
        seat_organization(pool, organization, user(pool).await, OrgRole::Owner).await;
    }
    let caller = match caller.beside() {
        Some(role) => {
            let caller = user(pool).await;
            seat_organization(pool, organization, caller, role).await;
            caller
        }
        None => target,
    };
    Seated {
        scope: organization,
        target,
        caller,
    }
}

/// A fresh workspace holding the target with `role`, among `company`, and the
/// caller.
async fn workspace_with(
    pool: &PgPool,
    role: WorkspaceRole,
    company: Company,
    caller: Caller<WorkspaceRole>,
) -> Seated {
    let organization = organization(pool).await;
    let workspace = workspace(pool, organization).await;
    let target = user(pool).await;
    seat_workspace(pool, workspace, target, role).await;
    match company {
        Company::Alone => {}
        Company::Owner => {
            seat_workspace(pool, workspace, user(pool).await, WorkspaceRole::Owner).await;
        }
        Company::Admin => {
            seat_workspace(pool, workspace, user(pool).await, WorkspaceRole::Admin).await;
        }
    }
    let caller = match caller.beside() {
        Some(role) => {
            let caller = user(pool).await;
            seat_workspace(pool, workspace, caller, role).await;
            caller
        }
        None => target,
    };
    Seated {
        scope: workspace,
        target,
        caller,
    }
}

#[test]
fn the_subset_rule_never_decides_for_a_caller_the_old_routes_admitted() {
    for caller in [OrgRole::Admin, OrgRole::Owner] {
        for target in ORGANIZATION_ROLES {
            for company in ORGANIZATION_COMPANY {
                let owners = organization_owners(Caller::Itself, target, company);
                assert_eq!(
                    expected_organization_removal(caller, target, owners),
                    old_organization_removal(caller, target, owners),
                    "removing {target:?} as {caller:?} beside {company:?}"
                );
                for granted in ORGANIZATION_ROLES {
                    assert_eq!(
                        expected_organization_change(caller, target, granted, owners),
                        old_organization_change(caller, target, granted, owners),
                        "{caller:?} giving {target:?} {granted:?} beside {company:?}"
                    );
                }
            }
        }
    }
    for caller in [WorkspaceRole::Admin, WorkspaceRole::Owner] {
        for target in WORKSPACE_ROLES {
            for company in WORKSPACE_COMPANY {
                let seats = workspace_privileged(Caller::Itself, target, company);
                assert_eq!(
                    expected_workspace_removal(caller, target, seats),
                    old_workspace_removal(caller, target, seats),
                    "removing {target:?} as {caller:?} beside {company:?}"
                );
                for granted in WORKSPACE_ROLES {
                    assert_eq!(
                        expected_workspace_change(caller, target, granted, seats),
                        old_workspace_change(caller, target, granted, seats),
                        "{caller:?} giving {target:?} {granted:?} beside {company:?}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn organization_role_changes_follow_the_old_rules_for_every_built_in_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();

    for caller in Caller::every(&ORGANIZATION_ROLES) {
        for target in ORGANIZATION_ROLES {
            for granted in ORGANIZATION_ROLES {
                for company in ORGANIZATION_COMPANY {
                    let seated = organization_with(pool, target, company, caller).await;
                    let expected = expected_organization_change(
                        caller.role(target),
                        target,
                        granted,
                        organization_owners(caller, target, company),
                    );

                    let outcome: Outcome = organization_members::change_role(
                        pool,
                        seated.scope,
                        seated.target,
                        Assignment::BuiltIn(granted),
                        seated.caller,
                    )
                    .await
                    .expect("the change is answered")
                    .into();

                    let case =
                        format!("{caller:?} giving {target:?} {granted:?} beside {company:?}");
                    assert_eq!(outcome, expected, "{case}");
                    let held = if outcome == Outcome::Changed {
                        granted
                    } else {
                        target
                    };
                    assert_eq!(
                        organization_seat(pool, seated.scope, seated.target).await,
                        seat(held.as_str(), true),
                        "{case}"
                    );
                }
            }
        }
    }

    fixture.cleanup().await;
}

#[tokio::test]
async fn organization_removals_follow_the_old_rules_for_every_built_in_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();

    for caller in Caller::every(&ORGANIZATION_ROLES) {
        for target in ORGANIZATION_ROLES {
            for company in ORGANIZATION_COMPANY {
                let seated = organization_with(pool, target, company, caller).await;
                let expected = expected_organization_removal(
                    caller.role(target),
                    target,
                    organization_owners(caller, target, company),
                );

                let outcome: Outcome = organization_members::remove_guarded(
                    pool,
                    seated.scope,
                    seated.target,
                    seated.caller,
                )
                .await
                .expect("the removal is answered")
                .into();

                let case = format!("{caller:?} removing {target:?} beside {company:?}");
                assert_eq!(outcome, expected, "{case}");
                assert_eq!(
                    organization_seat(pool, seated.scope, seated.target).await,
                    seat(target.as_str(), outcome != Outcome::Changed),
                    "{case}"
                );
            }
        }
    }

    let organization = organization(pool).await;
    let owner = user(pool).await;
    seat_organization(pool, organization, owner, OrgRole::Owner).await;
    let outcome: Outcome =
        organization_members::remove_guarded(pool, organization, user(pool).await, owner)
            .await
            .expect("the removal is answered")
            .into();
    assert_eq!(outcome, Outcome::Missing);

    fixture.cleanup().await;
}

#[tokio::test]
async fn workspace_role_changes_follow_the_old_rules_for_every_built_in_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();

    for caller in Caller::every(&WORKSPACE_ROLES) {
        for target in WORKSPACE_ROLES {
            for granted in WORKSPACE_ROLES {
                for company in WORKSPACE_COMPANY {
                    let seated = workspace_with(pool, target, company, caller).await;
                    let expected = expected_workspace_change(
                        caller.role(target),
                        target,
                        granted,
                        workspace_privileged(caller, target, company),
                    );

                    let outcome: Outcome = workspace_members::change_role(
                        pool,
                        seated.scope,
                        seated.target,
                        Assignment::BuiltIn(granted),
                        seated.caller,
                    )
                    .await
                    .expect("the change is answered")
                    .into();

                    let case =
                        format!("{caller:?} giving {target:?} {granted:?} beside {company:?}");
                    assert_eq!(outcome, expected, "{case}");
                    let held = if outcome == Outcome::Changed {
                        granted
                    } else {
                        target
                    };
                    assert_eq!(
                        workspace_seat(pool, seated.scope, seated.target).await,
                        seat(held.as_str(), true),
                        "{case}"
                    );
                }
            }
        }
    }

    let workspace = workspace(pool, organization(pool).await).await;
    let owner = user(pool).await;
    seat_workspace(pool, workspace, owner, WorkspaceRole::Owner).await;
    let outcome: Outcome = workspace_members::change_role(
        pool,
        workspace,
        user(pool).await,
        Assignment::BuiltIn(WorkspaceRole::Member),
        owner,
    )
    .await
    .expect("the change is answered")
    .into();
    assert_eq!(outcome, Outcome::Missing);

    fixture.cleanup().await;
}

#[tokio::test]
async fn workspace_removals_follow_the_old_rules_for_every_built_in_role() {
    let fixture = fixture().await;
    let pool = fixture.pool();

    for caller in Caller::every(&WORKSPACE_ROLES) {
        for target in WORKSPACE_ROLES {
            for company in WORKSPACE_COMPANY {
                let seated = workspace_with(pool, target, company, caller).await;
                let expected = expected_workspace_removal(
                    caller.role(target),
                    target,
                    workspace_privileged(caller, target, company),
                );

                let outcome: Outcome = workspace_members::remove_guarded(
                    pool,
                    seated.scope,
                    seated.target,
                    seated.caller,
                )
                .await
                .expect("the removal is answered")
                .into();

                let case = format!("{caller:?} removing {target:?} beside {company:?}");
                assert_eq!(outcome, expected, "{case}");
                assert_eq!(
                    workspace_seat(pool, seated.scope, seated.target).await,
                    seat(target.as_str(), outcome != Outcome::Changed),
                    "{case}"
                );
            }
        }
    }

    fixture.cleanup().await;
}
