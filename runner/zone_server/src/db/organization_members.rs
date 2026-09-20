//! Organization membership database queries
//!
//! Provides functions to manage user membership in organizations,
//! including role management and permission checking.

use chrono::NaiveDateTime;
use sqlx::{Executor, PgConnection, PgPool, Postgres};
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

/// The target's own role, held for the length of the transaction.
///
/// Reading it in the route and acting on it here are two moments: an owner can
/// promote the target in between, and the request then lands on an admin or
/// owner the caller was never allowed to touch.
async fn lock_member(
    transaction: &mut sqlx::Transaction<'_, Postgres>,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<OrgRole>> {
    let role: Option<String> = sqlx::query_scalar(
        r#"
        SELECT role FROM organization_members
        WHERE organization_id = $1 AND user_id = $2 AND is_active = TRUE
        FOR UPDATE
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(role.map(|role| role.parse().unwrap_or(OrgRole::Member)))
}

/// Whether `caller` outranks what `target` currently holds.
fn outranks(caller: OrgRole, target: OrgRole) -> bool {
    target < OrgRole::Admin || caller == OrgRole::Owner
}

/// Outcome of a removal that must leave an owner seated.
#[derive(Debug)]
pub enum Removal {
    Removed,
    Forbidden,
    Missing,
    LastOwner,
}

/// Remove a member, refusing to unseat the organization's last owner.
///
/// The plain [`remove_member`] is the unguarded one, for callers that mean to
/// strip access. This is the one a route wants: it counts and removes with the
/// owner rows locked, so two removals arriving together cannot each read a
/// count that says one may go.
pub async fn remove_guarded(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    caller: OrgRole,
) -> DbResult<Removal> {
    let mut transaction = pool.begin().await?;

    let owners: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT user_id
        FROM organization_members
        WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE
        FOR UPDATE
        "#,
    )
    .bind(organization_id)
    .fetch_all(&mut *transaction)
    .await?;

    let Some(target) = lock_member(&mut transaction, organization_id, user_id).await? else {
        return Ok(Removal::Missing);
    };
    if !outranks(caller, target) {
        return Ok(Removal::Forbidden);
    }

    if owners.len() <= 1 && owners.contains(&user_id) {
        return Ok(Removal::LastOwner);
    }

    let result = sqlx::query(
        r#"
        UPDATE organization_members
        SET is_active = FALSE, updated_at = NOW()
        WHERE organization_id = $1 AND user_id = $2 AND is_active = TRUE
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    if result.rows_affected() > 0 {
        Ok(Removal::Removed)
    } else {
        Ok(Removal::Missing)
    }
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

/// An active member together with the account they sign in with.
#[derive(Debug, Clone)]
pub struct MemberWithUser {
    pub member: OrganizationMemberRow,
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct MemberWithUserRow {
    id: Uuid,
    organization_id: Uuid,
    user_id: Uuid,
    role: String,
    is_active: bool,
    invited_by: Option<Uuid>,
    invited_at: Option<NaiveDateTime>,
    accepted_at: Option<NaiveDateTime>,
    created_at: Option<NaiveDateTime>,
    updated_at: Option<NaiveDateTime>,
    email: String,
    display_name: Option<String>,
}

impl From<MemberWithUserRow> for MemberWithUser {
    fn from(row: MemberWithUserRow) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            member: OrganizationMemberRow {
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
            },
            email: row.email,
            display_name: row.display_name,
        }
    }
}

const MEMBER_WITH_USER_COLUMNS: &str = r#"
    om.id, om.organization_id, om.user_id, om.role, om.is_active,
    om.invited_by, om.invited_at, om.accepted_at, om.created_at, om.updated_at,
    u.email, u.display_name
"#;

/// List the active members of an organization with their emails and names.
pub async fn list_members_with_users(
    pool: &PgPool,
    organization_id: Uuid,
) -> DbResult<Vec<MemberWithUser>> {
    let rows: Vec<MemberWithUserRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT {MEMBER_WITH_USER_COLUMNS}
        FROM organization_members om
        INNER JOIN users u ON u.id = om.user_id
        WHERE om.organization_id = $1 AND om.is_active = TRUE
        ORDER BY om.created_at ASC
        "#
    )))
    .bind(organization_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(MemberWithUser::from).collect())
}

/// One member of an organization with their email and name.
pub async fn get_member_with_user(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<MemberWithUser>> {
    let row: Option<MemberWithUserRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT {MEMBER_WITH_USER_COLUMNS}
        FROM organization_members om
        INNER JOIN users u ON u.id = om.user_id
        WHERE om.organization_id = $1 AND om.user_id = $2
        "#
    )))
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(MemberWithUser::from))
}

/// An organization a user belongs to, with the role they hold in it.
#[derive(Debug, Clone)]
pub struct UserOrganization {
    pub organization: super::organizations::OrganizationRow,
    pub role: OrgRole,
}

#[derive(Debug, sqlx::FromRow)]
struct UserOrganizationRow {
    id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    is_active: Option<bool>,
    created_at: Option<NaiveDateTime>,
    updated_at: Option<NaiveDateTime>,
    role: String,
}

/// List the organizations a user is an active member of, with their role.
pub async fn list_user_organizations_with_role(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Vec<UserOrganization>> {
    let rows: Vec<UserOrganizationRow> = sqlx::query_as(
        r#"
        SELECT o.id, o.name, o.slug, o.description, o.is_active, o.created_at, o.updated_at,
               om.role
        FROM organizations o
        INNER JOIN organization_members om ON o.id = om.organization_id
        WHERE om.user_id = $1 AND om.is_active = TRUE
        ORDER BY o.created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| UserOrganization {
            organization: super::organizations::OrganizationRow {
                id: row.id,
                name: row.name,
                slug: row.slug,
                description: row.description,
                is_active: row.is_active,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            role: row.role.parse().unwrap_or(OrgRole::Member),
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

/// Outcome of a role change that must leave an owner seated.
#[derive(Debug)]
pub enum RoleChange {
    Applied(Box<OrganizationMemberRow>),
    Forbidden,
    Missing,
    LastOwner,
}

/// Change a member's role, refusing to unseat the organization's last owner.
///
/// Counting the owners and then updating one of them are two statements, so two
/// demotions racing each other both read a safe count and between them leave
/// none. Lock the owner rows for the length of the transaction: the second
/// demotion waits, then counts what the first actually left.
pub async fn change_role(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrgRole,
    caller: OrgRole,
) -> DbResult<RoleChange> {
    let mut transaction = pool.begin().await?;

    let owners: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT user_id
        FROM organization_members
        WHERE organization_id = $1 AND role = 'owner' AND is_active = TRUE
        FOR UPDATE
        "#,
    )
    .bind(organization_id)
    .fetch_all(&mut *transaction)
    .await?;

    let Some(target) = lock_member(&mut transaction, organization_id, user_id).await? else {
        return Ok(RoleChange::Missing);
    };
    if !outranks(caller, target) {
        return Ok(RoleChange::Forbidden);
    }

    // The rank being granted is the other half of the same question, and the
    // route asks it too. Ask it here as well: this is the guarded entry point,
    // and a caller reaching it should not be able to acquire one guard without
    // the other. Only an owner seats an owner; an admin seating an admin is
    // this organization's policy, deliberately, and the route it goes through
    // says the same.
    if role == OrgRole::Owner && caller != OrgRole::Owner {
        return Ok(RoleChange::Forbidden);
    }

    if role != OrgRole::Owner && owners.len() <= 1 && owners.contains(&user_id) {
        return Ok(RoleChange::LastOwner);
    }

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
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;

    let now = chrono::Utc::now().naive_utc();
    Ok(RoleChange::Applied(Box::new(OrganizationMemberRow {
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
    })))
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

/// Read and lock an active membership for the duration of the caller's transaction.
///
/// Holding this shared row lock gives membership revocation and the protected
/// operation a definite order. Callers must keep the transaction open until
/// the protected read or mutation finishes.
pub async fn lock_role(
    connection: &mut PgConnection,
    organization_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<OrgRole>> {
    let role: Option<String> = sqlx::query_scalar(
        r#"
        SELECT role
        FROM organization_members
        WHERE organization_id = $1
          AND user_id = $2
          AND is_active = TRUE
        FOR SHARE
        "#,
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(connection)
    .await?;

    Ok(role.and_then(|value| value.parse().ok()))
}

/// Check if user is an admin or owner of organization
pub async fn is_admin(pool: &PgPool, organization_id: Uuid, user_id: Uuid) -> DbResult<bool> {
    let role: Option<String> = sqlx::query_scalar("SELECT get_organization_role($1, $2)")
        .bind(user_id)
        .bind(organization_id)
        .fetch_one(pool)
        .await?;

    if let Some(role_str) = role
        && let Ok(role) = role_str.parse::<OrgRole>()
    {
        return Ok(role >= OrgRole::Admin);
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

    if let Some(role_str) = role
        && let Ok(role) = role_str.parse::<OrgRole>()
    {
        return Ok(role == OrgRole::Owner);
    }

    Ok(false)
}

/// Count active owners in an organization
/// How many active seats an organization holds.
pub async fn count_active_members(pool: &PgPool, organization_id: Uuid) -> DbResult<i64> {
    let count: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM organization_members WHERE organization_id = $1 AND is_active = TRUE",
    )
    .bind(organization_id)
    .fetch_one(pool)
    .await?;

    Ok(count.unwrap_or(0))
}

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
