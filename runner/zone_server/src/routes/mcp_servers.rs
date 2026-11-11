//! Organization MCP server endpoints

use std::collections::BTreeMap;

use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::McpTransport;
use abnegate_secret::SecretValue;
use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use reqwest::Url;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::IgnoredAny;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::audit::actions;
use crate::db::audit::resources;
use crate::db::mcp_servers;
use crate::db::mcp_servers::McpServerChanges;
use crate::db::mcp_servers::McpServerError;
use crate::db::mcp_servers::McpServerRecord;
use crate::db::mcp_servers::NewMcpServer;
use crate::services::mcp_servers::definition::Definition;
use crate::services::mcp_servers::load;
use crate::services::mcp_servers::organization::OrganizationHub;
use crate::services::mcp_servers::source::Source;
use crate::services::mcp_servers::status;
use crate::services::mcp_servers::status::Connection;
use crate::services::mcp_servers::status::Policy;
use crate::services::mcp_servers::status::Status;
use crate::services::mcp_servers::transport::Transport;
use crate::services::mcp_servers::validation;
use crate::services::mcp_servers::validation::ValidationError;
use crate::state::AppState;

use super::common::AuditEvent;
use super::common::ErrorResponse;
use super::common::audit;

const INTERNAL: &str = "Internal server error";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpSecretField {
    pub name: String,
    pub has_value: bool,
}

fn fields(secrets: &BTreeMap<String, SecretValue>) -> Vec<McpSecretField> {
    secrets
        .keys()
        .map(|name| McpSecretField {
            name: name.clone(),
            has_value: true,
        })
        .collect()
}

/// A server as the settings pages read it: which secrets it holds, never what they are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct McpServerResponse {
    pub id: Option<Uuid>,
    pub name: String,
    pub source: Source,
    pub transport: Transport,
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub url: Option<String>,
    pub tools: Option<Vec<String>>,
    pub enabled: bool,
    pub environment: Vec<McpSecretField>,
    pub headers: Vec<McpSecretField>,
    pub status: Status,
    pub error: Option<String>,
    pub available_tools: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl McpServerResponse {
    fn organization(
        record: &McpServerRecord,
        transport: Transport,
        connection: Connection,
    ) -> Self {
        Self {
            id: Some(record.id),
            name: record.name.clone(),
            source: Source::Organization,
            transport,
            command: record.command.clone(),
            arguments: record.arguments.clone(),
            url: record.url.clone(),
            tools: record.tools.clone(),
            enabled: record.enabled,
            environment: fields(&record.secrets.environment),
            headers: fields(&record.secrets.headers),
            status: connection.status,
            error: connection.error,
            available_tools: connection.tools,
            created_at: Some(record.created_at.to_rfc3339()),
            updated_at: Some(record.updated_at.to_rfc3339()),
        }
    }

    fn instance(name: &str, server: &McpServer, connection: Connection) -> Self {
        Self {
            id: None,
            name: name.to_string(),
            source: Source::Instance,
            transport: transport(server),
            command: server.command.clone(),
            arguments: Vec::new(),
            url: server.endpoint().and_then(|endpoint| origin(&endpoint.url)),
            tools: (!server.tools.is_empty()).then(|| server.tools.clone()),
            enabled: !server.disabled,
            environment: fields(&server.environment),
            headers: fields(&server.headers),
            status: connection.status,
            error: connection.error,
            available_tools: connection.tools,
            created_at: None,
            updated_at: None,
        }
    }
}

fn origin(url: &str) -> Option<String> {
    validation::literal(validation::URL, url).ok()?;
    let origin = Url::parse(url).ok()?.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

#[derive(Debug, Serialize)]
pub struct McpServerListResponse {
    pub commands: bool,
    pub servers: Vec<McpServerResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceMcpServerResponse {
    #[serde(flatten)]
    pub server: McpServerResponse,
    pub workspace_enabled: Option<bool>,
    pub effective: bool,
}

impl WorkspaceMcpServerResponse {
    fn of(server: McpServerResponse, overrides: &BTreeMap<Uuid, bool>) -> Self {
        let workspace_enabled = server.id.and_then(|id| overrides.get(&id).copied());
        Self {
            effective: effective(server.enabled, workspace_enabled),
            workspace_enabled,
            server,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceMcpServerListResponse {
    pub servers: Vec<WorkspaceMcpServerResponse>,
}

fn effective(enabled: bool, workspace_enabled: Option<bool>) -> bool {
    enabled && workspace_enabled.unwrap_or(true)
}

/// What a workspace's audit row holds when its overrides were stored and the servers they apply
/// to could not be read back.
#[derive(Debug, Serialize)]
struct StoredOverrides<'a> {
    overrides: &'a BTreeMap<Uuid, bool>,
}

#[derive(Debug, Serialize)]
struct RemovedMcpServer<'a> {
    name: &'a str,
    transport: &'a str,
    command: Option<&'a str>,
    arguments: &'a [String],
    url: Option<&'a str>,
    tools: Option<&'a [String]>,
    enabled: bool,
}

impl<'a> From<&'a McpServerRecord> for RemovedMcpServer<'a> {
    fn from(record: &'a McpServerRecord) -> Self {
        Self {
            name: &record.name,
            transport: &record.transport,
            command: record.command.as_deref(),
            arguments: &record.arguments,
            url: record.url.as_deref(),
            tools: record.tools.as_deref(),
            enabled: record.enabled,
        }
    }
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMcpServerRequest {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub url: Option<String>,
    pub tools: Option<Vec<String>>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub environment: BTreeMap<String, SecretValue>,
    #[serde(default)]
    pub headers: BTreeMap<String, SecretValue>,
}

impl CreateMcpServerRequest {
    fn validated(self, commands: bool) -> Result<NewMcpServer, Refusal> {
        let transport: Transport = self.transport.parse()?;
        validation::name(&self.name)?;
        if transport == Transport::Stdio && !commands {
            return Err(Refusal::Commands);
        }
        validation::target(transport, self.command.as_deref(), self.url.as_deref())?;
        let (command, url) = match transport {
            Transport::Stdio => {
                unexpected(transport, validation::HEADERS, self.headers.is_empty())?;
                (self.command.map(command).transpose()?, None)
            }
            Transport::Http | Transport::Sse => {
                unexpected(transport, validation::ARGUMENTS, self.arguments.is_empty())?;
                unexpected(
                    transport,
                    validation::ENVIRONMENT,
                    self.environment.is_empty(),
                )?;
                (None, self.url.as_deref().map(url).transpose()?)
            }
        };
        let arguments = arguments(self.arguments)?;
        if let Some(tools) = &self.tools {
            validation::tools(tools)?;
        }
        secrets(
            validation::ENVIRONMENT,
            validation::environment_name,
            self.environment
                .iter()
                .map(|(name, value)| (name, Some(value))),
        )?;
        secrets(
            validation::HEADERS,
            validation::header_name,
            self.headers.iter().map(|(name, value)| (name, Some(value))),
        )?;
        Ok(NewMcpServer {
            name: self.name,
            transport: transport.as_str().to_string(),
            command,
            arguments,
            url,
            tools: self.tools,
            enabled: self.enabled.unwrap_or(true),
            environment: self.environment,
            headers: self.headers,
        })
    }
}

/// The fields of a server to change. A server keeps the transport it was made with, so a body
/// that names one is refused whatever it says.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMcpServerRequest {
    #[serde(default, deserialize_with = "present")]
    pub transport: Option<Option<IgnoredAny>>,
    pub name: Option<String>,
    pub command: Option<String>,
    pub arguments: Option<Vec<String>>,
    pub url: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub tools: Option<Option<Vec<String>>>,
    pub enabled: Option<bool>,
    pub environment: Option<BTreeMap<String, Option<SecretValue>>>,
    pub headers: Option<BTreeMap<String, Option<SecretValue>>>,
}

impl UpdateMcpServerRequest {
    fn changes(
        self,
        stored: &McpServerRecord,
        commands: bool,
    ) -> Result<McpServerChanges, Refusal> {
        if self.transport.is_some() {
            return Err(Refusal::Transport);
        }
        let stdio = stored_transport(stored) == Transport::Stdio;
        if stdio && !commands {
            return Err(Refusal::Commands);
        }
        if let Some(name) = &self.name {
            validation::name(name)?;
        }
        let transport = if stdio {
            Transport::Stdio
        } else {
            Transport::Http
        };
        validation::target(
            transport,
            self.command.as_deref().or(stored.command.as_deref()),
            self.url.as_deref().or(stored.url.as_deref()),
        )?;
        let (command, url) = if stdio {
            unexpected(
                transport,
                validation::HEADERS,
                self.headers.as_ref().is_none_or(BTreeMap::is_empty),
            )?;
            (self.command.map(command).transpose()?, None)
        } else {
            unexpected(
                transport,
                validation::ARGUMENTS,
                self.arguments.as_ref().is_none_or(Vec::is_empty),
            )?;
            unexpected(
                transport,
                validation::ENVIRONMENT,
                self.environment.as_ref().is_none_or(BTreeMap::is_empty),
            )?;
            (None, self.url.as_deref().map(url).transpose()?)
        };
        let arguments = self.arguments.map(arguments).transpose()?;
        if let Some(Some(tools)) = &self.tools {
            validation::tools(tools)?;
        }
        if let Some(environment) = &self.environment {
            secrets(
                validation::ENVIRONMENT,
                validation::environment_name,
                environment
                    .iter()
                    .map(|(name, value)| (name, value.as_ref())),
            )?;
        }
        if let Some(headers) = &self.headers {
            secrets(
                validation::HEADERS,
                validation::header_name,
                headers.iter().map(|(name, value)| (name, value.as_ref())),
            )?;
        }
        Ok(McpServerChanges {
            name: self.name,
            command,
            arguments,
            url,
            tools: self.tools,
            enabled: self.enabled,
            environment: self.environment,
            headers: self.headers,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspaceMcpServersRequest {
    pub servers: BTreeMap<String, Option<bool>>,
}

impl UpdateWorkspaceMcpServersRequest {
    fn overrides(self) -> Result<BTreeMap<Uuid, Option<bool>>, Refusal> {
        self.servers
            .into_iter()
            .map(|(server, enabled)| match Uuid::parse_str(&server) {
                Ok(id) => Ok((id, enabled)),
                Err(_) => Err(Refusal::Identifier(server)),
            })
            .collect()
    }
}

fn unexpected(transport: Transport, field: &str, absent: bool) -> Result<(), ValidationError> {
    if absent {
        return Ok(());
    }
    Err(ValidationError::Unexpected {
        field: field.to_string(),
        transport,
    })
}

fn command(value: String) -> Result<String, Refusal> {
    validation::literal(validation::COMMAND, &value)?;
    Ok(value)
}

fn arguments(values: Vec<String>) -> Result<Vec<String>, Refusal> {
    for value in &values {
        validation::literal(validation::ARGUMENTS, value)?;
    }
    Ok(values)
}

fn url(value: &str) -> Result<String, Refusal> {
    Ok(validation::url(value)?.into())
}

fn secrets<'a>(
    field: &str,
    name: fn(&str) -> Result<(), ValidationError>,
    entries: impl Iterator<Item = (&'a String, Option<&'a SecretValue>)>,
) -> Result<(), ValidationError> {
    for (key, value) in entries {
        name(key)?;
        if let Some(value) = value {
            validation::literal(field, value.expose())?;
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    #[error("Invalid user ID in token")]
    Caller,
    #[error(transparent)]
    Invalid(#[from] ValidationError),
    #[error("{}", status::COMMANDS_DISABLED)]
    Commands,
    #[error("`transport` cannot be changed; remove the server and add it again")]
    Transport,
    #[error("A server with this name is already configured on this instance")]
    Taken,
    #[error("`{0}` is not an MCP server id")]
    Identifier(String),
    #[error("{0}")]
    Unreadable(String),
    #[error(transparent)]
    Server(#[from] McpServerError),
}

impl Refusal {
    fn status(&self) -> StatusCode {
        match self {
            Self::Caller => StatusCode::UNAUTHORIZED,
            Self::Invalid(_) | Self::Transport | Self::Identifier(_) | Self::Unreadable(_) => {
                StatusCode::BAD_REQUEST
            }
            Self::Commands => StatusCode::FORBIDDEN,
            Self::Taken => StatusCode::CONFLICT,
            Self::Server(error) => match error {
                McpServerError::Forbidden(_) => StatusCode::FORBIDDEN,
                McpServerError::NotFound(_) => StatusCode::NOT_FOUND,
                McpServerError::Invalid(_) => StatusCode::BAD_REQUEST,
                McpServerError::Conflict(_) => StatusCode::CONFLICT,
                McpServerError::Database(_) | McpServerError::Sealing(_) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            },
        }
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "MCP server storage failed");
            INTERNAL.to_string()
        } else {
            self.to_string()
        };
        (status, Json(ErrorResponse::new(message))).into_response()
    }
}

impl From<JsonRejection> for Refusal {
    fn from(rejection: JsonRejection) -> Self {
        Self::Unreadable(rejection.body_text())
    }
}

fn caller(auth: &AuthUser) -> Result<Uuid, Refusal> {
    Uuid::parse_str(&auth.0.sub).map_err(|_| Refusal::Caller)
}

fn transport(server: &McpServer) -> Transport {
    if server.command.is_some() {
        Transport::Stdio
    } else if server.transport == Some(McpTransport::Sse) {
        Transport::Sse
    } else {
        Transport::Http
    }
}

fn stored_transport(record: &McpServerRecord) -> Transport {
    transport(&load::definition(record).server)
}

async fn taken(state: &AppState, name: &str) -> bool {
    state.mcp().configured().await.servers.contains_key(name)
}

fn connection(
    hub: Option<&OrganizationHub>,
    definition: &Definition,
    policy: Policy,
) -> Connection {
    match hub {
        Some(hub) => hub.connection(definition.id),
        None => definition
            .known(policy)
            .unwrap_or_else(Connection::connecting),
    }
}

fn row(
    hub: Option<&OrganizationHub>,
    policy: Policy,
    record: &McpServerRecord,
) -> McpServerResponse {
    let definition = load::definition(record);
    let connection = connection(hub, &definition, policy);
    McpServerResponse::organization(record, transport(&definition.server), connection)
}

fn warm(state: &AppState, organization: Uuid) {
    let state = state.clone();
    tokio::spawn(async move {
        load::organization(&state, organization).await;
    });
}

fn answered(
    state: &AppState,
    organization: Uuid,
    hub: Option<&OrganizationHub>,
    record: &McpServerRecord,
) -> McpServerResponse {
    let response = row(hub, state.mcp().policy(), record);
    if hub.is_none() && response.status == Status::Connecting {
        warm(state, organization);
    }
    response
}

async fn server(
    state: &AppState,
    organization: Uuid,
    record: &McpServerRecord,
) -> McpServerResponse {
    let hub = load::sync(state, organization).await;
    answered(state, organization, hub.as_deref(), record)
}

async fn organization_servers(
    state: &AppState,
    organization: Uuid,
    records: &[McpServerRecord],
) -> Vec<McpServerResponse> {
    let hub = load::sync(state, organization).await;
    let policy = state.mcp().policy();
    let rows: Vec<McpServerResponse> = records
        .iter()
        .map(|record| row(hub.as_deref(), policy, record))
        .collect();
    if hub.is_none() && rows.iter().any(|row| row.status == Status::Connecting) {
        warm(state, organization);
    }
    rows
}

async fn instance_servers(state: &AppState) -> Vec<McpServerResponse> {
    let configuration = state.mcp().configured().await;
    if !configuration.servers.is_empty() && state.mcp().existing_instance().is_none() {
        let state = state.clone();
        tokio::spawn(async move {
            state.mcp().instance().await;
        });
    }
    configuration
        .servers
        .iter()
        .map(|(name, server)| {
            let connection = state.mcp().instance_connection(name, server);
            McpServerResponse::instance(name, server, connection)
        })
        .collect()
}

async fn workspace_servers(
    state: &AppState,
    organization: Uuid,
    records: &[McpServerRecord],
    overrides: &BTreeMap<Uuid, bool>,
) -> WorkspaceMcpServerListResponse {
    let mut servers = organization_servers(state, organization, records).await;
    servers.extend(instance_servers(state).await);
    WorkspaceMcpServerListResponse {
        servers: servers
            .into_iter()
            .map(|server| WorkspaceMcpServerResponse::of(server, overrides))
            .collect(),
    }
}

#[derive(Debug, Deserialize)]
pub struct McpServerPath {
    pub org_id: Uuid,
    pub server_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceMcpServersPath {
    pub org_id: Uuid,
    pub ws_id: Uuid,
}

/// GET /api/organizations/{org_id}/mcp-servers
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
) -> Result<Json<McpServerListResponse>, Refusal> {
    let user = caller(&auth)?;
    let records =
        mcp_servers::list_authorized(state.db(), state.master_key(), org_id, user).await?;
    let mut servers = organization_servers(&state, org_id, &records).await;
    servers.extend(instance_servers(&state).await);
    Ok(Json(McpServerListResponse {
        commands: state.config().mcp_commands,
        servers,
    }))
}

/// POST /api/organizations/{org_id}/mcp-servers
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(org_id): Path<Uuid>,
    request: Result<Json<CreateMcpServerRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<McpServerResponse>), Refusal> {
    let user = caller(&auth)?;
    let Json(request) = request?;
    mcp_servers::authorize_admin(state.db(), org_id, user).await?;
    let new = request.validated(state.config().mcp_commands)?;
    if taken(&state, &new.name).await {
        return Err(Refusal::Taken);
    }
    let record =
        mcp_servers::create_authorized(state.db(), state.master_key(), org_id, user, new).await?;
    let response = server(&state, org_id, &record).await;
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(org_id),
            workspace_id: None,
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::MCP_SERVER_CREATED,
            resource_type: resources::MCP_SERVER,
            resource_id: Some(record.id),
            old_values: None,
            new_values: serde_json::to_value(&response).ok(),
        },
    )
    .await;
    Ok((StatusCode::CREATED, Json(response)))
}

/// GET /api/organizations/{org_id}/mcp-servers/{server_id}
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<McpServerPath>,
) -> Result<Json<McpServerResponse>, Refusal> {
    let user = caller(&auth)?;
    let record = mcp_servers::get_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.server_id,
        user,
    )
    .await?;
    Ok(Json(server(&state, path.org_id, &record).await))
}

/// PUT /api/organizations/{org_id}/mcp-servers/{server_id}
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<McpServerPath>,
    request: Result<Json<UpdateMcpServerRequest>, JsonRejection>,
) -> Result<Json<McpServerResponse>, Refusal> {
    let user = caller(&auth)?;
    let Json(request) = request?;
    mcp_servers::authorize_admin(state.db(), path.org_id, user).await?;
    let stored = mcp_servers::get_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.server_id,
        user,
    )
    .await?;
    let changes = request.changes(&stored, state.config().mcp_commands)?;
    if let Some(name) = changes.name.as_deref()
        && name != stored.name
        && taken(&state, name).await
    {
        return Err(Refusal::Taken);
    }
    let before = row(
        state.mcp().existing_organization(path.org_id).as_deref(),
        state.mcp().policy(),
        &stored,
    );
    let record = mcp_servers::update_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.server_id,
        user,
        changes,
    )
    .await?;
    let response = server(&state, path.org_id, &record).await;
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(path.org_id),
            workspace_id: None,
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::MCP_SERVER_UPDATED,
            resource_type: resources::MCP_SERVER,
            resource_id: Some(record.id),
            old_values: serde_json::to_value(&before).ok(),
            new_values: serde_json::to_value(&response).ok(),
        },
    )
    .await;
    Ok(Json(response))
}

/// DELETE /api/organizations/{org_id}/mcp-servers/{server_id}
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<McpServerPath>,
) -> Result<StatusCode, Refusal> {
    let user = caller(&auth)?;
    let record =
        mcp_servers::delete_authorized(state.db(), path.org_id, path.server_id, user).await?;
    load::sync(&state, path.org_id).await;
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(path.org_id),
            workspace_id: None,
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::MCP_SERVER_DELETED,
            resource_type: resources::MCP_SERVER,
            resource_id: Some(record.id),
            old_values: serde_json::to_value(RemovedMcpServer::from(&record)).ok(),
            new_values: None,
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/organizations/{org_id}/mcp-servers/{server_id}/connection
pub async fn reconnect(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<McpServerPath>,
) -> Result<(StatusCode, Json<McpServerResponse>), Refusal> {
    let user = caller(&auth)?;
    mcp_servers::authorize_admin(state.db(), path.org_id, user).await?;
    let record = mcp_servers::get_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.server_id,
        user,
    )
    .await?;
    if stored_transport(&record) == Transport::Stdio && !state.config().mcp_commands {
        return Err(Refusal::Commands);
    }
    let hub = load::reconnect(&state, path.org_id, record.id).await;
    let response = answered(&state, path.org_id, hub.as_deref(), &record);
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(path.org_id),
            workspace_id: None,
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::MCP_SERVER_RECONNECTED,
            resource_type: resources::MCP_SERVER,
            resource_id: Some(record.id),
            old_values: None,
            new_values: serde_json::to_value(&response).ok(),
        },
    )
    .await;
    Ok((StatusCode::ACCEPTED, Json(response)))
}

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/mcp-servers
pub async fn list_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceMcpServersPath>,
) -> Result<Json<WorkspaceMcpServerListResponse>, Refusal> {
    let user = caller(&auth)?;
    let (records, overrides) = mcp_servers::workspace_list_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.ws_id,
        user,
    )
    .await?;
    Ok(Json(
        workspace_servers(&state, path.org_id, &records, &overrides).await,
    ))
}

/// PUT /api/organizations/{org_id}/workspaces/{ws_id}/mcp-servers
pub async fn update_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceMcpServersPath>,
    request: Result<Json<UpdateWorkspaceMcpServersRequest>, JsonRejection>,
) -> Result<Json<WorkspaceMcpServerListResponse>, Refusal> {
    let user = caller(&auth)?;
    let Json(request) = request?;
    let changes = request.overrides()?;
    let stored = mcp_servers::set_workspace_overrides_authorized(
        state.db(),
        path.org_id,
        path.ws_id,
        user,
        changes,
    )
    .await?;
    let read = mcp_servers::workspace_list_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.ws_id,
        user,
    )
    .await;
    let response = match read {
        Ok((records, overrides)) => {
            Ok(workspace_servers(&state, path.org_id, &records, &overrides).await)
        }
        Err(error) => Err(error),
    };
    let new_values = match &response {
        Ok(response) => serde_json::to_value(response).ok(),
        Err(_) => serde_json::to_value(StoredOverrides { overrides: &stored }).ok(),
    };
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(path.org_id),
            workspace_id: Some(path.ws_id),
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::WORKSPACE_MCP_SERVERS_UPDATED,
            resource_type: resources::WORKSPACE_MCP_SERVERS,
            resource_id: Some(path.ws_id),
            old_values: None,
            new_values,
        },
    )
    .await;
    Ok(Json(response?))
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::future::ready;
    use std::sync::Arc;

    use abnegate_agent::tool::Proxy;
    use chrono::Utc;
    use serde_json::Value;
    use serde_json::json;
    use zone_core::servers::mask;

    use super::*;
    use crate::auth::jwt::Claims;
    use crate::db::mcp_servers::Secrets;
    use crate::services::mcp_servers::hubs::Hubs;
    use crate::services::mcp_servers::testing;
    use crate::services::mcp_servers::testing::Tenant;

    const HEADER_SECRET: &str = "Bearer s3cr3t";
    const ENVIRONMENT_SECRET: &str = "v4lue";
    const REMOTE_URL: &str = "http://127.0.0.1:1/mcp";
    const ARGUMENT_SECRET: &str = "postgresql://operator:hunter2@db.internal/zone";
    const CREDENTIALED_URL: &str =
        "https://operator:hunter2@mcp.internal:8443/tenants/acme?key=hunter2#hunter2";
    const CREDENTIAL: &str = "hunter2";
    const REFERRING_URL: &str = "https://${MCP_HOST}:9443/tenants/${MCP_TENANT}?key=${MCP_KEY}";
    const BOUND_TOKEN: &str = "t0ken-bound-8d1f";

    const POLICY: Policy = Policy {
        enabled: true,
        commands: true,
    };
    const REMOTE: Policy = Policy {
        enabled: true,
        commands: false,
    };
    const OFF: Policy = Policy {
        enabled: false,
        commands: true,
    };

    fn secret(name: &str, value: &str) -> BTreeMap<String, SecretValue> {
        BTreeMap::from([(name.to_string(), SecretValue::from(value))])
    }

    fn record(transport: Transport) -> McpServerRecord {
        let stdio = transport == Transport::Stdio;
        McpServerRecord {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            name: "docs".to_string(),
            transport: transport.as_str().to_string(),
            command: stdio.then(|| "python3".to_string()),
            arguments: if stdio {
                vec!["stub.py".to_string()]
            } else {
                Vec::new()
            },
            url: (!stdio).then(|| REMOTE_URL.to_string()),
            tools: None,
            enabled: true,
            secrets: Secrets::default(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create(body: Value, commands: bool) -> Result<NewMcpServer, Refusal> {
        serde_json::from_value::<CreateMcpServerRequest>(body)
            .expect("the body is a create request")
            .validated(commands)
    }

    fn update(body: Value) -> UpdateMcpServerRequest {
        serde_json::from_value(body).expect("the body is an update request")
    }

    fn stdio_body() -> Value {
        json!({ "name": "docs", "transport": "stdio", "command": "python3", "arguments": ["stub.py"] })
    }

    fn remote_body() -> Value {
        json!({ "name": "docs", "transport": "http", "url": REMOTE_URL })
    }

    fn with(mut body: Value, field: &str, value: Value) -> Value {
        body[field] = value;
        body
    }

    fn placeholder(refusal: &Refusal) -> bool {
        matches!(
            refusal,
            Refusal::Invalid(ValidationError::Placeholder { .. })
        )
    }

    fn misplaced(refusal: &Refusal, field: &str, transport: Transport) -> bool {
        matches!(
            refusal,
            Refusal::Invalid(ValidationError::Unexpected { field: named, transport: given })
                if named == field && *given == transport
        )
    }

    fn operator() -> Vec<McpServerResponse> {
        let mut database = McpServer::command(
            "npx",
            [
                "-y",
                "@modelcontextprotocol/server-postgres",
                ARGUMENT_SECRET,
            ],
        );
        database.environment = secret("PGPASSWORD", ENVIRONMENT_SECRET);
        let mut remote = McpServer::remote(CREDENTIALED_URL);
        remote.headers = secret("Authorization", HEADER_SECRET);
        mask(&mut remote);
        let failed = Connection::failed(status::summary(
            "referring",
            &referring(),
            &format!("MCP handshake failed for 'referring': JSON-RPC error: -32000: {CREDENTIAL}"),
        ));
        vec![
            McpServerResponse::instance("database", &database, Connection::connecting()),
            McpServerResponse::instance("remote", &remote, Connection::connecting()),
            McpServerResponse::instance("referring", &referring(), failed),
        ]
    }

    /// An instance server as the instance layer holds it once the environment answered every
    /// reference: the URL and headers as written, the variables bound, and what they resolve
    /// to masked.
    fn referring() -> McpServer {
        let mut server = McpServer::remote(REFERRING_URL)
            .with_header("Authorization", "Bearer ${MCP_TOKEN}")
            .with_secret("MCP_HOST", "mcp.internal")
            .with_secret("MCP_TENANT", "acme")
            .with_secret("MCP_KEY", CREDENTIAL)
            .with_secret("MCP_TOKEN", BOUND_TOKEN);
        mask(&mut server);
        server
    }

    fn sealed(body: &Value) {
        let text = body.to_string();
        for secret in [
            CREDENTIAL,
            ENVIRONMENT_SECRET,
            "s3cr3t",
            "tenants/acme",
            BOUND_TOKEN,
            "MCP_",
            "${",
        ] {
            assert!(!text.contains(secret), "{secret} is published: {text}");
        }
    }

    #[test]
    fn a_response_names_a_servers_secrets_without_their_values() {
        let mut record = record(Transport::Http);
        record.tools = Some(vec!["search".to_string()]);
        record.secrets = Secrets {
            environment: secret("TOKEN", ENVIRONMENT_SECRET),
            headers: secret("Authorization", HEADER_SECRET),
        };

        let response = McpServerResponse::organization(
            &record,
            stored_transport(&record),
            Connection::failed(status::UNREACHABLE),
        );

        let body = serde_json::to_string(&response).unwrap();
        assert!(
            !body.contains("s3cr3t") && !body.contains(ENVIRONMENT_SECRET),
            "{body}"
        );
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({
                "id": record.id,
                "name": "docs",
                "source": "organization",
                "transport": "http",
                "command": null,
                "arguments": [],
                "url": REMOTE_URL,
                "tools": ["search"],
                "enabled": true,
                "environment": [{ "name": "TOKEN", "has_value": true }],
                "headers": [{ "name": "Authorization", "has_value": true }],
                "status": "failed",
                "error": "Zone could not reach the server",
                "available_tools": [],
                "created_at": record.created_at.to_rfc3339(),
                "updated_at": record.updated_at.to_rfc3339(),
            })
        );
    }

    #[test]
    fn an_instance_row_has_no_identity_and_names_its_secrets_only() {
        let mut server = McpServer::command("magents", ["mcp", "--token", CREDENTIAL]);
        server.environment = secret("TOKEN", ENVIRONMENT_SECRET);
        server.headers = secret("Authorization", HEADER_SECRET);

        let response = McpServerResponse::instance(
            "magents",
            &server,
            Connection::connected(vec!["spawn_session".to_string()]),
        );

        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({
                "id": null,
                "name": "magents",
                "source": "instance",
                "transport": "stdio",
                "command": "magents",
                "arguments": [],
                "url": null,
                "tools": null,
                "enabled": true,
                "environment": [{ "name": "TOKEN", "has_value": true }],
                "headers": [{ "name": "Authorization", "has_value": true }],
                "status": "connected",
                "error": null,
                "available_tools": ["spawn_session"],
                "created_at": null,
                "updated_at": null,
            })
        );
    }

    #[test]
    fn an_instance_url_is_published_as_its_origin_only() {
        for (url, published) in [
            (CREDENTIALED_URL, Some("https://mcp.internal:8443")),
            ("http://10.0.0.5/mcp?token=hunter2", Some("http://10.0.0.5")),
            ("https://example.com:443/a/b", Some("https://example.com")),
            ("https://${HOST}/mcp", None),
            ("file:///etc/passwd", None),
            ("not a url", None),
        ] {
            assert_eq!(origin(url).as_deref(), published, "{url}");
        }
    }

    #[test]
    fn an_operators_credentials_never_reach_a_list_or_an_audit_row() {
        let overrides = BTreeMap::new();
        let list = McpServerListResponse {
            commands: true,
            servers: operator(),
        };
        let workspace = WorkspaceMcpServerListResponse {
            servers: operator()
                .into_iter()
                .map(|server| WorkspaceMcpServerResponse::of(server, &overrides))
                .collect(),
        };

        let listed = serde_json::to_value(&list).unwrap();
        let audited = serde_json::to_value(&workspace).unwrap();

        sealed(&listed);
        sealed(&audited);
        for body in [&listed, &audited] {
            let database = &body["servers"][0];
            assert_eq!(database["command"], "npx");
            assert_eq!(database["arguments"], json!([]));
            assert_eq!(
                database["environment"],
                json!([{ "name": "PGPASSWORD", "has_value": true }])
            );
            let remote = &body["servers"][1];
            assert_eq!(remote["url"], "https://mcp.internal:8443");
            assert_eq!(
                remote["headers"],
                json!([{ "name": "Authorization", "has_value": true }])
            );
            let referring = &body["servers"][2];
            assert_eq!(referring["url"], "https://mcp.internal:9443");
            assert_eq!(
                referring["headers"],
                json!([{ "name": "Authorization", "has_value": true }])
            );
            assert_eq!(referring["environment"], json!([]));
            assert_eq!(referring["error"], status::REJECTED);
        }
    }

    #[test]
    fn an_instance_url_with_a_reference_is_published_as_its_resolved_origin() {
        let unbound = McpServer::remote(REFERRING_URL);
        let mut events = referring();
        events.transport = Some(McpTransport::Sse);

        let published = |server: &McpServer| {
            McpServerResponse::instance("referring", server, Connection::connecting()).url
        };

        assert_eq!(
            published(&referring()).as_deref(),
            Some("https://mcp.internal:9443")
        );
        assert_eq!(
            published(&events).as_deref(),
            Some("https://mcp.internal:9443")
        );
        assert_eq!(published(&unbound), None);
        assert_eq!(published(&McpServer::command("magents", ["mcp"])), None);
    }

    #[test]
    fn a_removed_server_is_audited_without_its_secrets() {
        let mut record = record(Transport::Http);
        record.secrets.headers = secret("Authorization", HEADER_SECRET);

        assert_eq!(
            serde_json::to_value(RemovedMcpServer::from(&record)).unwrap(),
            json!({
                "name": "docs",
                "transport": "http",
                "command": null,
                "arguments": [],
                "url": REMOTE_URL,
                "tools": null,
                "enabled": true,
            })
        );
    }

    #[test]
    fn a_server_with_no_hub_is_connecting_only_when_it_can_connect() {
        let stdio = record(Transport::Stdio);
        assert_eq!(
            connection(None, &load::definition(&stdio), POLICY),
            Connection::connecting()
        );
        assert_eq!(
            connection(None, &load::definition(&stdio), REMOTE),
            Connection::failed(status::COMMANDS_DISABLED)
        );
        assert_eq!(
            connection(None, &load::definition(&stdio), OFF),
            Connection::failed(status::MCP_DISABLED)
        );

        let mut disabled = record(Transport::Stdio);
        disabled.enabled = false;
        assert_eq!(
            connection(None, &load::definition(&disabled), POLICY),
            Connection::disabled()
        );

        let http = record(Transport::Http);
        for policy in [POLICY, REMOTE] {
            assert_eq!(
                connection(None, &load::definition(&http), policy),
                Connection::connecting()
            );
            assert_eq!(
                connection(None, &load::definition(&record(Transport::Sse)), policy),
                Connection::failed(status::SSE_UNSUPPORTED)
            );
        }
        assert_eq!(
            connection(None, &load::definition(&http), OFF),
            Connection::failed(status::MCP_DISABLED)
        );
        let mut disabled = http;
        disabled.enabled = false;
        assert_eq!(
            connection(None, &load::definition(&disabled), POLICY),
            Connection::disabled()
        );
    }

    #[tokio::test]
    async fn a_connection_is_read_by_the_rows_id_whatever_name_the_reader_holds() {
        let read = Definition::stub("before");
        let stored = Definition {
            name: "after".to_string(),
            ..read.clone().revised()
        };
        let hub = Hubs::new(Proxy::default(), POLICY)
            .organization(Uuid::new_v4(), |_| {
                ready(Ok::<_, Infallible>(Some(vec![stored].into())))
            })
            .await
            .expect("the rows load");

        assert_eq!(connection(Some(&hub), &read, POLICY), delegate());
    }

    #[test]
    fn a_server_reads_its_transport_back() {
        for transport in [Transport::Stdio, Transport::Http, Transport::Sse] {
            assert_eq!(stored_transport(&record(transport)), transport);
        }
    }

    #[test]
    fn a_workspace_row_is_effective_only_when_both_switches_are_on() {
        assert!(effective(true, None));
        assert!(effective(true, Some(true)));
        assert!(!effective(true, Some(false)));
        assert!(!effective(false, None));
        assert!(!effective(false, Some(true)));
    }

    #[test]
    fn a_workspace_row_carries_its_own_override_beside_the_server() {
        let record = record(Transport::Stdio);
        let other = Uuid::new_v4();
        let overrides = BTreeMap::from([(record.id, false), (other, true)]);
        let organization = row(None, POLICY, &record);
        let instance = McpServerResponse::instance(
            "magents",
            &McpServer::command("magents", ["mcp"]),
            Connection::connecting(),
        );

        let overridden = WorkspaceMcpServerResponse::of(organization.clone(), &overrides);
        let inherited = WorkspaceMcpServerResponse::of(organization.clone(), &BTreeMap::new());
        let shared = WorkspaceMcpServerResponse::of(instance, &overrides);

        assert_eq!(overridden.workspace_enabled, Some(false));
        assert!(!overridden.effective);
        assert_eq!(inherited.workspace_enabled, None);
        assert!(inherited.effective);
        assert_eq!(shared.workspace_enabled, None);
        assert!(shared.effective);

        let mut expected = serde_json::to_value(&organization).unwrap();
        expected["workspace_enabled"] = json!(false);
        expected["effective"] = json!(false);
        assert_eq!(serde_json::to_value(&overridden).unwrap(), expected);
    }

    #[test]
    fn an_update_tells_an_absent_field_from_null() {
        assert_eq!(update(json!({})).tools, None);
        assert_eq!(update(json!({ "tools": null })).tools, Some(None));
        assert_eq!(
            update(json!({ "tools": ["search"] })).tools,
            Some(Some(vec!["search".to_string()]))
        );

        assert!(update(json!({})).transport.is_none());
        assert!(update(json!({ "transport": null })).transport.is_some());
        assert!(update(json!({ "transport": "http" })).transport.is_some());

        assert!(update(json!({})).headers.is_none());
        let headers = update(json!({ "headers": { "Authorization": null, "X-Team": "docs" } }))
            .headers
            .expect("the headers are present");
        assert_eq!(headers["Authorization"], None);
        assert_eq!(headers["X-Team"], Some(SecretValue::from("docs")));
    }

    #[test]
    fn a_created_server_keeps_only_the_target_its_transport_reaches() {
        let stdio = create(with(stdio_body(), "url", json!(" ")), true).unwrap();
        assert_eq!(stdio.transport, "stdio");
        assert_eq!(stdio.command.as_deref(), Some("python3"));
        assert_eq!(stdio.arguments, ["stub.py"]);
        assert_eq!(stdio.url, None);
        assert_eq!(stdio.tools, None);
        assert!(stdio.enabled);

        let body = with(remote_body(), "command", json!(""));
        let body = with(body, "url", json!(format!(" {REMOTE_URL} ")));
        let body = with(body, "enabled", json!(false));
        let body = with(body, "tools", json!(["search"]));
        let body = with(body, "headers", json!({ "Authorization": HEADER_SECRET }));
        let remote = create(body, false).unwrap();
        assert_eq!(remote.transport, "http");
        assert_eq!(remote.command, None);
        assert_eq!(remote.url.as_deref(), Some(REMOTE_URL));
        assert_eq!(remote.tools, Some(vec!["search".to_string()]));
        assert!(!remote.enabled);
        assert_eq!(remote.headers, secret("Authorization", HEADER_SECRET));
    }

    #[test]
    fn a_placeholder_is_refused_in_every_field_a_server_stores() {
        let created = [
            with(stdio_body(), "command", json!("${SHELL}")),
            with(stdio_body(), "arguments", json!(["stub.py", "${HOME}"])),
            with(stdio_body(), "environment", json!({ "WHERE": "${HOME}" })),
            with(remote_body(), "url", json!("http://${HOST}/mcp")),
            with(
                remote_body(),
                "headers",
                json!({ "Authorization": "Bearer ${TOKEN}" }),
            ),
        ];
        for body in created {
            let refusal = create(body.clone(), true).expect_err("a placeholder is refused");
            assert!(placeholder(&refusal), "{body}: {refusal}");
            assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
        }

        let changed = [
            (Transport::Stdio, json!({ "command": "${SHELL}" })),
            (Transport::Stdio, json!({ "arguments": ["${HOME}"] })),
            (
                Transport::Stdio,
                json!({ "environment": { "WHERE": "${HOME}" } }),
            ),
            (Transport::Http, json!({ "url": "http://${HOST}/mcp" })),
            (
                Transport::Http,
                json!({ "headers": { "Authorization": "Bearer ${TOKEN}" } }),
            ),
        ];
        for (transport, body) in changed {
            let refusal = update(body.clone())
                .changes(&record(transport), true)
                .expect_err("a placeholder is refused");
            assert!(placeholder(&refusal), "{body}: {refusal}");
        }
    }

    #[test]
    fn a_definition_the_validators_refuse_is_a_bad_request() {
        let refused = [
            with(remote_body(), "transport", json!("sse")),
            with(remote_body(), "transport", json!("websocket")),
            with(remote_body(), "url", json!("http://u:p@host/mcp")),
            with(
                remote_body(),
                "name",
                json!("n".repeat(validation::NAME_LIMIT + 1)),
            ),
            with(remote_body(), "tools", json!(["not a tool"])),
            with(remote_body(), "headers", json!({ "Bad Header": "value" })),
            with(stdio_body(), "environment", json!({ "1ST": "value" })),
            json!({ "name": "docs", "transport": "http", "command": "python3" }),
            json!({ "name": "docs", "transport": "stdio", "url": REMOTE_URL }),
        ];
        for body in refused {
            let refusal = create(body.clone(), true).expect_err("the definition is refused");
            assert!(matches!(refusal, Refusal::Invalid(_)), "{body}: {refusal}");
            assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[test]
    fn a_field_the_transport_never_uses_is_refused() {
        let created = [
            (
                with(stdio_body(), "headers", json!({ "Authorization": "x" })),
                validation::HEADERS,
                Transport::Stdio,
            ),
            (
                with(remote_body(), "environment", json!({ "TOKEN": "x" })),
                validation::ENVIRONMENT,
                Transport::Http,
            ),
            (
                with(remote_body(), "arguments", json!(["--quiet"])),
                validation::ARGUMENTS,
                Transport::Http,
            ),
        ];
        for (body, field, transport) in created {
            let refusal = create(body.clone(), true).expect_err("the field is refused");
            assert!(misplaced(&refusal, field, transport), "{body}: {refusal}");
            assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
        }

        let changed = [
            (
                Transport::Stdio,
                json!({ "headers": { "Authorization": null } }),
                validation::HEADERS,
            ),
            (
                Transport::Http,
                json!({ "environment": { "TOKEN": "x" } }),
                validation::ENVIRONMENT,
            ),
            (
                Transport::Http,
                json!({ "arguments": ["--quiet"] }),
                validation::ARGUMENTS,
            ),
            (
                Transport::Http,
                json!({ "command": "python3" }),
                validation::COMMAND,
            ),
        ];
        for (transport, body, field) in changed {
            let refusal = update(body.clone())
                .changes(&record(transport), true)
                .expect_err("the field is refused");
            assert!(misplaced(&refusal, field, transport), "{body}: {refusal}");
        }
    }

    #[test]
    fn an_empty_field_the_transport_never_uses_is_let_through() {
        let body = with(stdio_body(), "headers", json!({}));
        assert!(create(body, true).is_ok());
        let body = with(remote_body(), "environment", json!({}));
        let body = with(body, "arguments", json!([]));
        assert!(create(body, true).is_ok());

        let cleared = update(json!({ "headers": {} }))
            .changes(&record(Transport::Stdio), true)
            .expect("an empty map removes what a server never uses");
        assert_eq!(cleared.headers, Some(BTreeMap::new()));
        let cleared = update(json!({ "environment": {}, "arguments": [] }))
            .changes(&record(Transport::Http), true)
            .expect("an empty map removes what a server never uses");
        assert_eq!(cleared.environment, Some(BTreeMap::new()));
        assert_eq!(cleared.arguments, Some(Vec::new()));
    }

    #[test]
    fn a_command_server_is_refused_while_commands_are_off() {
        let created = create(stdio_body(), false).expect_err("commands are off");
        assert!(matches!(created, Refusal::Commands), "{created}");
        assert_eq!(created.status(), StatusCode::FORBIDDEN);
        assert_eq!(created.to_string(), status::COMMANDS_DISABLED);

        let changed = update(json!({ "enabled": false }))
            .changes(&record(Transport::Stdio), false)
            .expect_err("commands are off");
        assert!(matches!(changed, Refusal::Commands), "{changed}");

        let remote = update(json!({ "enabled": false }))
            .changes(&record(Transport::Http), false)
            .expect("a remote server does not need commands");
        assert_eq!(remote.enabled, Some(false));
    }

    #[test]
    fn an_update_keeps_the_transport_the_server_was_made_with() {
        for body in [
            json!({ "transport": "stdio" }),
            json!({ "transport": null }),
        ] {
            let refusal = update(body)
                .changes(&record(Transport::Http), true)
                .expect_err("the transport is fixed");
            assert!(matches!(refusal, Refusal::Transport), "{refusal}");
            assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
        }

        let crossed = [
            (Transport::Stdio, json!({ "url": REMOTE_URL })),
            (Transport::Stdio, json!({ "command": " " })),
            (Transport::Http, json!({ "command": "python3" })),
            (Transport::Http, json!({ "url": "" })),
        ];
        for (transport, body) in crossed {
            let refusal = update(body.clone())
                .changes(&record(transport), true)
                .expect_err("the target does not fit the transport");
            assert!(matches!(refusal, Refusal::Invalid(_)), "{body}: {refusal}");
        }
    }

    #[test]
    fn an_update_changes_only_what_its_body_names() {
        let untouched = update(json!({}))
            .changes(&record(Transport::Stdio), true)
            .unwrap();
        assert_eq!(untouched.name, None);
        assert_eq!(untouched.command, None);
        assert_eq!(untouched.arguments, None);
        assert_eq!(untouched.url, None);
        assert_eq!(untouched.tools, None);
        assert_eq!(untouched.enabled, None);
        assert_eq!(untouched.environment, None);
        assert_eq!(untouched.headers, None);

        let changed = update(json!({
            "name": "renamed",
            "arguments": ["stub.py", "--quiet"],
            "tools": null,
            "environment": { "TOKEN": null, "FRESH": ENVIRONMENT_SECRET },
        }))
        .changes(&record(Transport::Stdio), true)
        .unwrap();
        assert_eq!(changed.name.as_deref(), Some("renamed"));
        assert_eq!(
            changed.arguments,
            Some(vec!["stub.py".to_string(), "--quiet".to_string()])
        );
        assert_eq!(changed.tools, Some(None));
        assert_eq!(
            changed.environment,
            Some(BTreeMap::from([
                (
                    "FRESH".to_string(),
                    Some(SecretValue::from(ENVIRONMENT_SECRET))
                ),
                ("TOKEN".to_string(), None),
            ]))
        );
    }

    #[test]
    fn a_workspace_override_is_keyed_by_a_server_id() {
        let server = Uuid::new_v4();
        let cleared = Uuid::new_v4();
        let request: UpdateWorkspaceMcpServersRequest = serde_json::from_value(json!({
            "servers": { server.to_string(): false, cleared.to_string(): null }
        }))
        .unwrap();
        assert_eq!(
            request.overrides().unwrap(),
            BTreeMap::from([(server, Some(false)), (cleared, None)])
        );

        let request: UpdateWorkspaceMcpServersRequest =
            serde_json::from_value(json!({ "servers": { "docs": false } })).unwrap();
        let refusal = request.overrides().expect_err("a name is not an id");
        assert!(matches!(&refusal, Refusal::Identifier(key) if key == "docs"));
        assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_storage_error_maps_to_its_status() {
        let statuses = [
            (McpServerError::Forbidden("no"), StatusCode::FORBIDDEN),
            (McpServerError::NotFound("gone"), StatusCode::NOT_FOUND),
            (
                McpServerError::Invalid("bad".to_string()),
                StatusCode::BAD_REQUEST,
            ),
            (McpServerError::Conflict("taken"), StatusCode::CONFLICT),
            (
                McpServerError::Database(sqlx::Error::PoolClosed),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                McpServerError::Sealing("unsealable".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (error, status) in statuses {
            assert_eq!(Refusal::from(error).status(), status);
        }
        assert_eq!(Refusal::Taken.status(), StatusCode::CONFLICT);
        assert_eq!(Refusal::Caller.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_storage_failure_answers_without_its_cause() {
        let response = Refusal::from(McpServerError::Sealing("the key is 0xC0FFEE".to_string()))
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "error": INTERNAL })
        );

        let refused = Refusal::from(McpServerError::Conflict(
            "A server with this name already exists",
        ))
        .into_response();
        assert_eq!(refused.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(refused.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "error": "A server with this name already exists" })
        );
    }

    fn delegate() -> Connection {
        Connection::connected(vec!["delegate".to_string()])
    }

    fn administrator(tenant: &Tenant) -> AuthUser {
        AuthUser(Claims {
            sub: tenant.user.to_string(),
            email: tenant.email.clone(),
            roles: Vec::new(),
            permissions: Vec::new(),
            exp: i64::MAX,
            iat: 0,
            jti: String::new(),
            is_admin: false,
        })
    }

    fn stub_body(name: &str) -> Value {
        let stub = testing::stub(name);
        json!({
            "name": name,
            "transport": "stdio",
            "command": stub.command,
            "arguments": stub.arguments,
        })
    }

    fn path(tenant: &Tenant, server: &McpServerResponse) -> Path<McpServerPath> {
        Path(McpServerPath {
            org_id: tenant.organization,
            server_id: server.id.expect("an organization's server has an id"),
        })
    }

    async fn creating(state: &AppState, tenant: &Tenant, body: Value) -> McpServerResponse {
        let request = serde_json::from_value(body).expect("the body is a create request");
        let (status, Json(response)) = super::create(
            State(state.clone()),
            administrator(tenant),
            Path(tenant.organization),
            Ok(Json(request)),
        )
        .await
        .expect("the server is created");
        assert_eq!(status, StatusCode::CREATED);
        response
    }

    async fn updating(
        state: &AppState,
        tenant: &Tenant,
        server: &McpServerResponse,
        body: Value,
    ) -> McpServerResponse {
        let Json(response) = super::update(
            State(state.clone()),
            administrator(tenant),
            path(tenant, server),
            Ok(Json(update(body))),
        )
        .await
        .expect("the server is updated");
        response
    }

    async fn reconnecting(
        state: &AppState,
        tenant: &Tenant,
        server: &McpServerResponse,
    ) -> McpServerResponse {
        let (status, Json(response)) = reconnect(
            State(state.clone()),
            administrator(tenant),
            path(tenant, server),
        )
        .await
        .expect("the reconnect is accepted");
        assert_eq!(status, StatusCode::ACCEPTED);
        response
    }

    async fn reading(
        state: &AppState,
        tenant: &Tenant,
        server: &McpServerResponse,
    ) -> McpServerResponse {
        let Json(response) = get(
            State(state.clone()),
            administrator(tenant),
            path(tenant, server),
        )
        .await
        .expect("the server is read");
        response
    }

    async fn listing(state: &AppState, tenant: &Tenant) -> Vec<McpServerResponse> {
        let Json(response) = list(
            State(state.clone()),
            administrator(tenant),
            Path(tenant.organization),
        )
        .await
        .expect("the servers are listed");
        response
            .servers
            .into_iter()
            .filter(|server| server.source == Source::Organization)
            .collect()
    }

    #[tokio::test]
    async fn every_write_reaches_a_hub_that_is_already_built() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let hub = load::organization(&state, tenant.organization)
            .await
            .expect("the organization's hub");

        let created = creating(&state, &tenant, stub_body("stub")).await;
        load::organization(&state, tenant.organization).await;
        let connected = hub.named("stub");
        let generation = hub.generation("stub");

        let narrowed = updating(&state, &tenant, &created, json!({ "tools": ["other"] })).await;
        let kept = hub.generation("stub");

        let renamed = updating(&state, &tenant, &created, json!({ "name": "renamed" })).await;
        let ghost = hub.named("stub");
        load::organization(&state, tenant.organization).await;
        let session = hub
            .session("renamed")
            .expect("the renamed server connected");

        let requested = reconnecting(&state, &tenant, &created).await;
        load::organization(&state, tenant.organization).await;
        let replaced = hub
            .session("renamed")
            .is_some_and(|replaced| !Arc::ptr_eq(&replaced, &session));

        let removed = delete(
            State(state.clone()),
            administrator(&tenant),
            path(&tenant, &created),
        )
        .await
        .expect("the server is deleted");
        let left = hub.names();
        tenant.remove().await;

        assert_eq!(created.status, Status::Connecting);
        assert_eq!(connected, delegate());
        assert_eq!(narrowed.status, Status::Connected);
        assert_eq!(narrowed.tools, Some(vec!["other".to_string()]));
        assert_eq!(kept, generation, "an allowlist edit reconnected the server");
        assert_eq!(renamed.status, Status::Connecting);
        assert_eq!(ghost, Connection::failed(status::UNCONNECTED));
        assert_eq!(requested.status, Status::Connecting);
        assert!(replaced, "a reconnect kept the session");
        assert_eq!(removed, StatusCode::NO_CONTENT);
        assert!(left.is_empty(), "a deleted server kept its slot: {left:?}");
    }

    #[tokio::test]
    async fn a_reconnect_answers_with_the_servers_own_connection() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let mut body = stub_body("off");
        body["enabled"] = json!(false);
        let off = creating(&state, &tenant, body).await;

        let unbuilt = reconnecting(&state, &tenant, &off).await;
        let hubs = state.mcp().organizations();
        let remote = creating(
            &state,
            &tenant,
            json!({ "name": "remote", "transport": "http", "url": REMOTE_URL }),
        )
        .await;
        let first = reconnecting(&state, &tenant, &remote).await;
        load::organization(&state, tenant.organization).await;
        let refused = reading(&state, &tenant, &remote).await;
        let built = [
            reconnecting(&state, &tenant, &remote).await,
            reconnecting(&state, &tenant, &off).await,
        ];
        let stub = creating(&state, &tenant, stub_body("stub")).await;
        load::organization(&state, tenant.organization).await;
        let settled = reading(&state, &tenant, &stub).await;
        let requested = reconnecting(&state, &tenant, &stub).await;
        load::organization(&state, tenant.organization).await;
        tenant.remove().await;

        assert_eq!(
            hubs, 0,
            "a reconnect of a server that cannot connect built a hub"
        );
        let [again, disabled] = built;
        for off in [unbuilt, disabled] {
            assert_eq!(off.status, Status::Disabled);
            assert_eq!(off.error, None);
        }
        for connecting in [remote, first, again] {
            assert_eq!(connecting.status, Status::Connecting);
            assert_eq!(connecting.error, None);
            assert_eq!(connecting.available_tools, Vec::<String>::new());
        }
        assert_eq!(refused.status, Status::Failed);
        assert_eq!(refused.error.as_deref(), Some(status::UNREACHABLE));
        assert_eq!(settled.status, Status::Connected);
        assert_eq!(requested.status, Status::Connecting);
        assert_eq!(requested.available_tools, Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_read_shows_what_another_replica_wrote() {
        let tenant = Tenant::new().await;
        let writer = tenant.replica().await;
        let reader = tenant.replica().await;
        load::organization(&reader, tenant.organization).await;

        let created = creating(&writer, &tenant, stub_body("stub")).await;
        let listed = listing(&reader, &tenant).await;
        load::organization(&reader, tenant.organization).await;
        let read = reading(&reader, &tenant, &created).await;
        updating(&writer, &tenant, &created, json!({ "name": "renamed" })).await;
        let renamed = listing(&reader, &tenant).await;
        tenant.delete(created.id.expect("a stored server")).await;
        let emptied = listing(&reader, &tenant).await;
        let left = reader
            .mcp()
            .existing_organization(tenant.organization)
            .map(|hub| hub.names());
        load::organization(&writer, tenant.organization).await;
        tenant.remove().await;

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, Status::Connecting);
        assert_eq!(read.status, Status::Connected);
        assert_eq!(read.available_tools, ["delegate"]);
        assert_eq!(renamed.len(), 1);
        assert_eq!(renamed[0].name, "renamed");
        assert_eq!(renamed[0].status, Status::Connecting);
        assert!(emptied.is_empty());
        assert_eq!(left, Some(Vec::new()));
    }
}
