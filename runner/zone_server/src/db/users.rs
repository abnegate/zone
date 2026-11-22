//! User database queries

use chrono::NaiveDateTime;
use sqlx::{PgConnection, PgExecutor, PgPool};
use uuid::Uuid;
use zone_enterprise::sso::Account;

use super::DbResult;
use crate::services::time_zone::{self, TimeZone};

pub const EMAIL_KEY: &str = "users_email_key";
pub const EMAIL_LOWER_KEY: &str = "users_email_lower_key";

const ADMINISTRATOR_ROLE: &str = "admin";

const ADMINISTRATOR_PERMISSIONS: [&str; 40] = [
    "organizations:create",
    "organizations:read",
    "organizations:update",
    "organizations:delete",
    "workspaces:create",
    "workspaces:read",
    "workspaces:update",
    "workspaces:delete",
    "projects:create",
    "projects:read",
    "projects:update",
    "projects:delete",
    "tasks:create",
    "tasks:read",
    "tasks:update",
    "tasks:delete",
    "chats:create",
    "chats:read",
    "chats:update",
    "chats:delete",
    "sources:create",
    "sources:read",
    "sources:update",
    "sources:delete",
    "models:create",
    "models:read",
    "models:update",
    "models:delete",
    "wiki:create",
    "wiki:read",
    "wiki:update",
    "wiki:delete",
    "users:create",
    "users:read",
    "users:update",
    "users:delete",
    "pull_requests:read",
    "pull_requests:update",
    "releases:read",
    "releases:create",
];

/// User row from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub is_active: Option<bool>,
    pub is_admin: Option<bool>,
    pub email_verified: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub last_login_at: Option<NaiveDateTime>,
}

impl UserRow {
    pub fn account(&self, pristine: bool) -> Account {
        Account {
            id: self.id,
            active: self.is_active.unwrap_or(true),
            email_verified: self.email_verified,
            admin: self.is_admin.unwrap_or(false),
            pristine,
        }
    }
}

/// User with password hash (for authentication)
#[derive(Debug, Clone)]
pub struct UserWithPasswordRow {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub is_active: Option<bool>,
    pub is_admin: Option<bool>,
    pub email_verified: bool,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub last_login_at: Option<NaiveDateTime>,
    pub password_hash: String,
}

/// User with roles and permissions
#[derive(Debug, Clone)]
pub struct UserWithPermissions {
    pub user: UserRow,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

/// Count total users
pub async fn count_users(pool: &PgPool) -> DbResult<i64> {
    let result = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    Ok(result.unwrap_or(0))
}

/// Create a new user
pub async fn create_user(
    executor: impl PgExecutor<'_>,
    email: &str,
    password_hash: &str,
    display_name: Option<&str>,
    is_admin: bool,
) -> DbResult<UserRow> {
    let row = sqlx::query!(
        r#"
        INSERT INTO users (email, password_hash, display_name, is_admin)
        VALUES ($1, $2, $3, $4)
        RETURNING id, email, display_name, is_active, is_admin, email_verified,
                  created_at, updated_at, last_login_at
        "#,
        email,
        password_hash,
        display_name,
        is_admin
    )
    .fetch_one(executor)
    .await?;

    Ok(UserRow {
        id: row.id,
        email: row.email,
        display_name: row.display_name,
        is_active: row.is_active,
        is_admin: row.is_admin,
        email_verified: row.email_verified,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_login_at: row.last_login_at,
    })
}

/// Get user by email (for login)
pub async fn get_user_by_email(
    pool: &PgPool,
    email: &str,
) -> DbResult<Option<UserWithPasswordRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, email, display_name, is_active, is_admin, email_verified,
               created_at, updated_at, last_login_at, password_hash
        FROM users
        WHERE lower(email) = lower($1)
        "#,
        email
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| UserWithPasswordRow {
        id: r.id,
        email: r.email,
        display_name: r.display_name,
        is_active: r.is_active,
        is_admin: r.is_admin,
        email_verified: r.email_verified,
        created_at: r.created_at,
        updated_at: r.updated_at,
        last_login_at: r.last_login_at,
        password_hash: r.password_hash,
    }))
}

/// Get user by ID
pub async fn get_user_by_id(pool: &PgPool, id: Uuid) -> DbResult<Option<UserRow>> {
    let row = sqlx::query!(
        r#"
        SELECT id, email, display_name, is_active, is_admin, email_verified,
               created_at, updated_at, last_login_at
        FROM users
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| UserRow {
        id: r.id,
        email: r.email,
        display_name: r.display_name,
        is_active: r.is_active,
        is_admin: r.is_admin,
        email_verified: r.email_verified,
        created_at: r.created_at,
        updated_at: r.updated_at,
        last_login_at: r.last_login_at,
    }))
}

/// Update last login timestamp
pub async fn update_last_login(executor: impl PgExecutor<'_>, user_id: Uuid) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE users
        SET last_login_at = NOW(), updated_at = NOW()
        WHERE id = $1
        "#,
        user_id
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// Get user with roles and permissions
pub async fn get_user_with_permissions(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Option<UserWithPermissions>> {
    let Some(user) = get_user_by_id(pool, user_id).await? else {
        return Ok(None);
    };

    if user.is_admin.unwrap_or(false) {
        return Ok(Some(UserWithPermissions {
            user,
            roles: vec![ADMINISTRATOR_ROLE.to_string()],
            permissions: ADMINISTRATOR_PERMISSIONS
                .iter()
                .map(ToString::to_string)
                .collect(),
        }));
    }

    let roles: Vec<String> = sqlx::query_scalar!(
        r#"
        SELECT r.name
        FROM user_roles ur
        JOIN roles r ON ur.role_id = r.id
        WHERE ur.user_id = $1
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let permissions: Vec<String> = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT p.name
        FROM user_roles ur
        JOIN role_permissions rp ON ur.role_id = rp.role_id
        JOIN permissions p ON rp.permission_id = p.id
        WHERE ur.user_id = $1
        "#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(Some(UserWithPermissions {
        user,
        roles,
        permissions,
    }))
}

/// Assign a role to a user
pub async fn assign_user_role(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    role_name: &str,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO user_roles (user_id, role_id)
        SELECT $1, id FROM roles WHERE name = $2
        ON CONFLICT DO NOTHING
        "#,
        user_id,
        role_name
    )
    .execute(executor)
    .await?;

    Ok(())
}

pub async fn find_by_email_ignoring_case(
    executor: impl PgExecutor<'_>,
    email: &str,
) -> DbResult<Vec<UserRow>> {
    sqlx::query_as(
        "SELECT id, email, display_name, is_active, is_admin, email_verified, \
                created_at, updated_at, last_login_at \
         FROM users WHERE lower(email) = lower($1) ORDER BY created_at, id",
    )
    .bind(email)
    .fetch_all(executor)
    .await
}

/// Locks the account `FOR UPDATE` until the caller's transaction ends, so its password and last
/// sign-in hold still meanwhile. `false` when the account is gone.
pub async fn lock(connection: &mut PgConnection, user_id: Uuid) -> DbResult<bool> {
    let locked: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .fetch_optional(&mut *connection)
        .await?;
    Ok(locked.is_some())
}

/// The row stays locked `FOR UPDATE` until the caller's transaction ends, so no session or
/// refresh token can be inserted for the account meanwhile. `false` when the account is gone, or
/// was verified or made an admin after the sign-in resolved it.
pub async fn reclaim(
    connection: &mut PgConnection,
    user_id: Uuid,
    password_hash: &str,
) -> DbResult<bool> {
    let reclaimable: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users \
         WHERE id = $1 AND email_verified IS NOT TRUE AND is_admin IS NOT TRUE \
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *connection)
    .await?;
    if reclaimable.is_none() {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE users SET password_hash = $1, email_verified = TRUE, updated_at = NOW() WHERE id = $2",
    )
    .bind(password_hash)
    .bind(user_id)
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

/// The account's password hash, share-locked until the caller's transaction ends so a reclaim
/// can't replace it underneath a sign-in that verified it.
pub async fn lock_password_hash(
    connection: &mut PgConnection,
    user_id: Uuid,
) -> DbResult<Option<String>> {
    sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1 FOR SHARE")
        .bind(user_id)
        .fetch_optional(connection)
        .await
}

/// Set user active status
pub async fn set_user_active(pool: &PgPool, user_id: Uuid, is_active: bool) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE users
        SET is_active = $1, updated_at = NOW()
        WHERE id = $2
        "#,
        is_active,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Delete a user (for testing)
pub async fn delete_user(pool: &PgPool, user_id: Uuid) -> DbResult<()> {
    sqlx::query!("DELETE FROM users WHERE id = $1", user_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// The zone the person's console last reported, or `None` when it never has
/// or what it reported is no longer a zone this build knows.
pub async fn time_zone(executor: impl PgExecutor<'_>, user_id: Uuid) -> DbResult<Option<TimeZone>> {
    let stored: Option<String> = sqlx::query_scalar("SELECT time_zone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(executor)
        .await?
        .flatten();
    Ok(stored.and_then(|name| time_zone::parse(&name).ok()))
}

pub async fn set_time_zone(pool: &PgPool, user_id: Uuid, zone: TimeZone) -> DbResult<()> {
    sqlx::query("UPDATE users SET time_zone = $2 WHERE id = $1 AND time_zone IS DISTINCT FROM $2")
        .bind(user_id)
        .bind(zone.name())
        .execute(pool)
        .await?;
    Ok(())
}
