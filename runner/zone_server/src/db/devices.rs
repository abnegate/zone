//! Client devices that have signed in to this instance.

use chrono::{DateTime, Utc};
use sqlx::{AssertSqlSafe, PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::DbResult;

const CONNECTED_AFTER: chrono::TimeDelta = chrono::TimeDelta::minutes(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    Ios,
    Desktop,
    Browser,
    Cli,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Desktop => "desktop",
            Self::Browser => "browser",
            Self::Cli => "cli",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "android" => Self::Android,
            "ios" => Self::Ios,
            "desktop" => Self::Desktop,
            "cli" => Self::Cli,
            _ => Self::Browser,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Allowed,
    Pending,
    Blocked,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Pending => "pending",
            Self::Blocked => "blocked",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "blocked" => Self::Blocked,
            _ => Self::Allowed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Open,
    Allowed,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Allowed => "allowed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "allowed" => Self::Allowed,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: Uuid,
    pub user_id: Uuid,
    pub public_id: Uuid,
    pub name: Option<String>,
    pub platform: Platform,
    pub user_agent: Option<String>,
    pub last_ip: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub status: Status,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ListedDevice {
    pub device: Device,
    pub email: String,
    pub display_name: Option<String>,
    pub session_count: i64,
}

#[derive(Debug, Clone)]
pub struct Claim {
    pub public_id: Option<Uuid>,
    pub name: Option<String>,
    pub platform: Option<Platform>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Admission {
    Allowed(Device),
    Pending(Device),
    Blocked(Device),
}

impl Admission {
    pub fn device(&self) -> &Device {
        match self {
            Self::Allowed(device) | Self::Pending(device) | Self::Blocked(device) => device,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct DeviceRow {
    id: Uuid,
    user_id: Uuid,
    public_id: Uuid,
    name: Option<String>,
    platform: String,
    user_agent: Option<String>,
    last_ip: Option<String>,
    last_seen_at: DateTime<Utc>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DeviceRow> for Device {
    fn from(row: DeviceRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            public_id: row.public_id,
            name: row.name,
            platform: Platform::parse(&row.platform),
            user_agent: row.user_agent,
            last_ip: host_ip(row.last_ip),
            last_seen_at: row.last_seen_at,
            status: Status::parse(&row.status),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn host_ip(value: Option<String>) -> Option<String> {
    value.map(|ip| match ip.split_once('/') {
        Some((host, _)) => host.to_string(),
        None => ip,
    })
}

const DEVICE_COLUMNS: &str = r#"
    id,
    user_id,
    public_id,
    name,
    platform,
    user_agent,
    CAST(last_ip AS text) AS "last_ip",
    last_seen_at,
    status,
    created_at,
    updated_at
"#;

pub fn is_connected(last_seen_at: DateTime<Utc>, live: bool) -> bool {
    live || Utc::now() - last_seen_at <= CONNECTED_AFTER
}

pub async fn policy(pool: &PgPool) -> DbResult<Mode> {
    let mode: String = sqlx::query_scalar("SELECT mode FROM device_policy WHERE id = true")
        .fetch_one(pool)
        .await?;
    Ok(Mode::parse(&mode))
}

pub async fn set_policy(pool: &PgPool, mode: Mode) -> DbResult<Mode> {
    let mode: String = sqlx::query_scalar(
        r#"
        UPDATE device_policy
        SET mode = $1, updated_at = NOW()
        WHERE id = true
        RETURNING mode
        "#,
    )
    .bind(mode.as_str())
    .fetch_one(pool)
    .await?;
    Ok(Mode::parse(&mode))
}

pub async fn get(pool: &PgPool, id: Uuid) -> DbResult<Option<Device>> {
    let row: Option<DeviceRow> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {DEVICE_COLUMNS} FROM devices WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Device::from))
}

pub async fn admit(pool: &PgPool, user_id: Uuid, claim: &Claim) -> DbResult<Admission> {
    let mut transaction = pool.begin().await?;
    let mode = {
        let mode: String = sqlx::query_scalar("SELECT mode FROM device_policy WHERE id = true")
            .fetch_one(&mut *transaction)
            .await?;
        Mode::parse(&mode)
    };
    let public_id = claim.public_id.unwrap_or_else(Uuid::new_v4);
    let existing = get_by_public(&mut transaction, user_id, public_id).await?;
    let device = match existing {
        Some(device) => {
            touch_claimed(&mut transaction, device.id, claim).await?;
            let status = match (mode, device.status) {
                (Mode::Open, Status::Pending) => {
                    set_status_in(&mut transaction, device.id, Status::Allowed).await?;
                    Status::Allowed
                }
                (_, status) => status,
            };
            Device { status, ..device }
        }
        None => {
            let status = match mode {
                Mode::Allowed => Status::Pending,
                Mode::Open => Status::Allowed,
            };
            insert(&mut transaction, user_id, public_id, claim, status).await?
        }
    };
    transaction.commit().await?;
    Ok(match device.status {
        Status::Blocked => Admission::Blocked(device),
        Status::Pending if mode == Mode::Allowed => Admission::Pending(device),
        Status::Pending | Status::Allowed => Admission::Allowed(device),
    })
}

pub async fn list_for_organization(
    pool: &PgPool,
    organization_id: Uuid,
) -> DbResult<Vec<ListedDevice>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        user_id: Uuid,
        public_id: Uuid,
        name: Option<String>,
        platform: String,
        user_agent: Option<String>,
        last_ip: Option<String>,
        last_seen_at: DateTime<Utc>,
        status: String,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        email: String,
        display_name: Option<String>,
        session_count: i64,
    }

    let rows: Vec<Row> = sqlx::query_as(
        r#"
        SELECT
            d.id,
            d.user_id,
            d.public_id,
            d.name,
            d.platform,
            d.user_agent,
            CAST(d.last_ip AS text) AS "last_ip",
            d.last_seen_at,
            d.status,
            d.created_at,
            d.updated_at,
            u.email,
            u.display_name,
            (
                SELECT COUNT(*)::bigint
                FROM sessions s
                WHERE s.device_id = d.id
                  AND s.revoked_at IS NULL
                  AND s.expires_at > NOW()
            ) AS session_count
        FROM devices d
        INNER JOIN users u ON u.id = d.user_id
        INNER JOIN organization_members m
            ON m.user_id = d.user_id
           AND m.organization_id = $1
           AND m.is_active
        ORDER BY d.last_seen_at DESC
        "#,
    )
    .bind(organization_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ListedDevice {
            device: Device {
                id: row.id,
                user_id: row.user_id,
                public_id: row.public_id,
                name: row.name,
                platform: Platform::parse(&row.platform),
                user_agent: row.user_agent,
                last_ip: host_ip(row.last_ip),
                last_seen_at: row.last_seen_at,
                status: Status::parse(&row.status),
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            email: row.email,
            display_name: row.display_name,
            session_count: row.session_count,
        })
        .collect())
}

pub async fn belongs_to_organization(
    pool: &PgPool,
    organization_id: Uuid,
    device_id: Uuid,
) -> DbResult<bool> {
    let found: Option<(i32,)> = sqlx::query_as(
        r#"
        SELECT 1
        FROM devices d
        INNER JOIN organization_members m
            ON m.user_id = d.user_id
           AND m.organization_id = $1
           AND m.is_active
        WHERE d.id = $2
        "#,
    )
    .bind(organization_id)
    .bind(device_id)
    .fetch_optional(pool)
    .await?;
    Ok(found.is_some())
}

pub async fn set_status(pool: &PgPool, id: Uuid, status: Status) -> DbResult<Option<Device>> {
    let row: Option<DeviceRow> = sqlx::query_as(AssertSqlSafe(format!(
        r#"
        UPDATE devices
        SET status = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING {DEVICE_COLUMNS}
        "#
    )))
    .bind(id)
    .bind(status.as_str())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Device::from))
}

pub async fn rename(pool: &PgPool, id: Uuid, name: Option<&str>) -> DbResult<Option<Device>> {
    let row: Option<DeviceRow> = sqlx::query_as(AssertSqlSafe(format!(
        r#"
        UPDATE devices
        SET name = $2, updated_at = NOW()
        WHERE id = $1
        RETURNING {DEVICE_COLUMNS}
        "#
    )))
    .bind(id)
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Device::from))
}

pub async fn allowed_admin_count(pool: &PgPool) -> DbResult<i64> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM devices d
        INNER JOIN users u ON u.id = d.user_id
        WHERE d.status = 'allowed'
          AND COALESCE(u.is_admin, false)
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub fn is_last_allowed_admin(
    mode: Mode,
    status: Status,
    is_admin: bool,
    allowed_admin_count: i64,
) -> bool {
    mode == Mode::Allowed && status == Status::Allowed && is_admin && allowed_admin_count <= 1
}

pub async fn is_admin_device(pool: &PgPool, id: Uuid) -> DbResult<bool> {
    let found: Option<(i32,)> = sqlx::query_as(
        r#"
        SELECT 1
        FROM devices d
        INNER JOIN users u ON u.id = d.user_id
        WHERE d.id = $1
          AND COALESCE(u.is_admin, false)
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(found.is_some())
}

pub async fn status_for_session(
    executor: impl sqlx::Executor<'_, Database = Postgres>,
    session_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<Status>> {
    let status: Option<String> = sqlx::query_scalar(
        r#"
        SELECT d.status
        FROM sessions s
        INNER JOIN devices d ON d.id = s.device_id
        WHERE s.id = $1
          AND s.user_id = $2
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await?;
    Ok(status.map(|value| Status::parse(&value)))
}

async fn get_by_public(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    public_id: Uuid,
) -> DbResult<Option<Device>> {
    let row: Option<DeviceRow> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {DEVICE_COLUMNS} FROM devices WHERE user_id = $1 AND public_id = $2"
    )))
    .bind(user_id)
    .bind(public_id)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(Device::from))
}

async fn insert(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    public_id: Uuid,
    claim: &Claim,
    status: Status,
) -> DbResult<Device> {
    let row: DeviceRow = sqlx::query_as(AssertSqlSafe(format!(
        r#"
        INSERT INTO devices (
            user_id,
            public_id,
            name,
            platform,
            user_agent,
            last_ip,
            status
        )
        VALUES ($1, $2, $3, $4, $5, $6::inet, $7)
        RETURNING {DEVICE_COLUMNS}
        "#
    )))
    .bind(user_id)
    .bind(public_id)
    .bind(claim.name.as_deref())
    .bind(claim.platform.unwrap_or(Platform::Browser).as_str())
    .bind(claim.user_agent.as_deref())
    .bind(claim.ip_address.as_deref())
    .bind(status.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(Device::from(row))
}

async fn touch_claimed(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    claim: &Claim,
) -> DbResult<()> {
    sqlx::query(
        r#"
        UPDATE devices
        SET
            name = COALESCE($2, name),
            platform = COALESCE($3, platform),
            user_agent = COALESCE($4, user_agent),
            last_ip = COALESCE($5::inet, last_ip),
            last_seen_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(claim.name.as_deref())
    .bind(claim.platform.map(Platform::as_str))
    .bind(claim.user_agent.as_deref())
    .bind(claim.ip_address.as_deref())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn set_status_in(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    status: Status,
) -> DbResult<()> {
    sqlx::query("UPDATE devices SET status = $2, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .bind(status.as_str())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_falls_back_to_browser() {
        assert_eq!(Platform::parse("android"), Platform::Android);
        assert_eq!(Platform::parse("mystery"), Platform::Browser);
    }

    #[test]
    fn a_device_seen_just_now_is_connected() {
        assert!(is_connected(Utc::now(), false));
        assert!(is_connected(
            Utc::now() - chrono::TimeDelta::minutes(10),
            true
        ));
        assert!(!is_connected(
            Utc::now() - chrono::TimeDelta::minutes(10),
            false
        ));
    }

    #[test]
    fn inet_text_drops_the_prefix_length() {
        assert_eq!(
            host_ip(Some("192.168.4.31/32".into())).as_deref(),
            Some("192.168.4.31")
        );
        assert_eq!(
            host_ip(Some("192.168.4.31".into())).as_deref(),
            Some("192.168.4.31")
        );
    }

    #[test]
    fn the_last_allowed_admin_device_cannot_be_blocked_while_locked() {
        assert!(is_last_allowed_admin(
            Mode::Allowed,
            Status::Allowed,
            true,
            1
        ));
        assert!(!is_last_allowed_admin(
            Mode::Allowed,
            Status::Allowed,
            true,
            2
        ));
        assert!(!is_last_allowed_admin(Mode::Open, Status::Allowed, true, 1));
        assert!(!is_last_allowed_admin(
            Mode::Allowed,
            Status::Pending,
            true,
            1
        ));
        assert!(!is_last_allowed_admin(
            Mode::Allowed,
            Status::Allowed,
            false,
            1
        ));
    }
}
