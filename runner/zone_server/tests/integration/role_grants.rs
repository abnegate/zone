//! The database half of custom roles: the seeded built-in workspace grants,
//! the `workspace_permits` function, the stored-permission CHECK, the trigger
//! that releases a custom role, the deferred foreign keys, role storage and
//! the guarded role changes. Each is held against the Rust catalogue.

use std::collections::BTreeSet;

use crate::common::create_test_pool;
use sqlx::PgPool;
use uuid::Uuid;
use zone_server::auth::{Grant, Permission, Scope};
use zone_server::db::custom_roles::{
    self, Assignment, Change, Creation, Definition, Deletion, Invalid, Summary, Update,
};
use zone_server::db::organization_members::{self, OrgRole};
use zone_server::db::workspace_members::{self, WorkspaceRole};

const WORKSPACE_ROLES: [WorkspaceRole; 4] = [
    WorkspaceRole::Viewer,
    WorkspaceRole::Member,
    WorkspaceRole::Admin,
    WorkspaceRole::Owner,
];

const FOREIGN_KEY_VIOLATION: &str = "23503";
const CHECK_VIOLATION: &str = "23514";

fn scoped(scope: Scope) -> impl Iterator<Item = Permission> {
    Permission::ALL
        .into_iter()
        .filter(move |permission| permission.scope() == scope)
}

fn names(permissions: &[Permission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| permission.name().to_string())
        .collect()
}

async fn user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, password_hash) VALUES ($1, $2) RETURNING id")
        .bind(format!("{}@example.test", Uuid::new_v4()))
        .bind("x")
        .fetch_one(pool)
        .await
        .expect("a user")
}

async fn organization(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING id")
        .bind("Roles")
        .bind(Uuid::new_v4().to_string())
        .fetch_one(pool)
        .await
        .expect("an organization")
}

async fn workspace(pool: &PgPool, organization: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO workspaces (organization_id, name, slug) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind("Roles")
    .bind(Uuid::new_v4().to_string())
    .fetch_one(pool)
    .await
    .expect("a workspace")
}

async fn seat_organization(
    pool: &PgPool,
    organization: Uuid,
    member: Uuid,
    role: OrgRole,
    custom_role: Option<Uuid>,
) {
    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role, custom_role_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(organization)
    .bind(member)
    .bind(role.as_str())
    .bind(custom_role)
    .execute(pool)
    .await
    .expect("an organization member is seated");
}

async fn seat_workspace(
    pool: &PgPool,
    workspace: Uuid,
    member: Uuid,
    role: WorkspaceRole,
    custom_role: Option<Uuid>,
) {
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role, custom_role_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(workspace)
    .bind(member)
    .bind(role.as_str())
    .bind(custom_role)
    .execute(pool)
    .await
    .expect("a workspace member is seated");
}

async fn composed(pool: &PgPool, organization: Uuid, scope: Scope, granted: &[Permission]) -> Uuid {
    let grant = Grant::compose(scope, &names(granted)).expect("the role composes");
    let creator = user(pool).await;
    let definition = Definition {
        name: &format!("Role {}", Uuid::new_v4()),
        description: None,
        scope,
        grant: &grant,
    };
    match custom_roles::create(pool, organization, &definition, creator)
        .await
        .expect("the role is created")
    {
        Creation::Created(role) => role.id,
        Creation::NameTaken => panic!("a fresh name is never taken"),
    }
}

/// A role written straight into the tables, past `Grant::compose`, so the
/// rows hold exactly what the test says and nothing is normalized.
async fn raw(pool: &PgPool, organization: Uuid, scope: Scope, stored: &[&str]) -> Uuid {
    let role: Uuid = sqlx::query_scalar(
        "INSERT INTO custom_roles (organization_id, name, scope) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization)
    .bind(format!("Raw {}", Uuid::new_v4()))
    .bind(scope.name())
    .fetch_one(pool)
    .await
    .expect("a raw role");
    for permission in stored {
        sqlx::query("INSERT INTO custom_role_permissions (role_id, permission) VALUES ($1, $2)")
            .bind(role)
            .bind(*permission)
            .execute(pool)
            .await
            .expect("a stored permission");
    }
    role
}

async fn sql_permits(pool: &PgPool, member: Uuid, workspace: Uuid, name: &str) -> bool {
    sqlx::query_scalar("SELECT workspace_permits($1, $2, $3)")
        .bind(member)
        .bind(workspace)
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("workspace_permits answers")
}

async fn custom_role_of_organization_member(
    pool: &PgPool,
    organization: Uuid,
    member: Uuid,
) -> Option<Uuid> {
    sqlx::query_scalar(
        "SELECT custom_role_id FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization)
    .bind(member)
    .fetch_one(pool)
    .await
    .expect("the member row")
}

async fn custom_role_of_workspace_member(
    pool: &PgPool,
    workspace: Uuid,
    member: Uuid,
) -> Option<Uuid> {
    sqlx::query_scalar(
        "SELECT custom_role_id FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(member)
    .fetch_one(pool)
    .await
    .expect("the member row")
}

fn database_code(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|code| code.into_owned())
}

#[tokio::test]
async fn the_seeded_workspace_grants_equal_the_rust_mapping_in_both_directions() {
    let pool = create_test_pool().await;
    let seeded: BTreeSet<(String, String)> =
        sqlx::query_as("SELECT role, permission FROM workspace_role_permissions")
            .fetch_all(&pool)
            .await
            .expect("the seeded rows")
            .into_iter()
            .collect();
    let mapped: BTreeSet<(String, String)> = WORKSPACE_ROLES
        .into_iter()
        .flat_map(|role| {
            Grant::workspace(role)
                .names()
                .into_iter()
                .map(move |name| (role.as_str().to_string(), name.to_string()))
        })
        .collect();

    let missing: Vec<_> = mapped.difference(&seeded).collect();
    let extra: Vec<_> = seeded.difference(&mapped).collect();
    assert!(
        missing.is_empty(),
        "rows the Rust mapping grants but SQL does not: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "rows SQL grants but the Rust mapping does not: {extra:?}"
    );
}

#[tokio::test]
async fn workspace_permits_agrees_with_the_built_in_grants_for_every_role_and_name() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;

    for role in WORKSPACE_ROLES {
        let member = user(&pool).await;
        seat_workspace(&pool, workspace, member, role, None).await;
        let grant = Grant::workspace(role);
        for permission in Permission::ALL {
            assert_eq!(
                sql_permits(&pool, member, workspace, permission.name()).await,
                grant.allows(permission),
                "workspace_permits for {role:?} and {}",
                permission.name()
            );
            assert_eq!(
                workspace_members::permits(&pool, workspace, member, permission)
                    .await
                    .expect("permits answers"),
                grant.allows(permission),
                "permits for {role:?} and {}",
                permission.name()
            );
        }
        let membership = workspace_members::membership(&pool, workspace, member)
            .await
            .expect("membership answers")
            .expect("an active member");
        assert_eq!(membership.grant, grant);
        assert_eq!(membership.role, role);
        assert_eq!(membership.custom_role, None);
    }
}

#[tokio::test]
async fn workspace_permits_agrees_with_membership_for_custom_roles() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;

    let writer = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite, Permission::PullRequestsOverride],
    )
    .await;
    let baseless = raw(&pool, organization, Scope::Workspace, &["workspace:write"]).await;
    let empty = raw(&pool, organization, Scope::Workspace, &[]).await;

    for role in [writer, baseless, empty] {
        let member = user(&pool).await;
        seat_workspace(&pool, workspace, member, WorkspaceRole::Viewer, Some(role)).await;
        let membership = workspace_members::membership(&pool, workspace, member)
            .await
            .expect("membership answers")
            .expect("an active member");
        assert_eq!(
            membership.custom_role.as_ref().map(|role| role.id),
            Some(role)
        );
        assert!(membership.grant.allows(Permission::WorkspaceRead));
        for permission in scoped(Scope::Workspace) {
            assert_eq!(
                workspace_members::permits(&pool, workspace, member, permission)
                    .await
                    .expect("permits answers"),
                membership.grant.allows(permission),
                "custom role and {}",
                permission.name()
            );
        }
    }

    let member = user(&pool).await;
    seat_workspace(
        &pool,
        workspace,
        member,
        WorkspaceRole::Viewer,
        Some(baseless),
    )
    .await;
    assert!(
        workspace_members::is_member(&pool, member, workspace)
            .await
            .expect("is_member answers"),
        "a role stored without its base row still holds the base"
    );
    assert!(
        workspace_members::can_write(&pool, workspace, member)
            .await
            .expect("can_write answers")
    );
}

#[tokio::test]
async fn the_check_accepts_exactly_the_composable_and_base_names() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let role = raw(&pool, organization, Scope::Workspace, &[]).await;

    let mut accepted = 0;
    for permission in Permission::ALL {
        let allowed = permission.composable() || Some(permission) == permission.scope().base();
        let outcome = sqlx::query(
            "INSERT INTO custom_role_permissions (role_id, permission) VALUES ($1, $2)",
        )
        .bind(role)
        .bind(permission.name())
        .execute(&pool)
        .await;
        match outcome {
            Ok(_) => {
                assert!(
                    allowed,
                    "{} was stored but is not composable",
                    permission.name()
                );
                accepted += 1;
            }
            Err(error) => {
                assert!(!allowed, "{} was refused: {error}", permission.name());
                assert_eq!(database_code(&error).as_deref(), Some(CHECK_VIOLATION));
            }
        }
    }
    let expected = Permission::ALL
        .into_iter()
        .filter(|permission| {
            permission.composable() || Some(*permission) == permission.scope().base()
        })
        .count();
    assert_eq!(accepted, expected);

    let unknown =
        sqlx::query("INSERT INTO custom_role_permissions (role_id, permission) VALUES ($1, $2)")
            .bind(role)
            .bind("workspace:fly")
            .execute(&pool)
            .await
            .expect_err("an unknown name is refused");
    assert_eq!(database_code(&unknown).as_deref(), Some(CHECK_VIOLATION));
}

#[tokio::test]
async fn a_workspace_role_storing_an_organization_name_grants_nothing_for_it() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let role = raw(
        &pool,
        organization,
        Scope::Workspace,
        &["mcp_servers:update", "members:create"],
    )
    .await;
    let member = user(&pool).await;
    seat_workspace(&pool, workspace, member, WorkspaceRole::Viewer, Some(role)).await;

    let membership = workspace_members::membership(&pool, workspace, member)
        .await
        .expect("membership answers")
        .expect("an active member");
    assert_eq!(
        membership.grant.names(),
        vec![Permission::WorkspaceRead.name()]
    );
    for name in ["mcp_servers:update", "members:create"] {
        assert!(
            !sql_permits(&pool, member, workspace, name).await,
            "workspace_permits granted the organization permission {name}"
        );
    }
    assert!(
        !workspace_members::permits(&pool, workspace, member, Permission::McpServersUpdate)
            .await
            .expect("permits answers")
    );
}

#[tokio::test]
async fn a_foreign_or_mis_scoped_workspace_role_grants_nothing() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let elsewhere = self::organization(&pool).await;
    let workspace = workspace(&pool, organization).await;

    let foreign = composed(
        &pool,
        elsewhere,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let mis_scoped = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;

    for role in [foreign, mis_scoped] {
        let member = user(&pool).await;
        seat_workspace(&pool, workspace, member, WorkspaceRole::Viewer, Some(role)).await;

        for permission in scoped(Scope::Workspace) {
            assert!(
                !sql_permits(&pool, member, workspace, permission.name()).await,
                "{} was granted through a role of the wrong organization or scope",
                permission.name()
            );
        }
        let checked: bool = sqlx::query_scalar("SELECT check_workspace_membership($1, $2)")
            .bind(member)
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .expect("check_workspace_membership answers");
        assert!(!checked);
        assert!(
            !workspace_members::is_member(&pool, member, workspace)
                .await
                .expect("is_member answers")
        );
        let membership = workspace_members::membership(&pool, workspace, member)
            .await
            .expect("membership answers")
            .expect("the row is active");
        assert!(membership.grant.is_empty(), "{:?}", membership.grant);
        assert_eq!(membership.custom_role, None);
    }
}

#[tokio::test]
async fn a_foreign_or_mis_scoped_organization_role_grants_nothing() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let elsewhere = self::organization(&pool).await;

    let foreign = composed(
        &pool,
        elsewhere,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let mis_scoped = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;

    for role in [foreign, mis_scoped] {
        let member = user(&pool).await;
        seat_organization(&pool, organization, member, OrgRole::Member, Some(role)).await;
        let membership = organization_members::membership(&pool, organization, member)
            .await
            .expect("membership answers")
            .expect("the row is active");
        assert!(membership.grant.is_empty(), "{:?}", membership.grant);
        assert_eq!(membership.custom_role, None);
        assert!(
            !organization_members::permits(
                &pool,
                organization,
                member,
                Permission::OrganizationRead
            )
            .await
            .expect("permits answers")
        );
    }
}

#[tokio::test]
async fn an_organization_custom_role_grants_its_permissions_and_the_base() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let role = raw(
        &pool,
        organization,
        Scope::Organization,
        &["devices:read", "workspace:write"],
    )
    .await;
    let member = user(&pool).await;
    seat_organization(&pool, organization, member, OrgRole::Member, Some(role)).await;

    let membership = organization_members::membership(&pool, organization, member)
        .await
        .expect("membership answers")
        .expect("an active member");
    assert_eq!(
        membership.grant.names(),
        vec!["devices:read", "organization:read"]
    );
    assert_eq!(
        membership.custom_role.as_ref().map(|role| role.id),
        Some(role)
    );
    assert_eq!(membership.role, OrgRole::Member);

    let mut connection = pool.acquire().await.expect("a connection");
    let locked = organization_members::lock_membership(&mut connection, organization, member)
        .await
        .expect("lock_membership answers")
        .expect("an active member");
    assert_eq!(locked, membership);

    let listed = organization_members::list_members_with_users(&pool, organization)
        .await
        .expect("the members are listed");
    let listed = listed
        .iter()
        .find(|listed| listed.member.user_id == member)
        .expect("the member is listed");
    assert_eq!(listed.grant, membership.grant);
    assert_eq!(listed.custom_role, membership.custom_role);

    let organizations = organization_members::list_user_organizations_with_role(&pool, member)
        .await
        .expect("the organizations are listed");
    let listed = organizations
        .iter()
        .find(|listed| listed.organization.id == organization)
        .expect("the organization is listed");
    assert_eq!(listed.grant, membership.grant);
    assert_eq!(listed.custom_role, membership.custom_role);
}

#[tokio::test]
async fn workspace_listings_and_locks_carry_the_custom_role() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let role = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let member = user(&pool).await;
    seat_workspace(&pool, workspace, member, WorkspaceRole::Viewer, Some(role)).await;

    let membership = workspace_members::membership(&pool, workspace, member)
        .await
        .expect("membership answers")
        .expect("an active member");

    let mut connection = pool.acquire().await.expect("a connection");
    let locked = workspace_members::lock_membership(&mut connection, workspace, member)
        .await
        .expect("lock_membership answers")
        .expect("an active member");
    assert_eq!(locked, membership);

    let members = workspace_members::list_members_with_users(&pool, workspace)
        .await
        .expect("the members are listed");
    let listed = members
        .iter()
        .find(|listed| listed.member.user_id == member)
        .expect("the member is listed");
    assert_eq!(listed.grant, membership.grant);
    assert_eq!(listed.custom_role, membership.custom_role);
    assert_eq!(listed.grant.workspace_rank(), WorkspaceRole::Member);

    let workspaces = workspace_members::list_user_workspaces_in_org(&pool, member, organization)
        .await
        .expect("the workspaces are listed");
    let listed = workspaces
        .iter()
        .find(|listed| listed.workspace.id == workspace)
        .expect("the workspace is listed");
    assert_eq!(listed.grant, membership.grant);
    assert_eq!(listed.custom_role, membership.custom_role);
}

#[tokio::test]
async fn the_trigger_releases_a_custom_role_on_deactivation_and_on_a_legacy_role_update() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let organization_role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let workspace_role = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;

    let leaving = user(&pool).await;
    seat_organization(
        &pool,
        organization,
        leaving,
        OrgRole::Member,
        Some(organization_role),
    )
    .await;
    seat_workspace(
        &pool,
        workspace,
        leaving,
        WorkspaceRole::Viewer,
        Some(workspace_role),
    )
    .await;
    organization_members::remove_member(&pool, organization, leaving)
        .await
        .expect("the organization member is deactivated");
    workspace_members::remove_member(&pool, workspace, leaving)
        .await
        .expect("the workspace member is deactivated");
    assert_eq!(
        custom_role_of_organization_member(&pool, organization, leaving).await,
        None
    );
    assert_eq!(
        custom_role_of_workspace_member(&pool, workspace, leaving).await,
        None
    );

    let promoted = user(&pool).await;
    seat_organization(
        &pool,
        organization,
        promoted,
        OrgRole::Member,
        Some(organization_role),
    )
    .await;
    seat_workspace(
        &pool,
        workspace,
        promoted,
        WorkspaceRole::Viewer,
        Some(workspace_role),
    )
    .await;
    let role: String = sqlx::query_scalar(
        "UPDATE organization_members SET role = $3 WHERE organization_id = $1 AND user_id = $2 RETURNING role",
    )
    .bind(organization)
    .bind(promoted)
    .bind(OrgRole::Admin.as_str())
    .fetch_one(&pool)
    .await
    .expect("a role-only update lands despite the custom role check");
    assert_eq!(role, OrgRole::Admin.as_str());
    let role: String = sqlx::query_scalar(
        "UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2 RETURNING role",
    )
    .bind(workspace)
    .bind(promoted)
    .bind(WorkspaceRole::Member.as_str())
    .fetch_one(&pool)
    .await
    .expect("a role-only update lands despite the custom role check");
    assert_eq!(role, WorkspaceRole::Member.as_str());
    assert_eq!(
        custom_role_of_organization_member(&pool, organization, promoted).await,
        None
    );
    assert_eq!(
        custom_role_of_workspace_member(&pool, workspace, promoted).await,
        None
    );

    let kept = user(&pool).await;
    seat_workspace(
        &pool,
        workspace,
        kept,
        WorkspaceRole::Viewer,
        Some(workspace_role),
    )
    .await;
    sqlx::query(
        "UPDATE workspace_members SET updated_at = NOW() WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace)
    .bind(kept)
    .execute(&pool)
    .await
    .expect("an unrelated update");
    assert_eq!(
        custom_role_of_workspace_member(&pool, workspace, kept).await,
        Some(workspace_role),
        "an update that leaves the role alone keeps the custom role"
    );
}

#[tokio::test]
async fn a_custom_role_needs_the_lowest_built_in_column() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let organization_role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let workspace_role = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let member = user(&pool).await;

    let error = sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role, custom_role_id) \
         VALUES ($1, $2, 'admin', $3)",
    )
    .bind(organization)
    .bind(member)
    .bind(organization_role)
    .execute(&pool)
    .await
    .expect_err("an admin cannot also hold a custom role");
    assert_eq!(database_code(&error).as_deref(), Some(CHECK_VIOLATION));

    let error = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role, custom_role_id) \
         VALUES ($1, $2, 'member', $3)",
    )
    .bind(workspace)
    .bind(member)
    .bind(workspace_role)
    .execute(&pool)
    .await
    .expect_err("a workspace member cannot also hold a custom role");
    assert_eq!(database_code(&error).as_deref(), Some(CHECK_VIOLATION));
}

#[tokio::test]
async fn deleting_an_organization_cascades_through_members_holding_custom_roles() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let organization_role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let workspace_role = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let member = user(&pool).await;
    seat_organization(
        &pool,
        organization,
        member,
        OrgRole::Member,
        Some(organization_role),
    )
    .await;
    seat_workspace(
        &pool,
        workspace,
        member,
        WorkspaceRole::Viewer,
        Some(workspace_role),
    )
    .await;

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(organization)
        .execute(&pool)
        .await
        .expect("the organization is deleted along with its roles and members");

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM custom_roles WHERE organization_id = $1")
            .bind(organization)
            .fetch_one(&pool)
            .await
            .expect("the roles are countable");
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn a_role_someone_holds_cannot_be_deleted() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let role = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let member = user(&pool).await;
    seat_workspace(&pool, workspace, member, WorkspaceRole::Viewer, Some(role)).await;

    let error = sqlx::query("DELETE FROM custom_roles WHERE id = $1")
        .bind(role)
        .execute(&pool)
        .await
        .expect_err("the deferred foreign key refuses at commit");
    assert_eq!(
        database_code(&error).as_deref(),
        Some(FOREIGN_KEY_VIOLATION)
    );

    let outcome = custom_roles::delete(&pool, organization, role)
        .await
        .expect("the delete is answered");
    assert!(matches!(outcome, Deletion::InUse(1)), "{outcome:?}");

    workspace_members::remove_member(&pool, workspace, member)
        .await
        .expect("the holder leaves");
    let outcome = custom_roles::delete(&pool, organization, role)
        .await
        .expect("the delete is answered");
    assert!(
        matches!(outcome, Deletion::Deleted(ref deleted) if deleted.id == role),
        "{outcome:?}"
    );

    let outcome = custom_roles::delete(&pool, organization, role)
        .await
        .expect("the delete is answered");
    assert!(matches!(outcome, Deletion::Missing), "{outcome:?}");
}

#[tokio::test]
async fn delete_lets_go_of_inactive_references_before_it_deletes() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let member = user(&pool).await;
    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role, is_active, custom_role_id) \
         VALUES ($1, $2, 'member', FALSE, $3)",
    )
    .bind(organization)
    .bind(member)
    .bind(role)
    .execute(&pool)
    .await
    .expect("an inactive member pointing at the role");

    let outcome = custom_roles::delete(&pool, organization, role)
        .await
        .expect("the delete is answered");
    assert!(matches!(outcome, Deletion::Deleted(_)), "{outcome:?}");
    assert_eq!(
        custom_role_of_organization_member(&pool, organization, member).await,
        None
    );
}

#[tokio::test]
async fn roles_are_created_listed_read_and_named_uniquely() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let elsewhere = self::organization(&pool).await;
    let creator = user(&pool).await;
    let grant = Grant::compose(
        Scope::Workspace,
        &names(&[Permission::WorkspaceWrite, Permission::ProjectSyncsUpdate]),
    )
    .expect("the role composes");

    let definition = Definition {
        name: "Release Managers",
        description: Some("Ship things"),
        scope: Scope::Workspace,
        grant: &grant,
    };
    let created = match custom_roles::create(&pool, organization, &definition, creator)
        .await
        .expect("the role is created")
    {
        Creation::Created(role) => role,
        Creation::NameTaken => panic!("the name is free"),
    };
    assert_eq!(created.name, "Release Managers");
    assert_eq!(created.description.as_deref(), Some("Ship things"));
    assert_eq!(created.scope, Scope::Workspace);
    assert_eq!(created.grant, grant);
    assert_eq!(created.members, 0);
    assert_eq!(created.created_by, Some(creator));
    assert_eq!(created.organization_id, organization);

    let clash = Definition {
        name: "  release managers ",
        ..definition
    };
    assert!(matches!(
        custom_roles::create(&pool, organization, &clash, creator)
            .await
            .expect("answered"),
        Creation::NameTaken
    ));
    assert!(
        matches!(
            custom_roles::create(&pool, elsewhere, &definition, creator)
                .await
                .expect("answered"),
            Creation::Created(_)
        ),
        "names are unique per organization only"
    );

    let listed = custom_roles::list(&pool, organization)
        .await
        .expect("the roles are listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0], *created);
    assert_eq!(
        custom_roles::get(&pool, organization, created.id)
            .await
            .expect("answered"),
        Some(*created.clone())
    );
    assert_eq!(
        custom_roles::get(&pool, elsewhere, created.id)
            .await
            .expect("answered"),
        None,
        "a role is invisible from another organization"
    );
}

#[tokio::test]
async fn create_stores_only_what_a_role_of_its_scope_may_carry() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let creator = user(&pool).await;
    let owner = Grant::workspace(WorkspaceRole::Owner);
    let definition = Definition {
        name: "Everything",
        description: None,
        scope: Scope::Workspace,
        grant: &owner,
    };
    let Creation::Created(created) =
        custom_roles::create(&pool, organization, &definition, creator)
            .await
            .expect("the role is created")
    else {
        panic!("the name is free");
    };
    assert!(!created.grant.allows(Permission::WorkspaceDelete));
    assert!(
        !created
            .grant
            .allows(Permission::WorkspaceAdministratorsUpdate)
    );
    assert_eq!(created.grant, Grant::workspace(WorkspaceRole::Admin));
}

#[tokio::test]
async fn update_renames_describes_and_regrants_sparsely() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let other = composed(&pool, organization, Scope::Organization, &[]).await;
    let other_name = custom_roles::get(&pool, organization, other)
        .await
        .expect("answered")
        .expect("the role exists")
        .name;
    let admin = Grant::organization(OrgRole::Admin);

    let outcome = custom_roles::update(
        &pool,
        organization,
        role,
        &Change {
            name: Some("Device Wranglers"),
            description: Some(Some("Fleet")),
            grant: None,
        },
        &admin,
    )
    .await
    .expect("answered");
    let Update::Saved { previous, saved } = outcome else {
        panic!("the rename is saved: {outcome:?}");
    };
    assert_eq!(saved.name, "Device Wranglers");
    assert_eq!(saved.description.as_deref(), Some("Fleet"));
    assert_eq!(saved.grant, previous.grant);
    assert!(saved.updated_at >= previous.updated_at);

    let regranted = Grant::compose(
        Scope::Organization,
        &names(&[Permission::DevicesUpdate, Permission::AuditLogsRead]),
    )
    .expect("the grant composes");
    let outcome = custom_roles::update(
        &pool,
        organization,
        role,
        &Change {
            name: None,
            description: Some(None),
            grant: Some(&regranted),
        },
        &admin,
    )
    .await
    .expect("answered");
    let Update::Saved { saved, .. } = outcome else {
        panic!("the regrant is saved: {outcome:?}");
    };
    assert_eq!(saved.name, "Device Wranglers");
    assert_eq!(saved.description, None);
    assert_eq!(saved.grant, regranted);

    let outcome = custom_roles::update(
        &pool,
        organization,
        role,
        &Change {
            name: Some(&other_name.to_uppercase()),
            ..Change::default()
        },
        &admin,
    )
    .await
    .expect("answered");
    assert!(matches!(outcome, Update::NameTaken), "{outcome:?}");

    let elsewhere = self::organization(&pool).await;
    let outcome = custom_roles::update(&pool, elsewhere, role, &Change::default(), &admin)
        .await
        .expect("answered");
    assert!(matches!(outcome, Update::Missing), "{outcome:?}");
}

#[tokio::test]
async fn widening_a_held_workspace_role_needs_the_administrators_permission() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let held = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let member = user(&pool).await;
    seat_workspace(&pool, workspace, member, WorkspaceRole::Viewer, Some(held)).await;

    let admin = Grant::organization(OrgRole::Admin);
    let owner = Grant::organization(OrgRole::Owner);
    let wider = Grant::compose(
        Scope::Workspace,
        &names(&[Permission::WorkspaceWrite, Permission::WorkspaceUpdate]),
    )
    .expect("the wider grant composes");
    let narrower = Grant::compose(Scope::Workspace, &[]).expect("the narrower grant composes");
    let widen = Change {
        grant: Some(&wider),
        ..Change::default()
    };

    let outcome = custom_roles::update(&pool, organization, held, &widen, &admin)
        .await
        .expect("answered");
    assert!(matches!(outcome, Update::Exceeds), "{outcome:?}");
    let unchanged = custom_roles::get(&pool, organization, held)
        .await
        .expect("answered")
        .expect("the role exists");
    assert!(!unchanged.grant.allows(Permission::WorkspaceUpdate));

    let outcome = custom_roles::update(
        &pool,
        organization,
        held,
        &Change {
            grant: Some(&narrower),
            ..Change::default()
        },
        &admin,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, Update::Saved { .. }),
        "narrowing is always allowed: {outcome:?}"
    );

    let outcome = custom_roles::update(&pool, organization, held, &widen, &owner)
        .await
        .expect("answered");
    assert!(
        matches!(outcome, Update::Saved { .. }),
        "an owner widens: {outcome:?}"
    );

    let unheld = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    let outcome = custom_roles::update(&pool, organization, unheld, &widen, &admin)
        .await
        .expect("answered");
    assert!(
        matches!(outcome, Update::Saved { .. }),
        "a role nobody holds widens: {outcome:?}"
    );

    let organization_role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;
    let holder = user(&pool).await;
    seat_organization(
        &pool,
        organization,
        holder,
        OrgRole::Member,
        Some(organization_role),
    )
    .await;
    let wider = Grant::compose(
        Scope::Organization,
        &names(&[Permission::DevicesRead, Permission::DevicesUpdate]),
    )
    .expect("the wider grant composes");
    let outcome = custom_roles::update(
        &pool,
        organization,
        organization_role,
        &Change {
            grant: Some(&wider),
            ..Change::default()
        },
        &admin,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, Update::Saved { ref saved, .. } if saved.members == 1),
        "an organization role is capped at what every editor holds: {outcome:?}"
    );
}

#[tokio::test]
async fn an_organization_member_is_assigned_a_custom_role_and_back() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let owner = user(&pool).await;
    let member = user(&pool).await;
    seat_organization(&pool, organization, owner, OrgRole::Owner, None).await;
    seat_organization(&pool, organization, member, OrgRole::Admin, None).await;
    let role = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::DevicesRead],
    )
    .await;

    let outcome = organization_members::change_role(
        &pool,
        organization,
        member,
        Assignment::Custom(role),
        owner,
    )
    .await
    .expect("answered");
    let organization_members::RoleChange::Applied(assigned) = outcome else {
        panic!("the custom role is assigned: {outcome:?}");
    };
    assert_eq!(assigned.member.role, OrgRole::Member);
    assert_eq!(
        assigned
            .membership
            .custom_role
            .as_ref()
            .map(|summary| summary.id),
        Some(role),
        "the change carries the role it assigned"
    );
    assert!(assigned.membership.grant.allows(Permission::DevicesRead));
    let membership = organization_members::membership(&pool, organization, member)
        .await
        .expect("answered")
        .expect("an active member");
    assert_eq!(
        membership.custom_role,
        Some(Summary {
            id: role,
            name: custom_roles::get(&pool, organization, role)
                .await
                .expect("answered")
                .expect("the role exists")
                .name,
        })
    );
    assert!(membership.grant.allows(Permission::DevicesRead));
    assert!(!membership.grant.allows(Permission::MembersCreate));

    let outcome = organization_members::change_role(
        &pool,
        organization,
        member,
        Assignment::BuiltIn(OrgRole::Admin),
        owner,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::Applied(_)),
        "{outcome:?}"
    );
    let membership = organization_members::membership(&pool, organization, member)
        .await
        .expect("answered")
        .expect("an active member");
    assert_eq!(membership.custom_role, None);
    assert_eq!(membership.grant, Grant::organization(OrgRole::Admin));
}

#[tokio::test]
async fn organization_assignments_refuse_missing_roles_escalation_and_the_last_owner() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let elsewhere = self::organization(&pool).await;
    let owner = user(&pool).await;
    let admin = user(&pool).await;
    let member = user(&pool).await;
    let narrow = user(&pool).await;
    let updaters = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::MembersUpdate],
    )
    .await;
    seat_organization(&pool, organization, owner, OrgRole::Owner, None).await;
    seat_organization(&pool, organization, admin, OrgRole::Admin, None).await;
    seat_organization(&pool, organization, member, OrgRole::Member, None).await;
    seat_organization(&pool, organization, narrow, OrgRole::Member, Some(updaters)).await;

    for missing in [
        composed(&pool, elsewhere, Scope::Organization, &[]).await,
        composed(&pool, organization, Scope::Workspace, &[]).await,
        Uuid::new_v4(),
    ] {
        let outcome = organization_members::change_role(
            &pool,
            organization,
            member,
            Assignment::Custom(missing),
            owner,
        )
        .await
        .expect("answered");
        assert!(
            matches!(outcome, organization_members::RoleChange::RoleMissing),
            "{outcome:?}"
        );
    }

    let auditors = composed(
        &pool,
        organization,
        Scope::Organization,
        &[Permission::AuditLogsRead],
    )
    .await;
    let outcome = organization_members::change_role(
        &pool,
        organization,
        member,
        Assignment::Custom(auditors),
        narrow,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::Exceeds),
        "{outcome:?}"
    );

    let outcome = organization_members::change_role(
        &pool,
        organization,
        member,
        Assignment::Custom(auditors),
        admin,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::Applied(_)),
        "{outcome:?}"
    );

    let outcome = organization_members::change_role(
        &pool,
        organization,
        member,
        Assignment::BuiltIn(OrgRole::Member),
        narrow,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::Uncovered),
        "a caller who does not hold the target's permissions cannot touch them: {outcome:?}"
    );
    let outcome = organization_members::remove_guarded(&pool, organization, member, narrow)
        .await
        .expect("answered");
    assert!(
        matches!(outcome, organization_members::Removal::Uncovered),
        "{outcome:?}"
    );

    let outcome = organization_members::change_role(
        &pool,
        organization,
        owner,
        Assignment::Custom(auditors),
        owner,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::LastOwner),
        "a custom assignment demotes the last owner: {outcome:?}"
    );

    let outcome = organization_members::change_role(
        &pool,
        organization,
        owner,
        Assignment::Custom(auditors),
        admin,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, organization_members::RoleChange::Forbidden),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn workspace_assignments_follow_the_same_rules() {
    let pool = create_test_pool().await;
    let organization = organization(&pool).await;
    let elsewhere = self::organization(&pool).await;
    let workspace = workspace(&pool, organization).await;
    let owner = user(&pool).await;
    let admin = user(&pool).await;
    let member = user(&pool).await;
    let narrow = user(&pool).await;
    let writing = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite],
    )
    .await;
    seat_workspace(&pool, workspace, owner, WorkspaceRole::Owner, None).await;
    seat_workspace(&pool, workspace, admin, WorkspaceRole::Admin, None).await;
    seat_workspace(&pool, workspace, member, WorkspaceRole::Member, None).await;
    seat_workspace(
        &pool,
        workspace,
        narrow,
        WorkspaceRole::Viewer,
        Some(writing),
    )
    .await;
    let writers = composed(
        &pool,
        organization,
        Scope::Workspace,
        &[Permission::WorkspaceWrite, Permission::ProjectSyncsUpdate],
    )
    .await;

    for missing in [
        composed(&pool, elsewhere, Scope::Workspace, &[]).await,
        composed(&pool, organization, Scope::Organization, &[]).await,
    ] {
        let outcome = workspace_members::change_role(
            &pool,
            workspace,
            member,
            Assignment::Custom(missing),
            owner,
        )
        .await
        .expect("answered");
        assert!(
            matches!(outcome, workspace_members::RoleChange::RoleMissing),
            "{outcome:?}"
        );
    }

    let outcome = workspace_members::change_role(
        &pool,
        workspace,
        member,
        Assignment::Custom(writers),
        narrow,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, workspace_members::RoleChange::Exceeds),
        "{outcome:?}"
    );

    let outcome = workspace_members::change_role(
        &pool,
        workspace,
        member,
        Assignment::Custom(writers),
        admin,
    )
    .await
    .expect("answered");
    let workspace_members::RoleChange::Applied(assigned) = outcome else {
        panic!("an admin assigns the role: {outcome:?}");
    };
    assert_eq!(assigned.member.role, WorkspaceRole::Viewer);
    assert_eq!(
        assigned
            .membership
            .custom_role
            .as_ref()
            .map(|summary| summary.id),
        Some(writers),
        "the change carries the role it assigned"
    );
    assert!(
        workspace_members::can_write(&pool, workspace, member)
            .await
            .expect("answered")
    );

    let outcome = workspace_members::remove_guarded(&pool, workspace, member, narrow)
        .await
        .expect("answered");
    assert!(
        matches!(outcome, workspace_members::Removal::Uncovered),
        "a caller who lacks project_syncs:update cannot remove its holder: {outcome:?}"
    );

    let outcome =
        workspace_members::change_role(&pool, workspace, owner, Assignment::Custom(writers), owner)
            .await
            .expect("answered");
    assert!(
        matches!(outcome, workspace_members::RoleChange::LastOwner),
        "a custom assignment demotes the last owner: {outcome:?}"
    );

    let fellow = user(&pool).await;
    seat_workspace(&pool, workspace, fellow, WorkspaceRole::Admin, None).await;
    let outcome = workspace_members::change_role(
        &pool,
        workspace,
        fellow,
        Assignment::Custom(writers),
        admin,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, workspace_members::RoleChange::Forbidden),
        "only the administrators permission touches an admin: {outcome:?}"
    );

    let outcome = workspace_members::change_role(
        &pool,
        workspace,
        member,
        Assignment::BuiltIn(WorkspaceRole::Admin),
        admin,
    )
    .await
    .expect("answered");
    assert!(
        matches!(outcome, workspace_members::RoleChange::Forbidden),
        "{outcome:?}"
    );

    let outcome = workspace_members::remove_guarded(&pool, workspace, member, admin)
        .await
        .expect("answered");
    assert!(
        matches!(outcome, workspace_members::Removal::Removed),
        "{outcome:?}"
    );
}

#[test]
fn an_assignment_names_exactly_one_role() {
    let role = Uuid::new_v4();
    assert_eq!(
        Assignment::<OrgRole>::parse(Some("admin"), None),
        Ok(Assignment::BuiltIn(OrgRole::Admin))
    );
    assert_eq!(
        Assignment::<WorkspaceRole>::parse(None, Some(role)),
        Ok(Assignment::Custom(role))
    );
    assert_eq!(
        Assignment::<OrgRole>::parse(Some("viewer"), None),
        Err(Invalid::Unknown)
    );
    assert_eq!(
        Assignment::<OrgRole>::parse(Some("admin"), Some(role)),
        Err(Invalid::Ambiguous)
    );
    assert_eq!(
        Assignment::<OrgRole>::parse(None, None),
        Err(Invalid::Ambiguous)
    );
}
