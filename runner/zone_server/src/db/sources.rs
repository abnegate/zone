//! Source database queries

use std::fmt;

use abnegate_secret::REDACTED;
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::{PgExecutor, PgPool};
use thiserror::Error;
use uuid::Uuid;
use zone_context::adapters::GitLabAdapter;

use super::DbResult;
use super::github_installations::UNATTACHABLE;

pub const GITHUB: &str = "github";

pub const GITLAB: &str = "gitlab";

pub const FILESYSTEM: &str = "filesystem";

pub const ICAL: &str = "ical";

pub const IMAP: &str = "imap";

pub const DISCORD: &str = "discord";

pub const SLACK: &str = "slack";

pub const WEB: &str = "web";

pub const TEXT: &str = "text";

pub const NOTION: &str = "notion";

pub const GOOGLE_CALENDAR: &str = "google_calendar";

pub const GOOGLE_MAIL: &str = "google_mail";

const INSTANCE_NEEDS_TOKEN: &str =
    "Re-enter the access token to read this source from another GitLab instance.";

/// Source row from database
#[derive(Clone, sqlx::FromRow)]
pub struct SourceRow {
    pub id: Uuid,
    pub name: String,
    pub source_type: String,
    pub config: serde_json::Value,
    pub credentials_encrypted: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub is_active: Option<bool>,
    pub last_verified_at: Option<NaiveDateTime>,
    pub last_error: Option<String>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub workspace_id: Option<Uuid>,
    pub github_installation_id: Option<Uuid>,
}

impl fmt::Debug for SourceRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("source_type", &self.source_type)
            .field("config", &self.config)
            .field(
                "credentials_encrypted",
                &self.credentials_encrypted.as_ref().map(|_| REDACTED),
            )
            .field("description", &self.description)
            .field("url", &self.url)
            .field("is_active", &self.is_active)
            .field("last_verified_at", &self.last_verified_at)
            .field("last_error", &self.last_error)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("workspace_id", &self.workspace_id)
            .field("github_installation_id", &self.github_installation_id)
            .finish()
    }
}

/// List sources with optional filters
pub async fn list_sources(
    pool: &PgPool,
    workspace_id: Uuid,
    source_type: Option<&str>,
    is_active: Option<bool>,
    limit: i64,
    offset: i64,
) -> DbResult<Vec<SourceRow>> {
    match (source_type, is_active) {
        (Some(t), Some(a)) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                       github_installation_id
                FROM sources
                WHERE workspace_id = $1 AND source_type = $2 AND is_active = $3
                ORDER BY created_at DESC
                LIMIT $4 OFFSET $5
                "#,
            )
            .bind(workspace_id)
            .bind(t)
            .bind(a)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (Some(t), None) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                       github_installation_id
                FROM sources
                WHERE workspace_id = $1 AND source_type = $2
                ORDER BY created_at DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(workspace_id)
            .bind(t)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, Some(a)) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                       github_installation_id
                FROM sources
                WHERE workspace_id = $1 AND is_active = $2
                ORDER BY created_at DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(workspace_id)
            .bind(a)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, SourceRow>(
                r#"
                SELECT id, name, source_type, config, credentials_encrypted, description, url,
                       is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                       github_installation_id
                FROM sources
                WHERE workspace_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(workspace_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }
}

/// Whether the workspace has an active source of `source_type`. A source whose
/// `is_active` was never set counts as active, as `get_source`'s callers read it.
pub async fn has_active(pool: &PgPool, workspace_id: Uuid, source_type: &str) -> DbResult<bool> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM sources
            WHERE workspace_id = $1 AND source_type = $2 AND COALESCE(is_active, TRUE)
        )
        "#,
    )
    .bind(workspace_id)
    .bind(source_type)
    .fetch_one(pool)
    .await
}

/// Get source by ID
pub async fn get_source(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
               github_installation_id
        FROM sources
        WHERE id = $1 AND workspace_id = $2
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

/// The workspace's oldest active source of `source_type` that holds a
/// credential for the repository `owner/repo`, matched without case as GitHub
/// matches names.
pub async fn get_repository_source(
    pool: &PgPool,
    workspace_id: Uuid,
    source_type: &str,
    owner: &str,
    repo: &str,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
               github_installation_id
        FROM sources
        WHERE workspace_id = $1 AND source_type = $2 AND is_active IS NOT FALSE
          AND (credentials_encrypted IS NOT NULL OR github_installation_id IS NOT NULL)
          AND lower(config->>'owner') = lower($3) AND lower(config->>'repo') = lower($4)
        ORDER BY created_at, id
        LIMIT 1
        "#,
    )
    .bind(workspace_id)
    .bind(source_type)
    .bind(owner)
    .bind(repo)
    .fetch_optional(pool)
    .await
}

/// Everything a source is created with
#[derive(Clone)]
pub struct NewSource<'a> {
    pub workspace_id: Uuid,
    pub name: &'a str,
    pub source_type: &'a str,
    pub config: serde_json::Value,
    pub description: Option<&'a str>,
    pub url: Option<&'a str>,
    pub credentials_encrypted: Option<&'a str>,
    pub github_installation_id: Option<Uuid>,
}

/// Create a new source. An installation outside the workspace's
/// organization, revoked or suspended, creates nothing.
pub async fn create_source(pool: &PgPool, source: NewSource<'_>) -> Result<SourceRow, UpdateError> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        INSERT INTO sources (workspace_id, name, source_type, config, description, url,
                             credentials_encrypted, github_installation_id)
        SELECT $1, $2, $3, $4, $5, $6, $7, $8
        WHERE $8::uuid IS NULL OR EXISTS (
            SELECT 1 FROM github_installations gi
            JOIN workspaces w ON w.organization_id = gi.organization_id
            WHERE gi.id = $8 AND w.id = $1
              AND gi.revoked_at IS NULL AND gi.suspended_at IS NULL
        )
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                  github_installation_id
        "#,
    )
    .bind(source.workspace_id)
    .bind(source.name)
    .bind(source.source_type)
    .bind(source.config)
    .bind(source.description)
    .bind(source.url)
    .bind(source.credentials_encrypted)
    .bind(source.github_installation_id)
    .fetch_optional(pool)
    .await
    .map_err(refused_installation)?
    .ok_or(UpdateError::Unattachable)
}

/// What an update changes; a field left `None` keeps its stored value.
/// `github_installation_id` is `Some(None)` to detach the installation.
#[derive(Default)]
pub struct SourceChanges<'a> {
    pub name: Option<&'a str>,
    pub config: Option<serde_json::Value>,
    pub description: Option<&'a str>,
    pub url: Option<&'a str>,
    pub is_active: Option<bool>,
    pub github_installation_id: Option<Option<Uuid>>,
    pub credentials_encrypted: Option<&'a str>,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("The source does not exist in this workspace.")]
    NotFound,
    #[error("{UNATTACHABLE}")]
    Unattachable,
    #[error("{INSTANCE_NEEDS_TOKEN}")]
    InstanceNeedsToken,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

fn refused_installation(error: sqlx::Error) -> UpdateError {
    match &error {
        sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
            UpdateError::Unattachable
        }
        _ => UpdateError::Database(error),
    }
}

/// Apply every change to a source or none of them. A GitLab source holding a
/// token moves to another instance only with a token entered for it, judged
/// against the row as it is locked for the write.
pub async fn update_source(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    changes: SourceChanges<'_>,
) -> Result<SourceRow, UpdateError> {
    let mut transaction = pool.begin().await?;

    let locked = lock_source(&mut *transaction, id, workspace_id)
        .await?
        .ok_or(UpdateError::NotFound)?;
    if moves_stored_token(&locked, &changes) {
        return Err(UpdateError::InstanceNeedsToken);
    }

    let mut current = update_fields(&mut *transaction, id, workspace_id, &changes)
        .await?
        .ok_or(UpdateError::NotFound)?;

    if let Some(installation) = changes.github_installation_id {
        // The row is locked, so no row here means the installation was refused.
        current = set_github_installation(&mut *transaction, id, workspace_id, installation)
            .await
            .map_err(refused_installation)?
            .ok_or(UpdateError::Unattachable)?;
    }

    if let Some(sealed) = changes.credentials_encrypted {
        current = update_credentials(&mut *transaction, id, workspace_id, sealed)
            .await?
            .ok_or(UpdateError::NotFound)?;
    }

    transaction.commit().await?;
    Ok(current)
}

async fn lock_source<'e>(
    executor: impl PgExecutor<'e>,
    id: Uuid,
    workspace_id: Uuid,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
               github_installation_id
        FROM sources
        WHERE id = $1 AND workspace_id = $2
        FOR NO KEY UPDATE
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(executor)
    .await
}

fn moves_stored_token(stored: &SourceRow, changes: &SourceChanges<'_>) -> bool {
    stored.source_type == GITLAB
        && stored.credentials_encrypted.is_some()
        && changes.credentials_encrypted.is_none()
        && changes.config.as_ref().is_some_and(|config| {
            !matches!(
                (
                    GitLabAdapter::instance_of(&stored.config),
                    GitLabAdapter::instance_of(config),
                ),
                (Ok(stored), Ok(changed)) if stored == changed
            )
        })
}

async fn update_fields<'e>(
    executor: impl PgExecutor<'e>,
    id: Uuid,
    workspace_id: Uuid,
    changes: &SourceChanges<'_>,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        UPDATE sources
        SET name = COALESCE($3, name),
            config = COALESCE($4, config),
            description = COALESCE($5, description),
            url = COALESCE($6, url),
            is_active = COALESCE($7, is_active),
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                  github_installation_id
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(changes.name)
    .bind(changes.config.as_ref())
    .bind(changes.description)
    .bind(changes.url)
    .bind(changes.is_active)
    .fetch_optional(executor)
    .await
}

/// Delete a source
pub async fn delete_source(pool: &PgPool, id: Uuid, workspace_id: Uuid) -> DbResult<bool> {
    let result = sqlx::query("DELETE FROM sources WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(workspace_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Update verification status
pub async fn update_verification(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    last_error: Option<&str>,
) -> DbResult<bool> {
    let result = sqlx::query(
        r#"
        UPDATE sources
        SET last_verified_at = NOW(),
            last_error = $3,
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(last_error)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Store a personal access token, detaching any GitHub installation
pub async fn update_credentials<'e>(
    executor: impl PgExecutor<'e>,
    id: Uuid,
    workspace_id: Uuid,
    credentials_encrypted: &str,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        UPDATE sources
        SET credentials_encrypted = $3,
            github_installation_id = NULL,
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                  github_installation_id
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(credentials_encrypted)
    .fetch_optional(executor)
    .await
}

/// Attach a GitHub installation, dropping any stored token, or detach one.
/// An installation outside the workspace's organization, revoked or
/// suspended, attaches nothing and returns no row.
pub async fn set_github_installation<'e>(
    executor: impl PgExecutor<'e>,
    id: Uuid,
    workspace_id: Uuid,
    github_installation_id: Option<Uuid>,
) -> DbResult<Option<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        UPDATE sources
        SET github_installation_id = $3,
            credentials_encrypted = CASE WHEN $3::uuid IS NULL THEN credentials_encrypted END,
            updated_at = NOW()
        WHERE id = $1 AND workspace_id = $2
          AND ($3::uuid IS NULL OR EXISTS (
              SELECT 1 FROM github_installations gi
              JOIN workspaces w ON w.organization_id = gi.organization_id
              WHERE gi.id = $3 AND w.id = $2
                AND gi.revoked_at IS NULL AND gi.suspended_at IS NULL
          ))
        RETURNING id, name, source_type, config, credentials_encrypted, description, url,
                  is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
                  github_installation_id
        "#,
    )
    .bind(id)
    .bind(workspace_id)
    .bind(github_installation_id)
    .fetch_optional(executor)
    .await
}

/// Get multiple sources by their IDs
///
/// Returns sources in arbitrary order. Sources that don't exist are silently skipped.
/// CRITICAL: Requires workspace_id to prevent authorization bypass.
pub async fn get_sources_by_ids(
    pool: &PgPool,
    ids: &[Uuid],
    workspace_id: Uuid,
) -> DbResult<Vec<SourceRow>> {
    sqlx::query_as::<_, SourceRow>(
        r#"
        SELECT id, name, source_type, config, credentials_encrypted, description, url,
               is_active, last_verified_at, last_error, created_at, updated_at, workspace_id,
               github_installation_id
        FROM sources
        WHERE id = ANY($1) AND workspace_id = $2 AND is_active = true
        "#,
    )
    .bind(ids)
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

/// Index status for a source
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum IndexStatus {
    Pending,  // Never indexed
    Indexing, // Currently being indexed
    Indexed,  // Successfully indexed
    Failed,   // Last index attempt failed
    Stale,    // Config changed since last index (not implemented yet)
}

/// Source indexing status information
#[derive(Debug, Clone)]
pub struct SourceIndexStatus {
    pub status: IndexStatus,
    pub last_indexed_at: Option<NaiveDateTime>,
    pub indexed_items_count: Option<i64>,
}

/// Context gathering row (minimal, for index status queries)
#[derive(Debug, sqlx::FromRow)]
struct GatheringRow {
    pub status: String,
    pub completed_at: Option<NaiveDateTime>,
}

/// Get indexing status for a source
pub async fn get_source_index_status(
    pool: &PgPool,
    source_id: Uuid,
) -> DbResult<SourceIndexStatus> {
    // Get latest gathering for this source
    let gathering = sqlx::query_as::<_, GatheringRow>(
        r#"
        SELECT status, completed_at
        FROM context_gatherings
        WHERE $1 = ANY(source_ids)
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?;

    let item_count: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT content_items.id)
        FROM content_items
        INNER JOIN content_chunks ON content_chunks.content_item_id = content_items.id
        INNER JOIN embeddings ON embeddings.chunk_id = content_chunks.id
        WHERE content_items.source_id = $1
        "#,
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?;

    let searchable = item_count.unwrap_or(0) > 0;
    let status = match gathering {
        Some(ref g) if g.status == "running" => IndexStatus::Indexing,
        Some(ref g) if g.status == "failed" => IndexStatus::Failed,
        Some(ref g) if g.status == "completed" && searchable => IndexStatus::Indexed,
        Some(ref g) if g.status == "completed" => IndexStatus::Failed,
        _ if searchable => IndexStatus::Indexed,
        _ => IndexStatus::Pending,
    };

    Ok(SourceIndexStatus {
        status,
        last_indexed_at: gathering.and_then(|g| g.completed_at),
        indexed_items_count: item_count,
    })
}

/// Active source that the resync worker can consider
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActiveIndexSource {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub source_type: String,
    pub name: String,
    pub config: serde_json::Value,
    pub credentials_encrypted: Option<String>,
    pub is_active: Option<bool>,
    pub last_verified_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
    pub sync_version: Option<String>,
    pub last_sync_at: Option<NaiveDateTime>,
    pub user_id: Option<Uuid>,
    pub github_installation_id: Option<Uuid>,
}

impl ActiveIndexSource {
    pub fn as_source_row(&self) -> SourceRow {
        SourceRow {
            id: self.id,
            name: self.name.clone(),
            source_type: self.source_type.clone(),
            config: self.config.clone(),
            credentials_encrypted: self.credentials_encrypted.clone(),
            description: None,
            url: None,
            is_active: self.is_active,
            last_verified_at: self.last_verified_at,
            last_error: None,
            created_at: self.created_at,
            updated_at: self.updated_at,
            workspace_id: Some(self.workspace_id),
            github_installation_id: self.github_installation_id,
        }
    }
}

/// All active workspace sources for scheduled / change-driven reindex
pub async fn list_active_index_sources(pool: &PgPool) -> DbResult<Vec<ActiveIndexSource>> {
    sqlx::query_as::<_, ActiveIndexSource>(
        r#"
        SELECT
            s.id,
            s.workspace_id,
            s.source_type,
            s.name,
            s.config,
            s.credentials_encrypted,
            s.is_active,
            s.last_verified_at,
            s.created_at,
            s.updated_at,
            ss.version AS sync_version,
            ss.last_sync_at,
            s.github_installation_id,
            (
                SELECT wm.user_id
                FROM workspace_members wm
                WHERE wm.workspace_id = s.workspace_id
                  AND wm.is_active
                  AND wm.role IN ('owner', 'admin', 'member')
                ORDER BY
                    CASE wm.role
                        WHEN 'owner' THEN 0
                        WHEN 'admin' THEN 1
                        ELSE 2
                    END,
                    wm.created_at
                LIMIT 1
            ) AS user_id
        FROM sources s
        LEFT JOIN source_sync_state ss ON ss.source_id = s.id
        WHERE COALESCE(s.is_active, TRUE) = TRUE
          AND s.workspace_id IS NOT NULL
        "#,
    )
    .fetch_all(pool)
    .await
}

/// Content items that have text but no embeddings (failed or partial index)
pub async fn count_items_missing_embeddings(pool: &PgPool, source_id: Uuid) -> DbResult<i64> {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM content_items ci
        WHERE ci.source_id = $1
          AND COALESCE(ci.metadata_only, FALSE) = FALSE
          AND NOT EXISTS (
              SELECT 1 FROM embeddings e WHERE e.content_item_id = ci.id
          )
        "#,
    )
    .bind(source_id)
    .fetch_one(pool)
    .await
}

#[cfg(test)]
pub(crate) mod testing {
    use sqlx::PgPool;
    use uuid::Uuid;

    pub struct Installation {
        pub workspace: Uuid,
        pub id: Uuid,
    }

    pub async fn pool() -> PgPool {
        PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
            .await
            .expect("the test database")
    }

    /// A new organization and workspace, with one active GitHub installation for the organization.
    pub async fn installation(pool: &PgPool) -> Installation {
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations (id, name, slug) VALUES ($1, 'GitHub installation', $1::text)",
        )
        .bind(organization)
        .execute(pool)
        .await
        .expect("an organization");
        sqlx::query(
            "INSERT INTO workspaces (id, organization_id, name, slug) \
             VALUES ($1, $2, 'GitHub installation', $1::text)",
        )
        .bind(workspace)
        .bind(organization)
        .execute(pool)
        .await
        .expect("a workspace");
        let application: Uuid = sqlx::query_scalar(
            "INSERT INTO github_apps (organization_id, app_id, slug, name, owner_login, owner_type, \
               html_url, client_id, client_secret, private_key) \
             VALUES ($1, 1, 'zone', 'Zone', 'acme', 'Organization', 'https://github.com/apps/zone', \
               'Iv1.client', 'ENC[v1:client]', 'ENC[v1:key]') \
             RETURNING id",
        )
        .bind(organization)
        .fetch_one(pool)
        .await
        .expect("a GitHub App");
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO github_installations (organization_id, github_app_id, installation_id, \
               account_login, account_type, repository_selection, html_url) \
             VALUES ($1, $2, $3, 'acme', 'Organization', 'all', \
               'https://github.com/organizations/acme/settings/installations/7') \
             RETURNING id",
        )
        .bind(organization)
        .bind(application)
        .bind(rand::random_range(1..i64::MAX))
        .fetch_one(pool)
        .await
        .expect("a GitHub installation");
        Installation { workspace, id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::github_installations;
    use crate::db::organizations;
    use crate::db::users;
    use crate::db::workspaces;

    async fn create_test_pool() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/zone_test".to_string()
        });

        sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to test database")
    }

    async fn setup_test_data(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
        // Create organization
        let org_id = organizations::create_organization(
            pool,
            &format!("Test Org {}", Uuid::new_v4()),
            &format!("test-org-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create organization")
        .id;

        // Create workspace
        let workspace_id = workspaces::create_workspace(
            pool,
            org_id,
            &format!("Test Workspace {}", Uuid::new_v4()),
            &format!("test-ws-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create user
        let user_email = format!("test-{}@example.com", Uuid::new_v4());
        let user_id =
            users::create_user(pool, &user_email, "password_hash", Some("Test User"), false)
                .await
                .expect("Failed to create user")
                .id;

        (org_id, workspace_id, user_id)
    }

    async fn widgets(
        pool: &PgPool,
        workspace_id: Uuid,
        credentials_encrypted: Option<&str>,
        github_installation_id: Option<Uuid>,
    ) -> SourceRow {
        create_source(
            pool,
            NewSource {
                workspace_id,
                name: &format!("widgets-{}", Uuid::new_v4()),
                source_type: GITHUB,
                config: serde_json::json!({"owner": "acme", "repo": "widgets"}),
                description: None,
                url: None,
                credentials_encrypted,
                github_installation_id,
            },
        )
        .await
        .expect("a source")
    }

    #[tokio::test]
    async fn attaching_an_installation_drops_the_stored_token() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = widgets(&pool, installation.workspace, Some("ENC[v1:token]"), None).await;

        let attached = set_github_installation(
            &pool,
            source.id,
            installation.workspace,
            Some(installation.id),
        )
        .await
        .expect("the update")
        .expect("the source");

        assert_eq!(attached.github_installation_id, Some(installation.id));
        assert_eq!(attached.credentials_encrypted, None);
        assert_eq!(attached.id, source.id);
    }

    #[tokio::test]
    async fn detaching_an_installation_keeps_the_stored_token() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = widgets(
            &pool,
            installation.workspace,
            Some("ENC[v1:token]"),
            Some(installation.id),
        )
        .await;
        assert_eq!(source.github_installation_id, Some(installation.id));

        let detached = set_github_installation(&pool, source.id, installation.workspace, None)
            .await
            .expect("the update")
            .expect("the source");

        assert_eq!(detached.github_installation_id, None);
        assert_eq!(
            detached.credentials_encrypted.as_deref(),
            Some("ENC[v1:token]")
        );
    }

    #[tokio::test]
    async fn another_workspace_cannot_attach_an_installation() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let other = testing::installation(&pool).await;
        let source = widgets(&pool, installation.workspace, None, None).await;

        let refused = set_github_installation(&pool, source.id, other.workspace, Some(other.id))
            .await
            .expect("the update");

        assert!(
            refused.is_none(),
            "another workspace's write reached the source"
        );
        let unchanged = get_source(&pool, source.id, installation.workspace)
            .await
            .expect("the read")
            .expect("the source");
        assert_eq!(unchanged.github_installation_id, None);
    }

    async fn revoke(pool: &PgPool, installation: Uuid) {
        sqlx::query("UPDATE github_installations SET revoked_at = NOW() WHERE id = $1")
            .bind(installation)
            .execute(pool)
            .await
            .expect("the installation is revoked");
    }

    async fn stored(pool: &PgPool, source: &SourceRow) -> SourceRow {
        get_source(pool, source.id, source.workspace_id.expect("a workspace"))
            .await
            .expect("the read")
            .expect("the source")
    }

    #[tokio::test]
    async fn an_installation_revoked_before_the_write_rolls_back_the_rename() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = widgets(&pool, installation.workspace, Some("ENC[v1:token]"), None).await;
        revoke(&pool, installation.id).await;
        let renamed = format!("renamed-{}", Uuid::new_v4());

        let refused = update_source(
            &pool,
            source.id,
            installation.workspace,
            SourceChanges {
                name: Some(&renamed),
                github_installation_id: Some(Some(installation.id)),
                ..SourceChanges::default()
            },
        )
        .await;

        assert!(
            matches!(refused, Err(UpdateError::Unattachable)),
            "a revoked installation must be refused, got {refused:?}"
        );
        let unchanged = stored(&pool, &source).await;
        assert_eq!(unchanged.name, source.name, "the rename was rolled back");
        assert_eq!(unchanged.github_installation_id, None);
        assert_eq!(
            unchanged.credentials_encrypted.as_deref(),
            Some("ENC[v1:token]")
        );
    }

    #[tokio::test]
    async fn an_installation_zone_may_not_use_attaches_nothing() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let foreign = testing::installation(&pool).await;
        let suspended = testing::installation(&pool).await;
        sqlx::query("UPDATE github_installations SET suspended_at = NOW() WHERE id = $1")
            .bind(suspended.id)
            .execute(&pool)
            .await
            .expect("the installation is suspended");
        let revoked = testing::installation(&pool).await;
        revoke(&pool, revoked.id).await;

        for (workspace, refused, reason) in [
            (installation.workspace, foreign.id, "another organization's"),
            (installation.workspace, Uuid::new_v4(), "a deleted"),
            (suspended.workspace, suspended.id, "a suspended"),
            (revoked.workspace, revoked.id, "a revoked"),
        ] {
            let source = widgets(&pool, workspace, None, None).await;
            let attached = set_github_installation(&pool, source.id, workspace, Some(refused))
                .await
                .expect("the update");
            assert!(attached.is_none(), "{reason} installation was attached");
            assert_eq!(stored(&pool, &source).await.github_installation_id, None);
        }
    }

    #[tokio::test]
    async fn an_installation_lost_after_the_check_creates_no_source() {
        let pool = testing::pool().await;
        let revoked = testing::installation(&pool).await;
        let deleted = testing::installation(&pool).await;
        for installation in [&revoked, &deleted] {
            assert!(
                github_installations::attachable(&pool, installation.workspace, installation.id)
                    .await
                    .expect("the check")
                    .is_some()
            );
        }
        revoke(&pool, revoked.id).await;
        sqlx::query("DELETE FROM github_installations WHERE id = $1")
            .bind(deleted.id)
            .execute(&pool)
            .await
            .expect("the installation is deleted");

        for (installation, reason) in [(&revoked, "a revoked"), (&deleted, "a deleted")] {
            let name = format!("widgets-{}", Uuid::new_v4());
            let created = create_source(
                &pool,
                NewSource {
                    workspace_id: installation.workspace,
                    name: &name,
                    source_type: GITHUB,
                    config: serde_json::json!({"owner": "acme", "repo": "widgets"}),
                    description: None,
                    url: None,
                    credentials_encrypted: None,
                    github_installation_id: Some(installation.id),
                },
            )
            .await;

            assert!(
                matches!(created, Err(UpdateError::Unattachable)),
                "{reason} installation must be refused, got {created:?}"
            );
            assert!(
                list_sources(&pool, installation.workspace, None, None, 100, 0)
                    .await
                    .expect("the list")
                    .is_empty(),
                "{reason} installation created a source"
            );
        }
    }

    async fn instance(pool: &PgPool, workspace_id: Uuid, host: &str) -> SourceRow {
        create_source(
            pool,
            NewSource {
                workspace_id,
                name: &format!("project-{}", Uuid::new_v4()),
                source_type: GITLAB,
                config: serde_json::json!({ "project_id": "group/project", "host": host }),
                description: None,
                url: None,
                credentials_encrypted: Some("ENC[v1:token]"),
                github_installation_id: None,
            },
        )
        .await
        .expect("a source")
    }

    #[tokio::test]
    async fn a_gitlab_source_holding_a_token_moves_instance_only_with_a_new_one() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = instance(&pool, installation.workspace, "https://first.example").await;
        let moved =
            serde_json::json!({ "project_id": "group/project", "host": "https://second.example" });

        let refused = update_source(
            &pool,
            source.id,
            installation.workspace,
            SourceChanges {
                config: Some(moved.clone()),
                ..SourceChanges::default()
            },
        )
        .await;

        assert!(
            matches!(refused, Err(UpdateError::InstanceNeedsToken)),
            "the stored token must not follow the source, got {refused:?}"
        );
        let unchanged = stored(&pool, &source).await;
        assert_eq!(unchanged.config, source.config);
        assert_eq!(unchanged.updated_at, source.updated_at);

        let moved_with_token = update_source(
            &pool,
            source.id,
            installation.workspace,
            SourceChanges {
                config: Some(moved.clone()),
                credentials_encrypted: Some("ENC[v1:other]"),
                ..SourceChanges::default()
            },
        )
        .await
        .expect("a new token moves the source");
        assert_eq!(moved_with_token.config, moved);
        assert_eq!(
            moved_with_token.credentials_encrypted.as_deref(),
            Some("ENC[v1:other]")
        );
    }

    #[tokio::test]
    async fn a_rename_and_an_attach_land_together() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = widgets(&pool, installation.workspace, Some("ENC[v1:token]"), None).await;
        let renamed = format!("renamed-{}", Uuid::new_v4());

        let updated = update_source(
            &pool,
            source.id,
            installation.workspace,
            SourceChanges {
                name: Some(&renamed),
                github_installation_id: Some(Some(installation.id)),
                ..SourceChanges::default()
            },
        )
        .await
        .expect("the update");

        assert_eq!(updated.name, renamed);
        assert_eq!(updated.github_installation_id, Some(installation.id));
        assert_eq!(updated.credentials_encrypted, None);
        assert_eq!(stored(&pool, &source).await.name, renamed);
    }

    #[test]
    fn debug_redacts_the_sealed_credentials() {
        let row = SourceRow {
            id: Uuid::new_v4(),
            name: "widgets".to_string(),
            source_type: GITHUB.to_string(),
            config: serde_json::json!({"owner": "acme", "repo": "widgets"}),
            credentials_encrypted: Some("ENC[v1:sealed-token]".to_string()),
            description: None,
            url: None,
            is_active: Some(true),
            last_verified_at: None,
            last_error: None,
            created_at: None,
            updated_at: None,
            workspace_id: None,
            github_installation_id: None,
        };

        let rendered = format!("{row:?}");

        assert!(!rendered.contains("sealed-token"), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
        assert!(rendered.contains("widgets"), "{rendered}");
    }

    #[tokio::test]
    async fn a_new_token_detaches_the_installation_and_returns_the_row() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = widgets(&pool, installation.workspace, None, Some(installation.id)).await;

        let updated = update_credentials(&pool, source.id, installation.workspace, "ENC[v1:new]")
            .await
            .expect("the update")
            .expect("the source");

        assert_eq!(updated.github_installation_id, None);
        assert_eq!(
            updated.credentials_encrypted.as_deref(),
            Some("ENC[v1:new]")
        );
        assert!(
            update_credentials(&pool, source.id, Uuid::new_v4(), "ENC[v1:other]")
                .await
                .expect("the update")
                .is_none(),
            "a foreign workspace updated the token"
        );
    }

    #[tokio::test]
    async fn the_repository_lookup_finds_an_installation_only_source() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let bare = widgets(&pool, installation.workspace, None, None).await;
        assert!(
            get_repository_source(&pool, installation.workspace, GITHUB, "acme", "widgets")
                .await
                .expect("the lookup")
                .is_none(),
            "a source with neither a token nor an installation was offered"
        );

        let attached = set_github_installation(
            &pool,
            bare.id,
            installation.workspace,
            Some(installation.id),
        )
        .await
        .expect("the update")
        .expect("the source");
        let found = get_repository_source(&pool, installation.workspace, GITHUB, "ACME", "Widgets")
            .await
            .expect("the lookup")
            .expect("the installation-only source");

        assert_eq!(found.id, attached.id);
        assert_eq!(found.github_installation_id, Some(installation.id));
    }

    #[tokio::test]
    async fn active_index_sources_carry_the_installation() {
        let pool = testing::pool().await;
        let installation = testing::installation(&pool).await;
        let source = widgets(&pool, installation.workspace, None, Some(installation.id)).await;

        let listed = list_active_index_sources(&pool)
            .await
            .expect("the listing")
            .into_iter()
            .find(|candidate| candidate.id == source.id)
            .expect("the active source");

        assert_eq!(listed.github_installation_id, Some(installation.id));
        assert_eq!(
            listed.as_source_row().github_installation_id,
            Some(installation.id)
        );
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_create_source_sets_workspace_id() {
        let pool = create_test_pool().await;
        let (_org_id, workspace_id, _user_id) = setup_test_data(&pool).await;
        let unique_name = format!("Test Source {}", Uuid::new_v4());

        let source = create_source(
            &pool,
            NewSource {
                workspace_id,
                name: &unique_name,
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo"}),
                description: Some("Test description"),
                url: Some("https://github.com/test/repo"),
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source");

        assert_eq!(
            source.workspace_id,
            Some(workspace_id),
            "Source should have workspace_id set"
        );
        assert_eq!(source.name, unique_name);
        assert_eq!(source.source_type, "github");
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_list_sources_filters_by_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_id = Uuid::new_v4();
        let source1 = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id1,
                name: &format!("Source 1 {}", unique_id),
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo1"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source 1");

        // Create source in workspace 2
        let _source2 = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id2,
                name: &format!("Source 2 {}", unique_id),
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo2"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source 2");

        // List sources for workspace 1
        let sources1 = list_sources(&pool, workspace_id1, None, None, 100, 0)
            .await
            .expect("Failed to list sources for workspace 1");

        // Verify workspace 1 only sees its own source
        assert_eq!(sources1.len(), 1, "Workspace 1 should only see 1 source");
        assert_eq!(sources1[0].id, source1.id);
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_get_source_requires_matching_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_name = format!("Test Source {}", Uuid::new_v4());
        let source = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id1,
                name: &unique_name,
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source");

        // Try to get source with correct workspace - should succeed
        let result1 = get_source(&pool, source.id, workspace_id1)
            .await
            .expect("Failed to get source");
        assert!(
            result1.is_some(),
            "Should find source with correct workspace_id"
        );

        // Try to get source with wrong workspace - should fail
        let result2 = get_source(&pool, source.id, workspace_id2)
            .await
            .expect("Failed to query source");
        assert!(
            result2.is_none(),
            "Should not find source with incorrect workspace_id"
        );
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_update_source_requires_matching_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_name = format!("Test Source {}", Uuid::new_v4());
        let source = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id1,
                name: &unique_name,
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source");

        // Try to update with wrong workspace - should fail
        let updated_name = format!("Updated Name {}", Uuid::new_v4());
        let result1 = update_source(
            &pool,
            source.id,
            workspace_id2,
            SourceChanges {
                name: Some(&updated_name),
                ..SourceChanges::default()
            },
        )
        .await;
        assert!(
            matches!(result1, Err(UpdateError::NotFound)),
            "Should not update source with incorrect workspace_id"
        );

        // Try to update with correct workspace - should succeed
        let result2 = update_source(
            &pool,
            source.id,
            workspace_id1,
            SourceChanges {
                name: Some(&updated_name),
                ..SourceChanges::default()
            },
        )
        .await
        .expect("Should update source with correct workspace_id");
        assert_eq!(result2.name, updated_name);
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_delete_source_requires_matching_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create source in workspace 1
        let unique_name = format!("Test Source {}", Uuid::new_v4());
        let source = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id1,
                name: &unique_name,
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source");

        // Try to delete with wrong workspace - should fail
        let deleted = delete_source(&pool, source.id, workspace_id2)
            .await
            .expect("Failed to execute delete");
        assert!(
            !deleted,
            "Should not delete source with incorrect workspace_id"
        );

        // Verify source still exists
        let still_exists = get_source(&pool, source.id, workspace_id1)
            .await
            .expect("Failed to get source");
        assert!(
            still_exists.is_some(),
            "Source should still exist after failed delete"
        );

        // Try to delete with correct workspace - should succeed
        let deleted = delete_source(&pool, source.id, workspace_id1)
            .await
            .expect("Failed to execute delete");
        assert!(deleted, "Should delete source with correct workspace_id");
    }

    #[tokio::test]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "PostgreSQL not available on this platform"
    )]
    async fn test_get_sources_by_ids_filters_by_workspace() {
        let pool = create_test_pool().await;
        let (org_id, workspace_id1, _user_id) = setup_test_data(&pool).await;

        // Create second workspace
        let workspace_id2 = workspaces::create_workspace(
            &pool,
            org_id,
            &format!("Test Workspace 2 {}", Uuid::new_v4()),
            &format!("test-ws-2-{}", Uuid::new_v4()),
            None,
        )
        .await
        .expect("Failed to create workspace")
        .id;

        // Create sources in both workspaces
        let unique_id = Uuid::new_v4();
        let source1 = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id1,
                name: &format!("Source 1 {}", unique_id),
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo1"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source 1");

        let source2 = create_source(
            &pool,
            NewSource {
                workspace_id: workspace_id2,
                name: &format!("Source 2 {}", unique_id),
                source_type: "github",
                config: serde_json::json!({"repo": "test/repo2"}),
                description: None,
                url: None,
                credentials_encrypted: None,
                github_installation_id: None,
            },
        )
        .await
        .expect("Failed to create source 2");

        // Try to get both sources using workspace 1 - should only get source 1
        let sources = get_sources_by_ids(&pool, &[source1.id, source2.id], workspace_id1)
            .await
            .expect("Failed to get sources");

        assert_eq!(
            sources.len(),
            1,
            "Should only get sources from correct workspace"
        );
        assert_eq!(sources[0].id, source1.id);
    }
}
