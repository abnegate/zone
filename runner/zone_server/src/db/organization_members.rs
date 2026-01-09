//! Organization membership database queries
//!
//! Provides functions to manage user membership in organizations,
//! including role management and permission checking.

use chrono::NaiveDateTime;
use sqlx::{Executor, PgPool, Postgres};
use uuid::Uuid;

use super::DbResult;

/// Organization role hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrgRole {
    Member = 0,
    Admin = 1,
    Owner = 2,
}

impl std::str::FromStr for OrgRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            "owner" => Ok(Self::Owner),
            _ => Err(()),
        }
    }
}

impl OrgRole {
    /// Convert role to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

/// Organization member row from database
#[derive(Debug, Clone)]
pub struct OrganizationMemberRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub role: OrgRole,
    pub is_active: bool,
    pub invited_by: Option<Uuid>,
    pub invited_at: Option<NaiveDateTime>,
    pub accepted_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Add a member to an organization
///
/// CRITICAL-7: This function now fails if the member already exists (active or inactive).
/// Use `reactivate_member` to explicitly reactivate an inactive member.
pub async fn add_member(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrgRole,
    invited_by: Option<Uuid>,
) -> DbResult<OrganizationMemberRow> {
    let now = chrono::Utc::now().naive_utc();
    let row = sqlx::query!(
        r#"
        INSERT INTO organization_members (organization_id, user_id, role, invited_by, invited_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, organization_id, user_id, role, is_active,
                  invited_by, invited_at, accepted_at, created_at, updated_at
        "#,
        organization_id,
        user_id,
        role.as_str(),
        invited_by,
        now
    )
    .fetch_one(pool)
    .await?;

    Ok(OrganizationMemberRow {
        id: row.id,
        organization_id: row.organization_id,
        user_id: row.user_id,
        role: row.role.parse().unwrap_or(OrgRole::Member),
        is_active: row.is_active,
        invited_by: row.invited_by,
        invited_at: row.invited_at,
        accepted_at: row.accepted_at,
        created_at: row.created_at.unwrap_or(now),
        updated_at: row.updated_at.unwrap_or(now),
    })
}

/// Reactivate an inactive member (or add if they don't exist)
/// This is explicit about the intent to reactivate removed members
pub async fn reactivate_member<'a, E>(
    executor: E,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrgRole,
    invited_by: Option<Uuid>,
) -> DbResult<OrganizationMemberRow>
where
    E: Executor<'a, Database = Postgres>,
{
    let now = chrono::Utc::now().naive_utc();
    let row = sqlx::query!(
        r#"
        INSERT INTO organization_members (organization_id, user_id, role, invited_by, invited_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (organization_id, user_id) DO UPDATE
        SET role = EXCLUDED.role,
            is_active = TRUE,
            invited_by = EXCLUDED.invited_by,
            invited_at = EXCLUDED.invited_at,
            updated_at = NOW()
        RETURNING id, organization_id, user_id, role, is_active,
                  invited_by, invited_at, accepted_at, created_at, updated_at
        "#,
        organization_id,
        user_id,
        role.as_str(),
        invited_by,
        now
    )
    .fetch_one(executor)
    .await?;

    Ok(OrganizationMemberRow {
        id: row.id,
        organization_id: row.organization_id,
        user_id: row.user_id,
        role: row.role.parse().unwrap_or(OrgRole::Member),
        is_active: row.is_active,
        invited_by: row.invited_by,
        invited_at: row.invited_at,
        accepted_at: row.accepted_at,
        created_at: row.created_at.unwrap_or(now),
        updated_at: row.updated_at.unwrap_or(now),
    })
}

/// Remove a member from an organization (set inactive)
pub async fn remove_member(pool: &PgPool, organization_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query!(
        r#"
        UPDATE organization_members
        SET is_active = FALSE, updated_at = NOW()
        WHERE organization_id = $1 AND user_id = $2
        "#,
        organization_id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Get a member by organization and user ID
pub async fn get_member(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<OrganizationMemberRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, organization_id, user_id, role, is_active,
               invited_by, invited_at, accepted_at, created_at, updated_at
        FROM organization_members
        WHERE organization_id = $1 AND user_id = $2
        "#,
        organization_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        let now = chrono::Utc::now().naive_utc();
        OrganizationMemberRow {
            id: r.id,
            organization_id: r.organization_id,
            user_id: r.user_id,
            role: r.role.parse().unwrap_or(OrgRole::Member),
            is_active: r.is_active,
            invited_by: r.invited_by,
            invited_at: r.invited_at,
            accepted_at: r.accepted_at,
            created_at: r.created_at.unwrap_or(now),
            updated_at: r.updated_at.unwrap_or(now),
        }
    }))
}

/// List all active members of an organization
pub async fn list_members(
    pool: &PgPool,
    organization_id: Uuid,
) -> DbResult<Vec<OrganizationMemberRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, organization_id, user_id, role, is_active,
               invited_by, invited_at, accepted_at, created_at, updated_at
        FROM organization_members
        WHERE organization_id = $1 AND is_active = TRUE
        ORDER BY created_at ASC
        "#,
        organization_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let now = chrono::Utc::now().naive_utc();
            OrganizationMemberRow {
                id: r.id,
                organization_id: r.organization_id,
                user_id: r.user_id,
                role: r.role.parse().unwrap_or(OrgRole::Member),
                is_active: r.is_active,
                invited_by: r.invited_by,
                invited_at: r.invited_at,
                accepted_at: r.accepted_at,
                created_at: r.created_at.unwrap_or(now),
                updated_at: r.updated_at.unwrap_or(now),
            }
        })
        .collect())
}

/// List all organizations a user is a member of
pub async fn list_user_organizations(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Vec<super::organizations::OrganizationRow>> {
    let rows = sqlx::query!(
        r#"
        SELECT o.id, o.name, o.slug, o.description, o.is_active, o.created_at, o.updated_at
        FROM organizations o
        INNER JOIN organization_members om ON o.id = om.organization_id
        WHERE om.user_id = $1 AND om.is_active = TRUE
        ORDER BY o.created_at DESC
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| super::organizations::OrganizationRow {
            id: r.id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Update a member's role
pub async fn update_member_role(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrgRole,
) -> DbResult<OrganizationMemberRow> {
    let row = sqlx::query!(
        r#"
        UPDATE organization_members
        SET role = $3, updated_at = NOW()
        WHERE organization_id = $1 AND user_id = $2
        RETURNING id, organization_id, user_id, role, is_active,
                  invited_by, invited_at, accepted_at, created_at, updated_at
        "#,
        organization_id,
        user_id,
        role.as_str()
    )
    .fetch_one(pool)
    .await?;

    let now = chrono::Utc::now().naive_utc();
    Ok(OrganizationMemberRow {
        id: row.id,
        organization_id: row.organization_id,
        user_id: row.user_id,
        role: row.role.parse().unwrap_or(OrgRole::Member),
        is_active: row.is_active,
        invited_by: row.invited_by,
        invited_at: row.invited_at,
        accepted_at: row.accepted_at,
        created_at: row.created_at.unwrap_or(now),
        updated_at: row.updated_at.unwrap_or(now),
    })
}

/// Check if user is an active member of organization
pub async fn is_member(pool: &PgPool, organization_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let result =
        sqlx::query_scalar::<_, Option<bool>>("SELECT check_organization_membership($1, $2)")
            .bind(user_id)
            .bind(organization_id)
            .fetch_optional(pool)
            .await?;

    Ok(result.flatten().unwrap_or(false))
}

/// Check if user is an admin or owner of organization
pub async fn is_admin(pool: &PgPool, organization_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let role: Option<String> = sqlx::query_scalar("SELECT get_organization_role($1, $2)")
        .bind(user_id)
        .bind(organization_id)
        .fetch_one(pool)
        .await?;

    if let Some(role_str) = role {
        if let Ok(role) = role_str.parse::<OrgRole>() {
            return Ok(role >= OrgRole::Admin);
        }
    }

    Ok(false)
}

/// Check if user is an owner of organization
pub async fn is_owner(pool: &PgPool, organization_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let role: Option<String> = sqlx::query_scalar("SELECT get_organization_role($1, $2)")
        .bind(user_id)
        .bind(organization_id)
        .fetch_one(pool)
        .await?;

    if let Some(role_str) = role {
        if let Ok(role) = role_str.parse::<OrgRole>() {
            return Ok(role == OrgRole::Owner);
        }
    }

    Ok(false)
}

/// Count active owners in an organization
pub async fn count_owners(pool: &PgPool, organization_id: Uuid) -> DbResult<i64> {
    let count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) as "count!"
        FROM organization_members
        WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE
        "#,
        organization_id
    )
    .fetch_one(pool)
    .await?;

    Ok(count)
}

/// Add a member to an organization (transaction version)
pub async fn add_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrgRole,
    invited_by: Option<Uuid>,
) -> DbResult<OrganizationMemberRow> {
    let now = chrono::Utc::now().naive_utc();
    let row = sqlx::query!(
        r#"
        INSERT INTO organization_members (organization_id, user_id, role, invited_by, invited_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, organization_id, user_id, role, is_active,
                  invited_by, invited_at, accepted_at, created_at, updated_at
        "#,
        organization_id,
        user_id,
        role.as_str(),
        invited_by,
        now
    )
    .fetch_one(&mut **tx)
    .await?;

    Ok(OrganizationMemberRow {
        id: row.id,
        organization_id: row.organization_id,
        user_id: row.user_id,
        role: row.role.parse().unwrap_or(OrgRole::Member),
        is_active: row.is_active,
        invited_by: row.invited_by,
        invited_at: row.invited_at,
        accepted_at: row.accepted_at,
        created_at: row.created_at.unwrap_or(now),
        updated_at: row.updated_at.unwrap_or(now),
    })
}
