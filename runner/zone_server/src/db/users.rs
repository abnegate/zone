//! User database queries

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;

/// User row from database
#[derive(Debug, Clone)]
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
    pool: &PgPool,
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
    .fetch_one(pool)
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
        WHERE email = $1
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
pub async fn update_last_login(pool: &PgPool, user_id: Uuid) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE users
        SET last_login_at = NOW(), updated_at = NOW()
        WHERE id = $1
        "#,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Get user with roles and permissions
pub async fn get_user_with_permissions(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Option<UserWithPermissions>> {
    // Get user
    let user = match get_user_by_id(pool, user_id).await? {
        Some(u) => u,
        None => return Ok(None),
    };

    // Admin users get all permissions
    if user.is_admin.unwrap_or(false) {
        let roles = vec!["admin".to_string()];
        let permissions = vec![
            "organizations:create".to_string(),
            "organizations:read".to_string(),
            "organizations:update".to_string(),
            "organizations:delete".to_string(),
            "workspaces:create".to_string(),
            "workspaces:read".to_string(),
            "workspaces:update".to_string(),
            "workspaces:delete".to_string(),
            "projects:create".to_string(),
            "projects:read".to_string(),
            "projects:update".to_string(),
            "projects:delete".to_string(),
            "tasks:create".to_string(),
            "tasks:read".to_string(),
            "tasks:update".to_string(),
            "tasks:delete".to_string(),
            "chats:create".to_string(),
            "chats:read".to_string(),
            "chats:update".to_string(),
            "chats:delete".to_string(),
            "sources:create".to_string(),
            "sources:read".to_string(),
            "sources:update".to_string(),
            "sources:delete".to_string(),
            "models:create".to_string(),
            "models:read".to_string(),
            "models:update".to_string(),
            "models:delete".to_string(),
            "wiki:create".to_string(),
            "wiki:read".to_string(),
            "wiki:update".to_string(),
            "wiki:delete".to_string(),
            "users:create".to_string(),
            "users:read".to_string(),
            "users:update".to_string(),
            "users:delete".to_string(),
        ];
        return Ok(Some(UserWithPermissions {
            user,
            roles,
            permissions,
        }));
    }

    // Get roles
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

    // Get permissions from roles
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
pub async fn assign_user_role(pool: &PgPool, user_id: Uuid, role_name: &str) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO user_roles (user_id, role_id)
        SELECT $1, id FROM roles WHERE name = $2
        ON CONFLICT DO NOTHING
        "#,
        user_id,
        role_name
    )
    .execute(pool)
    .await?;

    Ok(())
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
