//! Organization MCP servers and the workspace overrides that switch them off for one workspace.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;
use chrono::DateTime;
use chrono::Utc;
use reqwest::Url;
use serde::Deserialize;
use sqlx::Executor;
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::QueryBuilder;
use uuid::Uuid;

use crate::secret::Unseal;
use crate::services::mcp_servers::validation;

use super::organization_members;
use super::organization_members::OrgRole;
use super::workspace_members;
use super::workspace_members::WorkspaceRole;

const COLUMNS: &str = "id, organization_id, name, transport, command, arguments, url, tools, enabled, secrets, created_by, created_at, updated_at";

const NAME_KEY: &str = "mcp_servers_organization_name_key";
const NAME_CHECK: &str = "mcp_servers_name_check";
const TRANSPORT_CHECK: &str = "mcp_servers_transport_check";
const TARGET_CHECK: &str = "mcp_servers_target_check";
const CHECK_VIOLATION: &str = "23514";
const REVISED: &str = "updated_at = GREATEST(now(), updated_at + interval '1 microsecond')";

const ORGANIZATION_NOT_FOUND: &str = "Organization not found";
const WORKSPACE_NOT_FOUND: &str = "Workspace not found";
const SERVER_NOT_FOUND: &str = "MCP server not found";
const ORGANIZATION_FORBIDDEN: &str = "Only organization admins can manage MCP servers";
const WORKSPACE_FORBIDDEN: &str = "Only workspace admins can change MCP servers";
const DUPLICATE_NAME: &str = "A server with this name already exists";
const INVALID_NAME: &str =
    "A server name is 1 to 32 letters, digits, '-' or '_', with no '__' and no trailing '_'";
const INVALID_TRANSPORT: &str = "The transport must be stdio or http";
const INVALID_TARGET: &str =
    "A stdio server needs a command and no URL; an http server needs a URL and no command";
const FOREIGN_SERVER: &str = "An MCP server in the overrides does not belong to this organization";
const UNSEALABLE_SECRETS: &str = "The MCP server secrets could not be sealed";
pub const ORIGIN_CHANGED: &str = "Re-enter the header values when the URL's origin changes";
const ENVIRONMENT_FIELD: &str = "Environment variable";
const HEADER_FIELD: &str = "Header";
const ENVIRONMENT_KEY: &str = "environment";
const HEADERS_KEY: &str = "headers";

/// The credentials a server holds, opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    pub environment: BTreeMap<String, SecretValue>,
    pub headers: BTreeMap<String, SecretValue>,
}

impl Secrets {
    fn is_empty(&self) -> bool {
        self.environment.is_empty() && self.headers.is_empty()
    }
}

#[derive(Deserialize)]
struct Opening {
    #[serde(default)]
    environment: BTreeMap<String, SecretValue>,
    #[serde(default)]
    headers: BTreeMap<String, SecretValue>,
}

/// An organization's MCP server, its secrets opened.
#[derive(Debug, Clone)]
pub struct McpServerRecord {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub url: Option<String>,
    pub tools: Option<Vec<String>>,
    pub enabled: bool,
    pub secrets: Secrets,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A server to add to an organization.
#[derive(Debug, Clone)]
pub struct NewMcpServer {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub url: Option<String>,
    pub tools: Option<Vec<String>>,
    pub enabled: bool,
    pub environment: BTreeMap<String, SecretValue>,
    pub headers: BTreeMap<String, SecretValue>,
}

/// The fields of a server to change; `None` leaves a field as it is.
///
/// In `environment` and `headers`, a name mapped to `None` keeps its stored value, and a stored
/// name the map leaves out is removed.
#[derive(Debug, Clone, Default)]
pub struct McpServerChanges {
    pub name: Option<String>,
    pub command: Option<String>,
    pub arguments: Option<Vec<String>>,
    pub url: Option<String>,
    pub tools: Option<Option<Vec<String>>>,
    pub enabled: Option<bool>,
    pub environment: Option<BTreeMap<String, Option<SecretValue>>>,
    pub headers: Option<BTreeMap<String, Option<SecretValue>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum McpServerError {
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(&'static str),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Sealing(String),
}

type McpServerResult<T> = Result<T, McpServerError>;

#[derive(Clone, sqlx::FromRow)]
struct Row {
    id: Uuid,
    organization_id: Uuid,
    name: String,
    transport: String,
    command: Option<String>,
    arguments: Vec<String>,
    url: Option<String>,
    tools: Option<Vec<String>>,
    enabled: bool,
    secrets: Option<String>,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Row {
    fn open(self, key: &MasterKey) -> McpServerResult<McpServerRecord> {
        let secrets = open(self.id, self.secrets.as_deref(), key)?;
        Ok(self.record(secrets))
    }

    fn record(self, secrets: Secrets) -> McpServerRecord {
        McpServerRecord {
            id: self.id,
            organization_id: self.organization_id,
            name: self.name,
            transport: self.transport,
            command: self.command,
            arguments: self.arguments,
            url: self.url,
            tools: self.tools,
            enabled: self.enabled,
            secrets,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

fn seal(secrets: &Secrets, key: &MasterKey) -> McpServerResult<Option<String>> {
    if secrets.is_empty() {
        return Ok(None);
    }
    let document: BTreeMap<&str, BTreeMap<&str, &str>> = [
        (ENVIRONMENT_KEY, &secrets.environment),
        (HEADERS_KEY, &secrets.headers),
    ]
    .into_iter()
    .map(|(field, values)| {
        let exposed = values
            .iter()
            .map(|(name, value)| (name.as_str(), value.expose()))
            .collect();
        (field, exposed)
    })
    .collect();
    let json = serde_json::to_string(&document)
        .map_err(|_| McpServerError::Sealing(UNSEALABLE_SECRETS.to_owned()))?;
    encrypt_value(&SecretValue::new(json), key)
        .map(Some)
        .map_err(|error| McpServerError::Sealing(format!("{UNSEALABLE_SECRETS}: {error}")))
}

fn open(server: Uuid, stored: Option<&str>, key: &MasterKey) -> McpServerResult<Secrets> {
    let Some(stored) = stored else {
        return Ok(Secrets::default());
    };
    let opened = key.open_strict(stored).map_err(|error| {
        McpServerError::Sealing(format!(
            "The secrets of MCP server {server} did not open: {error}"
        ))
    })?;
    let opening: Opening = serde_json::from_str(opened.expose()).map_err(|_| {
        McpServerError::Sealing(format!(
            "The stored secrets of MCP server {server} are malformed"
        ))
    })?;
    Ok(Secrets {
        environment: opening.environment,
        headers: opening.headers,
    })
}

fn classify(error: sqlx::Error) -> McpServerError {
    let Some(database) = error.as_database_error() else {
        return McpServerError::Database(error);
    };
    if database.is_unique_violation() && database.constraint() == Some(NAME_KEY) {
        return McpServerError::Conflict(DUPLICATE_NAME);
    }
    if database.code().as_deref() == Some(CHECK_VIOLATION) {
        let message = match database.constraint() {
            Some(NAME_CHECK) => INVALID_NAME,
            Some(TRANSPORT_CHECK) => INVALID_TRANSPORT,
            Some(TARGET_CHECK) => INVALID_TARGET,
            _ => return McpServerError::Database(error),
        };
        return McpServerError::Invalid(message.to_owned());
    }
    McpServerError::Database(error)
}

fn distinct(headers: &BTreeMap<String, SecretValue>) -> McpServerResult<()> {
    validation::headers(headers.keys().map(String::as_str))
        .map_err(|error| McpServerError::Invalid(error.to_string()))
}

fn merge(
    field: &str,
    stored: &BTreeMap<String, SecretValue>,
    changes: BTreeMap<String, Option<SecretValue>>,
) -> McpServerResult<BTreeMap<String, SecretValue>> {
    changes
        .into_iter()
        .map(
            |(name, value)| match value.or_else(|| stored.get(&name).cloned()) {
                Some(value) => Ok((name, value)),
                None => Err(McpServerError::Invalid(format!(
                    "{field} {name} has no saved value"
                ))),
            },
        )
        .collect()
}

fn moved(before: Option<&str>, after: &str) -> bool {
    match (before.map(Url::parse), Url::parse(after)) {
        (Some(Ok(before)), Ok(after)) => before.origin() != after.origin(),
        _ => true,
    }
}

fn kept(
    stored: &BTreeMap<String, SecretValue>,
    changes: Option<&BTreeMap<String, Option<SecretValue>>>,
) -> bool {
    match changes {
        None => !stored.is_empty(),
        Some(changes) => changes
            .iter()
            .any(|(name, value)| value.is_none() && stored.contains_key(name)),
    }
}

async fn authorize_organization(
    connection: &mut PgConnection,
    organization: Uuid,
    user: Uuid,
    minimum: OrgRole,
) -> McpServerResult<()> {
    match organization_members::lock_role(connection, organization, user).await? {
        Some(role) if role >= minimum => Ok(()),
        Some(_) => Err(McpServerError::Forbidden(ORGANIZATION_FORBIDDEN)),
        None => Err(McpServerError::NotFound(ORGANIZATION_NOT_FOUND)),
    }
}

async fn authorize_workspace(
    connection: &mut PgConnection,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
    write: bool,
) -> McpServerResult<()> {
    authorize_organization(&mut *connection, organization, user, OrgRole::Member).await?;
    let owned: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workspaces WHERE id = $1 AND organization_id = $2 FOR SHARE",
    )
    .bind(workspace)
    .bind(organization)
    .fetch_optional(&mut *connection)
    .await?;
    if owned.is_none() {
        return Err(McpServerError::NotFound(WORKSPACE_NOT_FOUND));
    }
    match workspace_members::lock_role(connection, workspace, user).await? {
        Some(WorkspaceRole::Owner | WorkspaceRole::Admin) => Ok(()),
        Some(_) if !write => Ok(()),
        Some(_) => Err(McpServerError::Forbidden(WORKSPACE_FORBIDDEN)),
        None => Err(McpServerError::NotFound(WORKSPACE_NOT_FOUND)),
    }
}

async fn rows<'e, E>(executor: E, organization: Uuid) -> McpServerResult<Vec<Row>>
where
    E: Executor<'e, Database = Postgres>,
{
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM mcp_servers WHERE organization_id = $1 ORDER BY name"
    )))
    .bind(organization)
    .fetch_all(executor)
    .await?)
}

async fn overrides<'e, E>(executor: E, workspace: Uuid) -> McpServerResult<BTreeMap<Uuid, bool>>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT server_id, enabled FROM workspace_mcp_servers WHERE workspace_id = $1",
    )
    .bind(workspace)
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().collect())
}

fn open_all(rows: Vec<Row>, key: &MasterKey) -> McpServerResult<Vec<McpServerRecord>> {
    rows.into_iter().map(|row| row.open(key)).collect()
}

/// Every server of `organization`, for one of its members.
pub async fn list_authorized(
    pool: &PgPool,
    key: &MasterKey,
    organization: Uuid,
    user: Uuid,
) -> McpServerResult<Vec<McpServerRecord>> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization, user, OrgRole::Member).await?;
    let rows = rows(&mut *transaction, organization).await?;
    transaction.commit().await?;
    open_all(rows, key)
}

/// One server of `organization`, for one of its members.
pub async fn get_authorized(
    pool: &PgPool,
    key: &MasterKey,
    organization: Uuid,
    server: Uuid,
    user: Uuid,
) -> McpServerResult<McpServerRecord> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization, user, OrgRole::Member).await?;
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM mcp_servers WHERE organization_id = $1 AND id = $2"
    )))
    .bind(organization)
    .bind(server)
    .fetch_optional(&mut *transaction)
    .await?;
    transaction.commit().await?;
    row.ok_or(McpServerError::NotFound(SERVER_NOT_FOUND))?
        .open(key)
}

/// Add a server to `organization`, for one of its administrators.
pub async fn create_authorized(
    pool: &PgPool,
    key: &MasterKey,
    organization: Uuid,
    user: Uuid,
    new: NewMcpServer,
) -> McpServerResult<McpServerRecord> {
    let secrets = Secrets {
        environment: new.environment,
        headers: new.headers,
    };
    distinct(&secrets.headers)?;
    let sealed = seal(&secrets, key)?;
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization, user, OrgRole::Admin).await?;
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO mcp_servers (organization_id, name, transport, command, arguments, url, tools, enabled, secrets, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {COLUMNS}"
    )))
    .bind(organization)
    .bind(&new.name)
    .bind(&new.transport)
    .bind(&new.command)
    .bind(&new.arguments)
    .bind(&new.url)
    .bind(&new.tools)
    .bind(new.enabled)
    .bind(sealed)
    .bind(user)
    .fetch_one(&mut *transaction)
    .await
    .map_err(classify)?;
    transaction.commit().await?;
    Ok(row.record(secrets))
}

/// Change the given fields of one of `organization`'s servers, for one of its administrators.
pub async fn update_authorized(
    pool: &PgPool,
    key: &MasterKey,
    organization: Uuid,
    server: Uuid,
    user: Uuid,
    changes: McpServerChanges,
) -> McpServerResult<McpServerRecord> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization, user, OrgRole::Admin).await?;
    let stored: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM mcp_servers WHERE organization_id = $1 AND id = $2 FOR UPDATE"
    )))
    .bind(organization)
    .bind(server)
    .fetch_optional(&mut *transaction)
    .await?;
    let stored = stored.ok_or(McpServerError::NotFound(SERVER_NOT_FOUND))?;
    let before = stored.clone().open(key)?;
    if changes
        .url
        .as_deref()
        .is_some_and(|url| moved(before.url.as_deref(), url))
        && kept(&before.secrets.headers, changes.headers.as_ref())
    {
        return Err(McpServerError::Invalid(ORIGIN_CHANGED.to_owned()));
    }

    let secrets = Secrets {
        environment: match changes.environment {
            Some(environment) => {
                merge(ENVIRONMENT_FIELD, &before.secrets.environment, environment)?
            }
            None => before.secrets.environment.clone(),
        },
        headers: match changes.headers {
            Some(headers) => merge(HEADER_FIELD, &before.secrets.headers, headers)?,
            None => before.secrets.headers.clone(),
        },
    };
    distinct(&secrets.headers)?;
    let resealing = secrets != before.secrets;
    let name = changes.name.filter(|name| *name != before.name);
    let command = changes
        .command
        .filter(|command| before.command.as_ref() != Some(command));
    let arguments = changes
        .arguments
        .filter(|arguments| *arguments != before.arguments);
    let url = changes.url.filter(|url| before.url.as_ref() != Some(url));
    let tools = changes.tools.filter(|tools| *tools != before.tools);
    let enabled = changes.enabled.filter(|enabled| *enabled != before.enabled);
    let unchanged = !resealing
        && name.is_none()
        && command.is_none()
        && arguments.is_none()
        && url.is_none()
        && tools.is_none()
        && enabled.is_none();

    let mut query = QueryBuilder::<Postgres>::new("UPDATE mcp_servers SET ");
    let mut assignments = query.separated(", ");
    if let Some(name) = name {
        assignments.push("name = ").push_bind_unseparated(name);
    }
    if let Some(command) = command {
        assignments
            .push("command = ")
            .push_bind_unseparated(command);
    }
    if let Some(arguments) = arguments {
        assignments
            .push("arguments = ")
            .push_bind_unseparated(arguments);
    }
    if let Some(url) = url {
        assignments.push("url = ").push_bind_unseparated(url);
    }
    if let Some(tools) = tools {
        assignments.push("tools = ").push_bind_unseparated(tools);
    }
    if let Some(enabled) = enabled {
        assignments
            .push("enabled = ")
            .push_bind_unseparated(enabled);
    }
    if resealing {
        assignments
            .push("secrets = ")
            .push_bind_unseparated(seal(&secrets, key)?);
    }
    assignments.push(REVISED);
    query
        .push(" WHERE organization_id = ")
        .push_bind(organization)
        .push(" AND id = ")
        .push_bind(server)
        .push(" RETURNING ")
        .push(COLUMNS);
    let row: Row = if unchanged {
        stored
    } else {
        query
            .build_query_as()
            .fetch_one(&mut *transaction)
            .await
            .map_err(classify)?
    };
    transaction.commit().await?;

    Ok(row.record(secrets))
}

/// Remove one of `organization`'s servers, for one of its administrators.
///
/// The record's `secrets` are empty: the removed row is never opened.
pub async fn delete_authorized(
    pool: &PgPool,
    organization: Uuid,
    server: Uuid,
    user: Uuid,
) -> McpServerResult<McpServerRecord> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization, user, OrgRole::Admin).await?;
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "DELETE FROM mcp_servers WHERE organization_id = $1 AND id = $2 RETURNING {COLUMNS}"
    )))
    .bind(organization)
    .bind(server)
    .fetch_optional(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(row
        .ok_or(McpServerError::NotFound(SERVER_NOT_FOUND))?
        .record(Secrets::default()))
}

/// Whether `user` administers `organization`.
pub async fn authorize_admin(pool: &PgPool, organization: Uuid, user: Uuid) -> McpServerResult<()> {
    let mut transaction = pool.begin().await?;
    authorize_organization(&mut transaction, organization, user, OrgRole::Admin).await?;
    transaction.commit().await?;
    Ok(())
}

/// Every server of `organization` whose secrets open, with no caller to authorize.
pub async fn for_organization(
    pool: &PgPool,
    key: &MasterKey,
    organization: Uuid,
) -> McpServerResult<Vec<McpServerRecord>> {
    let mut records = Vec::new();
    for row in rows(pool, organization).await? {
        let server = row.id;
        match row.open(key) {
            Ok(record) => records.push(record),
            Err(error) => {
                tracing::warn!(
                    %organization,
                    %server,
                    %error,
                    "Leaving out an MCP server whose secrets do not open"
                );
            }
        }
    }
    Ok(records)
}

/// The organization `workspace` belongs to.
pub async fn organization_for_workspace(
    pool: &PgPool,
    workspace: Uuid,
) -> McpServerResult<Option<Uuid>> {
    Ok(
        sqlx::query_scalar("SELECT organization_id FROM workspaces WHERE id = $1")
            .bind(workspace)
            .fetch_optional(pool)
            .await?,
    )
}

/// The override `workspace` stores for each server it chose about: `false` switches the server
/// off for the workspace, and `true` changes nothing, since the organization's own switch still
/// decides.
pub async fn workspace_overrides(
    pool: &PgPool,
    workspace: Uuid,
) -> McpServerResult<BTreeMap<Uuid, bool>> {
    overrides(pool, workspace).await
}

/// The organization's servers and `workspace`'s overrides of them, for a member of the workspace.
pub async fn workspace_list_authorized(
    pool: &PgPool,
    key: &MasterKey,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
) -> McpServerResult<(Vec<McpServerRecord>, BTreeMap<Uuid, bool>)> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(&mut transaction, organization, workspace, user, false).await?;
    let rows = rows(&mut *transaction, organization).await?;
    let overrides = overrides(&mut *transaction, workspace).await?;
    transaction.commit().await?;
    Ok((open_all(rows, key)?, overrides))
}

/// Set (`Some`) or clear (`None`) `workspace`'s overrides, for an administrator of the workspace,
/// and hand back every override it holds afterwards.
pub async fn set_workspace_overrides_authorized(
    pool: &PgPool,
    organization: Uuid,
    workspace: Uuid,
    user: Uuid,
    changes: BTreeMap<Uuid, Option<bool>>,
) -> McpServerResult<BTreeMap<Uuid, bool>> {
    let mut transaction = pool.begin().await?;
    authorize_workspace(&mut transaction, organization, workspace, user, true).await?;

    let servers: Vec<Uuid> = changes.keys().copied().collect();
    let owned: BTreeSet<Uuid> = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM mcp_servers WHERE organization_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE",
    )
    .bind(organization)
    .bind(&servers)
    .fetch_all(&mut *transaction)
    .await?
    .into_iter()
    .collect();
    if owned.len() != servers.len() {
        return Err(McpServerError::Invalid(FOREIGN_SERVER.to_owned()));
    }

    let mut set_servers = Vec::new();
    let mut set_enabled = Vec::new();
    let mut cleared = Vec::new();
    for (server, enabled) in changes {
        match enabled {
            Some(enabled) => {
                set_servers.push(server);
                set_enabled.push(enabled);
            }
            None => cleared.push(server),
        }
    }

    if !set_servers.is_empty() {
        sqlx::query(
            "INSERT INTO workspace_mcp_servers (workspace_id, server_id, organization_id, enabled) \
             SELECT $1, server, $2, enabled FROM unnest($3::uuid[], $4::boolean[]) AS change(server, enabled) \
             ON CONFLICT (workspace_id, server_id) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now()",
        )
        .bind(workspace)
        .bind(organization)
        .bind(&set_servers)
        .bind(&set_enabled)
        .execute(&mut *transaction)
        .await?;
    }
    if !cleared.is_empty() {
        sqlx::query(
            "DELETE FROM workspace_mcp_servers WHERE workspace_id = $1 AND server_id = ANY($2)",
        )
        .bind(workspace)
        .bind(&cleared)
        .execute(&mut *transaction)
        .await?;
    }

    let overrides = overrides(&mut *transaction, workspace).await?;
    transaction.commit().await?;
    Ok(overrides)
}

/// When each server of `organization` last changed, without opening any secrets.
pub async fn revisions(
    pool: &PgPool,
    organization: Uuid,
) -> McpServerResult<BTreeMap<Uuid, DateTime<Utc>>> {
    let rows: Vec<(Uuid, DateTime<Utc>)> =
        sqlx::query_as("SELECT id, updated_at FROM mcp_servers WHERE organization_id = $1")
            .bind(organization)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}

/// For each of `organizations` that still exists, when each of its servers last changed, without
/// opening any secrets. An organization that was deleted is left out.
pub async fn existing_revisions(
    pool: &PgPool,
    organizations: &[Uuid],
) -> McpServerResult<BTreeMap<Uuid, BTreeMap<Uuid, DateTime<Utc>>>> {
    let rows: Vec<(Uuid, Option<Uuid>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT organizations.id, mcp_servers.id, mcp_servers.updated_at FROM organizations \
         LEFT JOIN mcp_servers ON mcp_servers.organization_id = organizations.id \
         WHERE organizations.id = ANY($1)",
    )
    .bind(organizations)
    .fetch_all(pool)
    .await?;
    let mut existing: BTreeMap<Uuid, BTreeMap<Uuid, DateTime<Utc>>> = BTreeMap::new();
    for (organization, server, revision) in rows {
        let revisions = existing.entry(organization).or_default();
        if let (Some(server), Some(revision)) = (server, revision) {
            revisions.insert(server, revision);
        }
    }
    Ok(existing)
}

/// The servers `organization` has switched on and `workspace` has not switched off, their
/// secrets left unopened.
pub async fn offered(
    pool: &PgPool,
    organization: Uuid,
    workspace: Uuid,
) -> McpServerResult<Vec<McpServerRecord>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM mcp_servers WHERE organization_id = $1 AND enabled \
         AND NOT EXISTS (SELECT 1 FROM workspace_mcp_servers AS choice \
         WHERE choice.workspace_id = $2 AND choice.server_id = mcp_servers.id AND NOT choice.enabled) \
         ORDER BY name"
    )))
    .bind(organization)
    .bind(workspace)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| row.record(Secrets::default()))
        .collect())
}

#[cfg(test)]
mod tests;
