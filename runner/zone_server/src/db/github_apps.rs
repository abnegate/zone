//! GitHub Apps an organization registered through the app manifest flow.

use std::fmt;

use abnegate_secret::REDACTED;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;
use super::github_installations::UnknownValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerType {
    User,
    Organization,
}

impl OwnerType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Organization => "Organization",
        }
    }
}

impl TryFrom<String> for OwnerType {
    type Error = UnknownValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "User" => Ok(Self::User),
            "Organization" => Ok(Self::Organization),
            _ => Err(UnknownValue),
        }
    }
}

#[derive(Clone, sqlx::FromRow)]
pub struct AppRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub app_id: i64,
    pub slug: String,
    pub name: String,
    pub owner_login: String,
    #[sqlx(try_from = "String")]
    pub owner_type: OwnerType,
    pub html_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub private_key: String,
    pub webhook_secret: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for AppRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppRow")
            .field("id", &self.id)
            .field("organization_id", &self.organization_id)
            .field("app_id", &self.app_id)
            .field("slug", &self.slug)
            .field("name", &self.name)
            .field("owner_login", &self.owner_login)
            .field("owner_type", &self.owner_type)
            .field("html_url", &self.html_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &REDACTED)
            .field("private_key", &REDACTED)
            .field(
                "webhook_secret",
                &self.webhook_secret.as_ref().map(|_| REDACTED),
            )
            .field("created_by", &self.created_by)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// `client_secret`, `private_key` and `webhook_secret` arrive already sealed.
pub struct NewApp<'a> {
    pub organization_id: Uuid,
    pub app_id: i64,
    pub slug: &'a str,
    pub name: &'a str,
    pub owner_login: &'a str,
    pub owner_type: OwnerType,
    pub html_url: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub private_key: &'a str,
    pub webhook_secret: Option<&'a str>,
    pub created_by: Option<Uuid>,
}

const COLUMNS: &str = "id, organization_id, app_id, slug, name, owner_login, owner_type, html_url, \
     client_id, client_secret, private_key, webhook_secret, created_by, created_at, updated_at";

/// A second app for an owner the organization already registered fails with a
/// unique violation on `github_apps_organization_owner_idx`.
pub async fn insert(pool: &PgPool, app: &NewApp<'_>) -> DbResult<AppRow> {
    sqlx::query_as::<_, AppRow>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO github_apps \
           (organization_id, app_id, slug, name, owner_login, owner_type, html_url, \
            client_id, client_secret, private_key, webhook_secret, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         RETURNING {COLUMNS}"
    )))
    .bind(app.organization_id)
    .bind(app.app_id)
    .bind(app.slug)
    .bind(app.name)
    .bind(app.owner_login)
    .bind(app.owner_type.as_str())
    .bind(app.html_url)
    .bind(app.client_id)
    .bind(app.client_secret)
    .bind(app.private_key)
    .bind(app.webhook_secret)
    .bind(app.created_by)
    .fetch_one(pool)
    .await
}

pub async fn list(pool: &PgPool, organization_id: Uuid) -> DbResult<Vec<AppRow>> {
    sqlx::query_as::<_, AppRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM github_apps WHERE organization_id = $1 ORDER BY created_at, id"
    )))
    .bind(organization_id)
    .fetch_all(pool)
    .await
}

pub async fn get(pool: &PgPool, organization_id: Uuid, id: Uuid) -> DbResult<Option<AppRow>> {
    sqlx::query_as::<_, AppRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM github_apps WHERE organization_id = $1 AND id = $2"
    )))
    .bind(organization_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn for_owner(
    pool: &PgPool,
    organization_id: Uuid,
    owner_login: &str,
) -> DbResult<Option<AppRow>> {
    sqlx::query_as::<_, AppRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM github_apps \
         WHERE organization_id = $1 AND lower(owner_login) = lower($2)"
    )))
    .bind(organization_id)
    .bind(owner_login)
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, organization_id: Uuid, id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM github_apps WHERE organization_id = $1 AND id = $2")
        .bind(organization_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("connect to the test database")
    }

    pub(crate) async fn seed_organization(pool: &PgPool) -> Uuid {
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'GitHub', $1::text)")
            .bind(organization)
            .execute(pool)
            .await
            .expect("seed organization");
        organization
    }

    pub(crate) async fn seed_user(pool: &PgPool) -> Uuid {
        let user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash) \
             VALUES ($1, $1::text || '@zone.test', 'unused')",
        )
        .bind(user)
        .execute(pool)
        .await
        .expect("seed user");
        user
    }

    pub(crate) async fn seed_workspace(pool: &PgPool, organization: Uuid) -> Uuid {
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) \
             VALUES ($1, $2, 'GitHub', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(pool)
        .await
        .expect("seed workspace");
        workspace
    }

    pub(crate) async fn seed_app(pool: &PgPool, organization: Uuid, owner_login: &str) -> AppRow {
        insert(
            pool,
            &NewApp {
                organization_id: organization,
                app_id: rand::random_range(1..i64::MAX),
                slug: "zone-app",
                name: "Zone",
                owner_login,
                owner_type: OwnerType::Organization,
                html_url: "https://github.com/apps/zone-app",
                client_id: "Iv1.client",
                client_secret: "ENC[v1:client-secret]",
                private_key: "ENC[v1:private-key]",
                webhook_secret: Some("ENC[v1:webhook-secret]"),
                created_by: None,
            },
        )
        .await
        .expect("seed app")
    }

    #[test]
    fn owner_type_round_trips_and_refuses_unknown_values() {
        for owner_type in [OwnerType::User, OwnerType::Organization] {
            assert_eq!(
                OwnerType::try_from(owner_type.as_str().to_owned()).unwrap(),
                owner_type
            );
        }
        assert!(OwnerType::try_from("Enterprise".to_owned()).is_err());
        assert!(OwnerType::try_from("user".to_owned()).is_err());
    }

    #[tokio::test]
    async fn insert_round_trips_every_column() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let user = seed_user(&pool).await;
        let app = insert(
            &pool,
            &NewApp {
                organization_id: organization,
                app_id: 4242,
                slug: "zone-acme",
                name: "Zone (acme)",
                owner_login: "acme",
                owner_type: OwnerType::User,
                html_url: "https://github.com/apps/zone-acme",
                client_id: "Iv1.acme",
                client_secret: "ENC[v1:secret]",
                private_key: "ENC[v1:key]",
                webhook_secret: None,
                created_by: Some(user),
            },
        )
        .await
        .unwrap();

        assert_eq!(app.organization_id, organization);
        assert_eq!(app.app_id, 4242);
        assert_eq!(app.slug, "zone-acme");
        assert_eq!(app.name, "Zone (acme)");
        assert_eq!(app.owner_login, "acme");
        assert_eq!(app.owner_type, OwnerType::User);
        assert_eq!(app.html_url, "https://github.com/apps/zone-acme");
        assert_eq!(app.client_id, "Iv1.acme");
        assert_eq!(app.client_secret, "ENC[v1:secret]");
        assert_eq!(app.private_key, "ENC[v1:key]");
        assert_eq!(app.webhook_secret, None);
        assert_eq!(app.created_by, Some(user));

        let fetched = get(&pool, organization, app.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, app.id);
        assert_eq!(fetched.owner_type, OwnerType::User);
    }

    #[tokio::test]
    async fn reads_and_deletes_are_scoped_to_the_organization() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let other = seed_organization(&pool).await;
        let first = seed_app(&pool, organization, "first").await;
        let second = seed_app(&pool, organization, "second").await;
        seed_app(&pool, other, "first").await;

        let listed: Vec<Uuid> = list(&pool, organization)
            .await
            .unwrap()
            .into_iter()
            .map(|app| app.id)
            .collect();
        assert_eq!(listed, vec![first.id, second.id]);
        assert!(get(&pool, other, first.id).await.unwrap().is_none());
        assert!(!delete(&pool, other, first.id).await.unwrap());
        assert!(get(&pool, organization, first.id).await.unwrap().is_some());
        assert!(delete(&pool, organization, first.id).await.unwrap());
        assert!(get(&pool, organization, first.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn for_owner_matches_the_login_case_insensitively() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "Acme-Corp").await;

        let found = for_owner(&pool, organization, "acme-corp").await.unwrap();
        assert_eq!(found.map(|row| row.id), Some(app.id));
        let missing = for_owner(&pool, organization, "other").await.unwrap();
        assert!(missing.is_none());
        let elsewhere = seed_organization(&pool).await;
        let foreign = for_owner(&pool, elsewhere, "Acme-Corp").await.unwrap();
        assert!(foreign.is_none());
    }

    #[tokio::test]
    async fn one_app_per_owner_regardless_of_case() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        seed_app(&pool, organization, "Acme").await;

        let duplicate = insert(
            &pool,
            &NewApp {
                organization_id: organization,
                app_id: 7,
                slug: "zone-acme-2",
                name: "Zone",
                owner_login: "acme",
                owner_type: OwnerType::Organization,
                html_url: "https://github.com/apps/zone-acme-2",
                client_id: "Iv1.other",
                client_secret: "ENC[v1:a]",
                private_key: "ENC[v1:b]",
                webhook_secret: None,
                created_by: None,
            },
        )
        .await
        .expect_err("a case-only variant of an existing owner must be refused");
        let constraint = duplicate
            .as_database_error()
            .and_then(|error| error.constraint().map(str::to_owned));
        assert_eq!(
            constraint.as_deref(),
            Some("github_apps_organization_owner_idx")
        );

        seed_app(&pool, organization, "globex").await;
        assert_eq!(list(&pool, organization).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn debug_never_shows_a_secret() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        let app = seed_app(&pool, organization, "redaction").await;

        let rendered = format!("{app:?}");
        assert!(!rendered.contains("client-secret"), "{rendered}");
        assert!(!rendered.contains("private-key"), "{rendered}");
        assert!(!rendered.contains("webhook-secret"), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
        assert!(rendered.contains("redaction"), "{rendered}");
    }
}
