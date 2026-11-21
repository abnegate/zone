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
pub const UNKNOWN_ROLE: &str = "Unknown role";
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

impl<R: FromStr> Assignment<R> {
    pub fn parse(role: Option<&str>, custom_role_id: Option<Uuid>) -> Result<Self, &'static str> {
        match (role, custom_role_id) {
            (Some(role), None) => role.parse().map(Self::BuiltIn).map_err(|_| UNKNOWN_ROLE),
            (None, Some(id)) => Ok(Self::Custom(id)),
            _ => Err(AMBIGUOUS_ASSIGNMENT),
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
    NameTaken,
}

#[derive(Debug)]
pub enum Update {
    Saved {
        previous: Box<CustomRoleRow>,
        saved: Box<CustomRoleRow>,
    },
    Exceeds,
    NameTaken,
    Missing,
}

#[derive(Debug)]
pub enum Deletion {
    Deleted(Box<CustomRoleRow>),
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

impl From<StoredRole> for CustomRoleRow {
    fn from(role: StoredRole) -> Self {
        let scope = Scope::parse(&role.scope).unwrap_or(Scope::Server);
        Self {
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
        }
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

    Ok(rows.into_iter().map(CustomRoleRow::from).collect())
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

    Ok(row.map(CustomRoleRow::from))
}

pub async fn create(
    pool: &PgPool,
    organization_id: Uuid,
    definition: &Definition<'_>,
    created_by: Uuid,
) -> DbResult<Creation> {
    let grant = Grant::stored(definition.scope, definition.grant.names());
    let mut transaction = pool.begin().await?;

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
pub async fn update(
    pool: &PgPool,
    organization_id: Uuid,
    role_id: Uuid,
    change: &Change<'_>,
    caller: &Grant,
) -> DbResult<Update> {
    let mut transaction = pool.begin().await?;

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
/// foreign key has nothing left to refuse at commit.
pub async fn delete(pool: &PgPool, organization_id: Uuid, role_id: Uuid) -> DbResult<Deletion> {
    let mut transaction = pool.begin().await?;

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
                cr.id AS custom_role_id,
                cr.name AS custom_role_name,
                ARRAY(
                    SELECT crp.permission FROM custom_role_permissions crp
                    WHERE crp.role_id = cr.id
                ) AS custom_role_permissions"#
            ),
            format!(
                r#"LEFT JOIN custom_roles cr
                ON cr.id = {member}.custom_role_id
                AND cr.organization_id = {organization}
                AND cr.scope = '{scope}'"#
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
