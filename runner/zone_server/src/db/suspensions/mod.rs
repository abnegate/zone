//! Memberships taken away from a member who left an organization, kept with
//! the roles they held so a returning member can be given them back.
//!
//! The `release_custom_role` trigger clears a custom role the moment a row is
//! deactivated, so a suspension records it first.
//!
//! Every function here takes its locks in one order: the organization's owner
//! rows, then the member's organization row, then, by ascending workspace,
//! that workspace's admin and owner rows and the member's row in it, and any
//! custom role last.

mod record;
mod resumed;
mod seat;
mod successor;
mod suspension;
mod workspace_record;

use sqlx::{Executor, PgConnection, Postgres, Transaction};
use uuid::Uuid;

use super::DbResult;
use super::custom_roles;
use super::organization_members::{self, OrgRole};
use super::workspace_members;
use crate::auth::Scope;
use record::Record;
use workspace_record::WorkspaceRecord;

pub use resumed::Resumed;
pub use seat::Seat;
pub use successor::Successor;
pub use suspension::Suspension;

/// Deactivates a member's organization membership and every workspace
/// membership they hold in it, recording the role and custom role of each,
/// and seats an owner in each workspace they leave without one.
///
/// A workspace nobody is left to own stays without an owner: leaving an
/// organization is never refused for it.
pub async fn suspend(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Suspension> {
    organization_members::lock_owners(transaction, organization_id).await?;
    if let Some(seat) = Seat::organization(transaction, organization_id, user_id).await?
        && seat.is_active
    {
        sqlx::query(
            r#"
            INSERT INTO organization_suspensions (organization_id, user_id, role, custom_role_id)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (organization_id, user_id) DO UPDATE
            SET role = EXCLUDED.role,
                custom_role_id = EXCLUDED.custom_role_id,
                suspended_at = NOW()
            "#,
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(&seat.role)
        .bind(seat.custom_role_id)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "UPDATE organization_members SET is_active = FALSE, updated_at = NOW() \
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    }

    let mut suspension = Suspension::default();
    for workspace_id in workspaces(transaction, organization_id, user_id).await? {
        workspace_members::lock_privileged(transaction, workspace_id).await?;
        let Some(seat) = Seat::workspace(transaction, workspace_id, user_id).await? else {
            continue;
        };
        if !seat.is_active {
            continue;
        }
        sqlx::query(
            r#"
            INSERT INTO workspace_suspensions
                (workspace_id, user_id, organization_id, role, custom_role_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (workspace_id, user_id) DO UPDATE
            SET organization_id = EXCLUDED.organization_id,
                role = EXCLUDED.role,
                custom_role_id = EXCLUDED.custom_role_id,
                suspended_at = NOW()
            "#,
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(organization_id)
        .bind(&seat.role)
        .bind(seat.custom_role_id)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "UPDATE workspace_members SET is_active = FALSE, updated_at = NOW() \
             WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(workspace_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
        suspension.workspaces.push(workspace_id);
        if let Some(successor) =
            workspace_members::seat_successor(transaction, workspace_id).await?
        {
            suspension.successors.push(Successor {
                workspace_id,
                user_id: successor,
            });
        }
    }
    Ok(suspension)
}

/// Reactivates a suspended member with the roles they held.
///
/// A recorded custom role comes back only while it still belongs to the
/// organization at the scope it was held at; otherwise the organization
/// membership falls back to Member and a workspace membership to the role
/// the custom role was pinned to. A workspace membership comes back only
/// while its row still exists, beside whoever was seated in the meantime, so
/// a returning owner may share the workspace with a successor. A member
/// with no record comes back as Member.
pub async fn resume(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Resumed> {
    organization_members::lock_owners(transaction, organization_id).await?;
    let seat = Seat::organization(transaction, organization_id, user_id).await?;
    let record = Record::lock(transaction, organization_id, user_id).await?;
    let records = WorkspaceRecord::lock_all(transaction, organization_id, user_id).await?;
    let Some(seat) = seat else {
        forget(&mut **transaction, organization_id, user_id).await?;
        return Ok(Resumed::default());
    };

    let mut returning = Vec::with_capacity(records.len());
    for record in records {
        workspace_members::lock_privileged(transaction, record.workspace_id).await?;
        let suspended = Seat::workspace(transaction, record.workspace_id, user_id)
            .await?
            .is_some_and(|seat| !seat.is_active);
        if suspended {
            returning.push(record);
        }
    }

    let organization = if seat.is_active {
        None
    } else {
        Some(organization_role(transaction, organization_id, record).await?)
    };
    for record in &mut returning {
        if let Some(role_id) = record.custom_role_id
            && custom_roles::lock(transaction, organization_id, role_id, Scope::Workspace)
                .await?
                .is_none()
        {
            record.custom_role_id = None;
        }
    }

    if let Some(organization) = &organization {
        sqlx::query(
            "UPDATE organization_members \
             SET is_active = TRUE, role = $3, custom_role_id = $4, updated_at = NOW() \
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(&organization.role)
        .bind(organization.custom_role_id)
        .execute(&mut **transaction)
        .await?;
    }
    let mut workspaces = Vec::with_capacity(returning.len());
    for record in returning {
        sqlx::query(
            "UPDATE workspace_members \
             SET is_active = TRUE, role = $3, custom_role_id = $4, updated_at = NOW() \
             WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(record.workspace_id)
        .bind(user_id)
        .bind(&record.role)
        .bind(record.custom_role_id)
        .execute(&mut **transaction)
        .await?;
        workspaces.push(record.workspace_id);
    }

    forget(&mut **transaction, organization_id, user_id).await?;
    Ok(Resumed {
        organization: organization.is_some(),
        workspaces,
    })
}

/// Drops every suspension record of a member in an organization, so a
/// member who comes back another way does not get their old access with it.
pub async fn forget<'a, E>(executor: E, organization_id: Uuid, user_id: Uuid) -> DbResult<u64>
where
    E: Executor<'a, Database = Postgres>,
{
    let forgotten: i64 = sqlx::query_scalar(
        r#"
        WITH organization AS (
            DELETE FROM organization_suspensions
            WHERE organization_id = $1 AND user_id = $2
            RETURNING 1
        ),
        workspace AS (
            DELETE FROM workspace_suspensions
            WHERE organization_id = $1 AND user_id = $2
            RETURNING 1
        )
        SELECT (SELECT COUNT(*) FROM organization) + (SELECT COUNT(*) FROM workspace)
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_one(executor)
    .await?;
    Ok(u64::try_from(forgotten).unwrap_or_default())
}

/// Deletes a member's organization membership, every workspace membership
/// they hold in it and their suspension records, seating an owner in each
/// workspace they were active in and leave without one.
pub async fn remove(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Vec<Successor>> {
    organization_members::lock_owners(transaction, organization_id).await?;
    Seat::organization(transaction, organization_id, user_id).await?;

    let mut successors = Vec::new();
    for workspace_id in workspaces(transaction, organization_id, user_id).await? {
        workspace_members::lock_privileged(transaction, workspace_id).await?;
        let Some(seat) = Seat::workspace(transaction, workspace_id, user_id).await? else {
            continue;
        };
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
            .bind(workspace_id)
            .bind(user_id)
            .execute(&mut **transaction)
            .await?;
        if !seat.is_active {
            continue;
        }
        if let Some(successor) =
            workspace_members::seat_successor(transaction, workspace_id).await?
        {
            successors.push(Successor {
                workspace_id,
                user_id: successor,
            });
        }
    }

    sqlx::query("DELETE FROM organization_members WHERE organization_id = $1 AND user_id = $2")
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    forget(&mut **transaction, organization_id, user_id).await?;
    Ok(successors)
}

/// The organization role a returning member is given back.
async fn organization_role(
    connection: &mut PgConnection,
    organization_id: Uuid,
    record: Option<Record>,
) -> DbResult<Record> {
    let member = || Record {
        role: OrgRole::Member.as_str().to_string(),
        custom_role_id: None,
    };
    let Some(record) = record else {
        return Ok(member());
    };
    let Some(role_id) = record.custom_role_id else {
        return Ok(record);
    };
    match custom_roles::lock(connection, organization_id, role_id, Scope::Organization).await? {
        Some((summary, _)) => Ok(Record {
            role: OrgRole::Member.as_str().to_string(),
            custom_role_id: Some(summary.id),
        }),
        None => Ok(member()),
    }
}

/// The organization's workspaces the member has a row in, active or not, by
/// ascending id.
async fn workspaces(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"
        SELECT member.workspace_id
        FROM workspace_members AS member
        JOIN workspaces AS workspace ON workspace.id = member.workspace_id
        WHERE workspace.organization_id = $1 AND member.user_id = $2
        ORDER BY member.workspace_id
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_all(connection)
    .await
}
