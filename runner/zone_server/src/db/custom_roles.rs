//! Per-organization custom roles and the permissions they carry.
//!
//! A custom role has a fixed scope. Its holders get the role's composable,
//! in-scope permissions plus the scope's base, and nothing else: a stored name
//! that is unknown, of another scope or not composable grants nothing.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{Executor, PgConnection, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;
use crate::auth::{Grant, Permission, Scope};

pub const EXCEEDS: &str = "role_exceeds";
pub const EXCEEDS_MESSAGE: &str = "This role grants permissions you do not hold";
pub const AMBIGUOUS_ASSIGNMENT: &str = "Set exactly one of role or custom_role_id";

const UNIQUE_VIOLATION: &str = "23505";
const NAME_KEY: &str = "custom_roles_organization_name_key";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub id: Uuid,
    pub name: String,
}

/// What a membership holds. `role` is the stored built-in column, pinned to
/// the lowest rank while a custom role is assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership<R> {
    pub role: R,
    pub custom_role: Option<Summary>,
    pub grant: Grant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assignment<R> {
    BuiltIn(R),
    Custom(Uuid),
}

/// Why a request names no assignment. Each route words an unknown role in its
/// own terms, since it lists the roles that route accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Unknown,
    Ambiguous,
}

impl<R: FromStr> Assignment<R> {
    pub fn parse(role: Option<&str>, custom_role_id: Option<Uuid>) -> Result<Self, Invalid> {
        match (role, custom_role_id) {
            (Some(role), None) => role
                .parse()
                .map(Self::BuiltIn)
                .map_err(|_| Invalid::Unknown),
            (None, Some(id)) => Ok(Self::Custom(id)),
            _ => Err(Invalid::Ambiguous),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomRoleRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub scope: Scope,
    pub grant: Grant,
    pub members: i64,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
pub struct Definition<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub scope: Scope,
    pub grant: &'a Grant,
}

/// A sparse edit: `None` leaves a field alone, and `description:
/// Some(None)` clears it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Change<'a> {
    pub name: Option<&'a str>,
    pub description: Option<Option<&'a str>>,
    pub grant: Option<&'a Grant>,
}

#[derive(Debug)]
pub enum Creation {
    Created(Box<CustomRoleRow>),
    Forbidden,
    NameTaken,
}

#[derive(Debug)]
pub enum Update {
    Saved {
        previous: Box<CustomRoleRow>,
        saved: Box<CustomRoleRow>,
    },
    Forbidden,
    Exceeds,
    NameTaken,
    Missing,
}

#[derive(Debug)]
pub enum Deletion {
    Deleted(Box<CustomRoleRow>),
    Forbidden,
    InUse(i64),
    Missing,
}

#[derive(Debug, sqlx::FromRow)]
struct StoredRole {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    description: Option<String>,
    scope: String,
    permissions: Vec<String>,
    members: i64,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const SCOPE_COLUMN: &str = "scope";

impl TryFrom<StoredRole> for CustomRoleRow {
    type Error = sqlx::Error;

    fn try_from(role: StoredRole) -> Result<Self, Self::Error> {
        let scope = Scope::parse(&role.scope)
            .filter(|scope| scope.base().is_some())
            .ok_or_else(|| sqlx::Error::ColumnDecode {
                index: SCOPE_COLUMN.to_owned(),
                source: format!("{} is not the scope of a custom role", role.scope).into(),
            })?;
        Ok(Self {
            id: role.id,
            organization_id: role.organization_id,
            name: role.name,
            description: role.description,
            scope,
            grant: Grant::stored(scope, &role.permissions),
            members: role.members,
            created_by: role.created_by,
            created_at: role.created_at,
            updated_at: role.updated_at,
        })
    }
}

const ROLE_COLUMNS: &str = r#"
    r.id, r.organization_id, r.name, r.description, r.scope, r.created_by,
    r.created_at, r.updated_at,
    ARRAY(
        SELECT p.permission FROM custom_role_permissions p
        WHERE p.role_id = r.id ORDER BY p.permission
    ) AS permissions,
    (SELECT COUNT(*) FROM organization_members m WHERE m.custom_role_id = r.id AND m.is_active)
    + (SELECT COUNT(*) FROM workspace_members m WHERE m.custom_role_id = r.id AND m.is_active)
    AS members
"#;

pub async fn list(pool: &PgPool, organization_id: Uuid) -> DbResult<Vec<CustomRoleRow>> {
    let rows: Vec<StoredRole> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT {ROLE_COLUMNS}
        FROM custom_roles r
        WHERE r.organization_id = $1
        ORDER BY r.created_at, r.id
        "#
    )))
    .bind(organization_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(CustomRoleRow::try_from).collect()
}

pub async fn get(
    pool: &PgPool,
    organization_id: Uuid,
    role_id: Uuid,
) -> DbResult<Option<CustomRoleRow>> {
    fetch(pool, organization_id, role_id).await
}

async fn fetch<'e, E>(
    executor: E,
    organization_id: Uuid,
    role_id: Uuid,
) -> DbResult<Option<CustomRoleRow>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<StoredRole> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT {ROLE_COLUMNS}
        FROM custom_roles r
        WHERE r.organization_id = $1 AND r.id = $2
        "#
    )))
    .bind(organization_id)
    .bind(role_id)
    .fetch_optional(executor)
    .await?;

    row.map(CustomRoleRow::try_from).transpose()
}

/// The creator is judged as [`update`] judges an editor, after the organization is held against
/// deletion, which locks the organization before its members.
pub async fn create(
    pool: &PgPool,
    organization_id: Uuid,
    definition: &Definition<'_>,
    created_by: Uuid,
) -> DbResult<Creation> {
    let grant = Grant::stored(definition.scope, definition.grant.names());
    let mut transaction = pool.begin().await?;

    if !super::organizations::hold(&mut transaction, organization_id).await?
        || lock_editor(&mut transaction, organization_id, created_by)
            .await?
            .is_none()
    {
        return Ok(Creation::Forbidden);
    }

    let inserted = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO custom_roles (organization_id, name, description, scope, created_by)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(organization_id)
    .bind(definition.name)
    .bind(definition.description)
    .bind(definition.scope.name())
    .bind(created_by)
    .fetch_one(&mut *transaction)
    .await;
    let role_id = match inserted {
        Ok(role_id) => role_id,
        Err(error) if name_taken(&error) => return Ok(Creation::NameTaken),
        Err(error) => return Err(error),
    };

    store_permissions(&mut transaction, role_id, &grant).await?;
    let created = fetch(&mut *transaction, organization_id, role_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    transaction.commit().await?;

    Ok(Creation::Created(Box::new(created)))
}

/// Widening a workspace role that people already hold reaches into every
/// workspace they hold it in, including ones the editor does not administer,
/// so it needs the administrators permission. Narrowing never does.
///
/// The editor's membership is share-locked before the role, as every role
/// change locks member rows before role rows: a demotion in flight lands first
/// and the edit is judged by what the editor holds after it.
pub async fn update(
    pool: &PgPool,
    organization_id: Uuid,
    role_id: Uuid,
    change: &Change<'_>,
    editor: Uuid,
) -> DbResult<Update> {
    let mut transaction = pool.begin().await?;

    let Some(caller) = lock_editor(&mut transaction, organization_id, editor).await? else {
        return Ok(Update::Forbidden);
    };
    if !lock_for_update(&mut transaction, organization_id, role_id).await? {
        return Ok(Update::Missing);
    }
    let Some(previous) = fetch(&mut *transaction, organization_id, role_id).await? else {
        return Ok(Update::Missing);
    };

    let grant = change
        .grant
        .map(|grant| Grant::stored(previous.scope, grant.names()));
    if let Some(grant) = &grant
        && previous.scope == Scope::Workspace
        && previous.members > 0
        && !previous.grant.covers(grant)
        && !caller.allows(Permission::AdministratorsUpdate)
    {
        return Ok(Update::Exceeds);
    }

    let renamed = sqlx::query(
        r#"
        UPDATE custom_roles
        SET name = COALESCE($3, name),
            description = CASE WHEN $4 THEN $5 ELSE description END,
            updated_at = NOW()
        WHERE organization_id = $1 AND id = $2
        "#,
    )
    .bind(organization_id)
    .bind(role_id)
    .bind(change.name)
    .bind(change.description.is_some())
    .bind(change.description.flatten())
    .execute(&mut *transaction)
    .await;
    match renamed {
        Ok(_) => {}
        Err(error) if name_taken(&error) => return Ok(Update::NameTaken),
        Err(error) => return Err(error),
    }

    if let Some(grant) = &grant {
        sqlx::query("DELETE FROM custom_role_permissions WHERE role_id = $1")
            .bind(role_id)
            .execute(&mut *transaction)
            .await?;
        store_permissions(&mut transaction, role_id, grant).await?;
    }

    let saved = fetch(&mut *transaction, organization_id, role_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    transaction.commit().await?;

    Ok(Update::Saved {
        previous: Box::new(previous),
        saved: Box::new(saved),
    })
}

/// A role someone holds is never deleted out from under them. Inactive
/// memberships still pointing at it let go of it first, so the deferred
/// foreign key has nothing left to refuse at commit. The editor is judged as
/// [`update`] judges one.
pub async fn delete(
    pool: &PgPool,
    organization_id: Uuid,
    role_id: Uuid,
    editor: Uuid,
) -> DbResult<Deletion> {
    let mut transaction = pool.begin().await?;

    if lock_editor(&mut transaction, organization_id, editor)
        .await?
        .is_none()
    {
        return Ok(Deletion::Forbidden);
    }
    if !lock_for_update(&mut transaction, organization_id, role_id).await? {
        return Ok(Deletion::Missing);
    }
    let Some(role) = fetch(&mut *transaction, organization_id, role_id).await? else {
        return Ok(Deletion::Missing);
    };
    if role.members > 0 {
        return Ok(Deletion::InUse(role.members));
    }

    for statement in [
        "UPDATE organization_members SET custom_role_id = NULL WHERE custom_role_id = $1",
        "UPDATE workspace_members SET custom_role_id = NULL WHERE custom_role_id = $1",
    ] {
        sqlx::query(statement)
            .bind(role_id)
            .execute(&mut *transaction)
            .await?;
    }
    sqlx::query("DELETE FROM custom_roles WHERE organization_id = $1 AND id = $2")
        .bind(organization_id)
        .bind(role_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    Ok(Deletion::Deleted(Box::new(role)))
}

/// Holds the role steady while the caller's transaction assigns it: an edit or
/// a delete waits until the assignment lands, then sees the new holder.
pub async fn lock(
    connection: &mut PgConnection,
    organization_id: Uuid,
    role_id: Uuid,
    scope: Scope,
) -> DbResult<Option<(Summary, Grant)>> {
    let role: Option<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, name
        FROM custom_roles
        WHERE id = $1 AND organization_id = $2 AND scope = $3
        FOR SHARE
        "#,
    )
    .bind(role_id)
    .bind(organization_id)
    .bind(scope.name())
    .fetch_optional(&mut *connection)
    .await?;
    let Some((id, name)) = role else {
        return Ok(None);
    };

    let permissions: Vec<String> =
        sqlx::query_scalar("SELECT permission FROM custom_role_permissions WHERE role_id = $1")
            .bind(id)
            .fetch_all(&mut *connection)
            .await?;

    Ok(Some((
        Summary { id, name },
        Grant::stored(scope, permissions),
    )))
}

/// What the editor holds, share-locked for the transaction, or `None` unless
/// it still includes `roles:update`. A custom role never carries that
/// permission, so an editor who gets past this holds no custom role, and an
/// edit never waits for a role while it share-locks another.
async fn lock_editor(
    connection: &mut PgConnection,
    organization_id: Uuid,
    editor: Uuid,
) -> DbResult<Option<Grant>> {
    Ok(
        super::organization_members::lock_membership(connection, organization_id, editor)
            .await?
            .map(|membership| membership.grant)
            .filter(|grant| grant.allows(Permission::RolesUpdate)),
    )
}

async fn lock_for_update(
    connection: &mut PgConnection,
    organization_id: Uuid,
    role_id: Uuid,
) -> DbResult<bool> {
    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM custom_roles WHERE organization_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(role_id)
    .fetch_optional(connection)
    .await?;

    Ok(locked.is_some())
}

async fn store_permissions(
    connection: &mut PgConnection,
    role_id: Uuid,
    grant: &Grant,
) -> DbResult<()> {
    sqlx::query(
        r#"
        INSERT INTO custom_role_permissions (role_id, permission)
        SELECT $1, permission FROM unnest($2::text[]) AS permission
        "#,
    )
    .bind(role_id)
    .bind(grant.names())
    .execute(connection)
    .await?;

    Ok(())
}

fn name_taken(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|error| {
        error.code().as_deref() == Some(UNIQUE_VIOLATION) && error.constraint() == Some(NAME_KEY)
    })
}

/// The custom role a membership row points at, as a joined query reads it.
/// `assigned_role_id` is the member's column; `custom_role_id` is only set when
/// the role it names belongs to the right organization and scope.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct Reference {
    assigned_role_id: Option<Uuid>,
    custom_role_id: Option<Uuid>,
    custom_role_name: Option<String>,
    custom_role_permissions: Vec<String>,
}

impl Reference {
    /// The columns and join that load a [`Reference`] for the membership row
    /// aliased `member`, whose organization is `organization`.
    pub(crate) fn sql(member: &str, organization: &str, scope: Scope) -> (String, String) {
        let scope = scope.name();
        (
            format!(
                r#"{member}.custom_role_id AS assigned_role_id,
                custom_role.id AS custom_role_id,
                custom_role.name AS custom_role_name,
                ARRAY(
                    SELECT custom_role_permission.permission
                    FROM custom_role_permissions custom_role_permission
                    WHERE custom_role_permission.role_id = custom_role.id
                ) AS custom_role_permissions"#
            ),
            format!(
                r#"LEFT JOIN custom_roles custom_role
                ON custom_role.id = {member}.custom_role_id
                AND custom_role.organization_id = {organization}
                AND custom_role.scope = '{scope}'"#
            ),
        )
    }

    pub(crate) fn resolve(self, scope: Scope, built_in: Grant) -> (Option<Summary>, Grant) {
        match (
            self.assigned_role_id,
            self.custom_role_id,
            self.custom_role_name,
        ) {
            (None, _, _) => (None, built_in),
            (Some(_), Some(id), Some(name)) => (
                Some(Summary { id, name }),
                Grant::stored(scope, self.custom_role_permissions),
            ),
            (Some(_), _, _) => (None, Grant::default()),
        }
    }
}

/// A role a membership row points at, read under the caller's lock. A role
/// from another organization or of the wrong scope grants nothing.
pub(crate) async fn held(
    connection: &mut PgConnection,
    organization_id: Uuid,
    assigned: Option<Uuid>,
    scope: Scope,
    built_in: Grant,
) -> DbResult<(Option<Summary>, Grant)> {
    let Some(role_id) = assigned else {
        return Ok((None, built_in));
    };
    Ok(
        match lock(connection, organization_id, role_id, scope).await? {
            Some((summary, grant)) => (Some(summary), grant),
            None => (None, Grant::default()),
        },
    )
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::{Creation, Definition};
    use crate::auth::{Grant, Permission, Scope};
    use crate::db::organization_members::OrgRole;
    use crate::db::workspace_members::WorkspaceRole;

    /// A fresh custom role of `scope` in `organization` holding `granted` and the scope's base,
    /// created by `creator`, who must be an owner or admin of `organization`.
    pub async fn role(
        pool: &PgPool,
        organization: Uuid,
        creator: Uuid,
        scope: Scope,
        granted: &[Permission],
    ) -> Uuid {
        let names: Vec<String> = granted
            .iter()
            .map(|permission| permission.name().to_owned())
            .collect();
        let grant = Grant::compose(scope, &names).expect("the role composes");
        let name = format!("Custom {}", Uuid::new_v4());
        let definition = Definition {
            name: &name,
            description: None,
            scope,
            grant: &grant,
        };
        match super::create(pool, organization, &definition, creator)
            .await
            .expect("the role is created")
        {
            Creation::Created(role) => role.id,
            Creation::Forbidden => panic!("the creator does not hold roles:update"),
            Creation::NameTaken => panic!("a fresh role name was taken"),
        }
    }

    /// [`role`] created by an owner seated only to create it, who leaves again, so the
    /// organization keeps the members and owners the test gave it.
    pub async fn composed(
        pool: &PgPool,
        organization: Uuid,
        scope: Scope,
        granted: &[Permission],
    ) -> Uuid {
        let creator = owner(pool, organization).await;
        let role = role(pool, organization, creator, scope, granted).await;
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(creator)
            .execute(pool)
            .await
            .expect("the owner seated to create the role leaves");
        role
    }

    /// A fresh account seated as an owner of `organization`.
    async fn owner(pool: &PgPool, organization: Uuid) -> Uuid {
        let owner: Uuid = sqlx::query_scalar(
            "INSERT INTO users (email, password_hash) VALUES ($1, 'test-only') RETURNING id",
        )
        .bind(format!("{}@zone.test", Uuid::new_v4()))
        .fetch_one(pool)
        .await
        .expect("a user");
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(organization)
        .bind(owner)
        .bind(OrgRole::Owner.as_str())
        .execute(pool)
        .await
        .expect("an owner is seated");
        owner
    }

    /// Gives an organization member `role`, with the built-in column pinned to member as every
    /// custom holder's is.
    pub async fn assign_organization(pool: &PgPool, organization: Uuid, user: Uuid, role: Uuid) {
        sqlx::query(
            "UPDATE organization_members SET role = $3, custom_role_id = $4 \
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization)
        .bind(user)
        .bind(OrgRole::Member.as_str())
        .bind(role)
        .execute(pool)
        .await
        .expect("the organization custom role is assigned");
    }

    /// Gives a workspace member `role`, with the built-in column pinned to viewer as every custom
    /// holder's is.
    pub async fn assign_workspace(pool: &PgPool, workspace: Uuid, user: Uuid, role: Uuid) {
        sqlx::query(
            "UPDATE workspace_members SET role = $3, custom_role_id = $4 \
             WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace)
        .bind(user)
        .bind(WorkspaceRole::Viewer.as_str())
        .bind(role)
        .execute(pool)
        .await
        .expect("the workspace custom role is assigned");
    }

    /// Moves a workspace member onto a fresh workspace custom role holding `granted` and the base.
    pub async fn hold(
        pool: &PgPool,
        organization: Uuid,
        workspace: Uuid,
        user: Uuid,
        granted: &[Permission],
    ) {
        let role = composed(pool, organization, Scope::Workspace, granted).await;
        assign_workspace(pool, workspace, user, role).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(scope: &str) -> StoredRole {
        StoredRole {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            name: "Writers".to_owned(),
            description: None,
            scope: scope.to_owned(),
            permissions: vec![Permission::WorkspaceWrite.name().to_owned()],
            members: 0,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_stored_role_of_a_custom_scope_decodes_with_its_grant() {
        for scope in [Scope::Organization, Scope::Workspace] {
            let row = CustomRoleRow::try_from(stored(scope.name())).expect("the role decodes");
            assert_eq!(row.scope, scope);
            assert_eq!(
                row.grant,
                Grant::stored(scope, [Permission::WorkspaceWrite.name()])
            );
        }
    }

    #[test]
    fn a_stored_scope_no_custom_role_may_have_is_a_decode_error() {
        for scope in [Scope::Server.name(), "planet", ""] {
            let error = CustomRoleRow::try_from(stored(scope)).expect_err("the scope is refused");
            assert!(
                matches!(&error, sqlx::Error::ColumnDecode { index, .. } if index == SCOPE_COLUMN),
                "{scope:?} gave {error:?}"
            );
        }
    }
}
