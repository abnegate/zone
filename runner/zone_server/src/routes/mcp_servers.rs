//! Organization MCP server endpoints

use std::collections::BTreeMap;

use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::McpTransport;
use abnegate_secret::SecretValue;
use axum::Json;
use axum::extract::Path;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::IgnoredAny;
use uuid::Uuid;
use zone_core::servers;

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
use crate::services::mcp_servers::status::Status;
use crate::services::mcp_servers::transport::Transport;
use crate::services::mcp_servers::validation;
use crate::services::mcp_servers::validation::ValidationError;
use crate::state::AppState;

use super::common::AuditEvent;
use super::common::ErrorResponse;
use super::common::audit;

const COMMAND: &str = "command";
const ARGUMENTS: &str = "arguments";
const ENVIRONMENT: &str = "environment";
const HEADERS: &str = "headers";
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
            arguments: server.arguments.clone(),
            url: server.url.clone(),
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

    fn connecting(self) -> Self {
        let connection = Connection::connecting();
        Self {
            status: connection.status,
            error: connection.error,
            available_tools: connection.tools,
            ..self
        }
    }
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
            Transport::Stdio => (self.command.map(command).transpose()?, None),
            Transport::Http | Transport::Sse => (None, self.url.as_deref().map(url).transpose()?),
        };
        let arguments = arguments(self.arguments)?;
        if let Some(tools) = &self.tools {
            validation::tools(tools)?;
        }
        secrets(
            ENVIRONMENT,
            validation::environment_name,
            self.environment
                .iter()
                .map(|(name, value)| (name, Some(value))),
        )?;
        secrets(
            HEADERS,
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
        validation::target(
            if stdio {
                Transport::Stdio
            } else {
                Transport::Http
            },
            self.command.as_deref().or(stored.command.as_deref()),
            self.url.as_deref().or(stored.url.as_deref()),
        )?;
        let (command, url) = if stdio {
            (self.command.map(command).transpose()?, None)
        } else {
            (None, self.url.as_deref().map(url).transpose()?)
        };
        let arguments = self.arguments.map(arguments).transpose()?;
        if let Some(Some(tools)) = &self.tools {
            validation::tools(tools)?;
        }
        if let Some(environment) = &self.environment {
            secrets(
                ENVIRONMENT,
                validation::environment_name,
                environment
                    .iter()
                    .map(|(name, value)| (name, value.as_ref())),
            )?;
        }
        if let Some(headers) = &self.headers {
            secrets(
                HEADERS,
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

fn command(value: String) -> Result<String, Refusal> {
    validation::literal(COMMAND, &value)?;
    Ok(value)
}

fn arguments(values: Vec<String>) -> Result<Vec<String>, Refusal> {
    for value in &values {
        validation::literal(ARGUMENTS, value)?;
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
    #[error(transparent)]
    Server(#[from] McpServerError),
}

impl Refusal {
    fn status(&self) -> StatusCode {
        match self {
            Self::Caller => StatusCode::UNAUTHORIZED,
            Self::Invalid(_) | Self::Transport | Self::Identifier(_) => StatusCode::BAD_REQUEST,
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

fn taken(name: &str) -> bool {
    servers::config().servers.contains_key(name)
}

fn connection(
    hub: Option<&OrganizationHub>,
    definition: &Definition,
    commands: bool,
) -> Connection {
    match hub {
        Some(hub) => hub.connection(&definition.name),
        None => status::static_status(definition, commands).unwrap_or_else(Connection::connecting),
    }
}

fn row(
    hub: Option<&OrganizationHub>,
    commands: bool,
    record: &McpServerRecord,
) -> McpServerResponse {
    let definition = load::definition(record);
    let connection = connection(hub, &definition, commands);
    McpServerResponse::organization(record, transport(&definition.server), connection)
}

fn warm(state: &AppState, organization: Uuid) {
    let state = state.clone();
    tokio::spawn(async move {
        state
            .mcp()
            .organization(organization, || load::definitions(&state, organization))
            .await;
    });
}

fn server(state: &AppState, organization: Uuid, record: &McpServerRecord) -> McpServerResponse {
    let hub = state.mcp().existing_organization(organization);
    let response = row(hub.as_deref(), state.config().mcp_commands, record);
    if hub.is_none() && response.status == Status::Connecting {
        warm(state, organization);
    }
    response
}

fn organization_servers(
    state: &AppState,
    organization: Uuid,
    records: &[McpServerRecord],
) -> Vec<McpServerResponse> {
    let hub = state.mcp().existing_organization(organization);
    let commands = state.config().mcp_commands;
    let rows: Vec<McpServerResponse> = records
        .iter()
        .map(|record| row(hub.as_deref(), commands, record))
        .collect();
    if hub.is_none() && rows.iter().any(|row| row.status == Status::Connecting) {
        warm(state, organization);
    }
    rows
}

fn instance_servers(state: &AppState) -> Vec<McpServerResponse> {
    let configured = servers::config().servers;
    if !configured.is_empty() && state.mcp().existing_instance().is_none() {
        let state = state.clone();
        tokio::spawn(async move {
            state.mcp().instance().await;
        });
    }
    configured
        .iter()
        .map(|(name, server)| {
            McpServerResponse::instance(name, server, state.mcp().instance_connection(name))
        })
        .collect()
}

fn workspace_servers(
    state: &AppState,
    organization: Uuid,
    records: &[McpServerRecord],
    overrides: &BTreeMap<Uuid, bool>,
) -> WorkspaceMcpServerListResponse {
    WorkspaceMcpServerListResponse {
        servers: organization_servers(state, organization, records)
            .into_iter()
            .chain(instance_servers(state))
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
    let mut servers = organization_servers(&state, org_id, &records);
    servers.extend(instance_servers(&state));
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
    Json(request): Json<CreateMcpServerRequest>,
) -> Result<(StatusCode, Json<McpServerResponse>), Refusal> {
    let user = caller(&auth)?;
    mcp_servers::authorize_admin(state.db(), org_id, user).await?;
    let new = request.validated(state.config().mcp_commands)?;
    if taken(&new.name) {
        return Err(Refusal::Taken);
    }
    let record =
        mcp_servers::create_authorized(state.db(), state.master_key(), org_id, user, new).await?;
    state.mcp().refresh(org_id, load::definition(&record));
    let response = server(&state, org_id, &record);
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
    Ok(Json(server(&state, path.org_id, &record)))
}

/// PUT /api/organizations/{org_id}/mcp-servers/{server_id}
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<McpServerPath>,
    Json(request): Json<UpdateMcpServerRequest>,
) -> Result<Json<McpServerResponse>, Refusal> {
    let user = caller(&auth)?;
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
    if changes
        .name
        .as_deref()
        .is_some_and(|name| name != stored.name && taken(name))
    {
        return Err(Refusal::Taken);
    }
    let (record, _) = mcp_servers::update_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.server_id,
        user,
        changes,
    )
    .await?;
    if record.name != stored.name {
        state.mcp().remove(path.org_id, &stored.name);
    }
    state.mcp().refresh(path.org_id, load::definition(&record));
    let response = server(&state, path.org_id, &record);
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
            old_values: None,
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
    state.mcp().remove(path.org_id, &record.name);
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
    state.mcp().remove(path.org_id, &record.name);
    state.mcp().refresh(path.org_id, load::definition(&record));
    let current = server(&state, path.org_id, &record);
    let response = if record.enabled {
        current.connecting()
    } else {
        current
    };
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
    Ok(Json(workspace_servers(
        &state,
        path.org_id,
        &records,
        &overrides,
    )))
}

/// PUT /api/organizations/{org_id}/workspaces/{ws_id}/mcp-servers
pub async fn update_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(path): Path<WorkspaceMcpServersPath>,
    Json(request): Json<UpdateWorkspaceMcpServersRequest>,
) -> Result<Json<WorkspaceMcpServerListResponse>, Refusal> {
    let user = caller(&auth)?;
    let changes = request.overrides()?;
    mcp_servers::set_workspace_overrides_authorized(
        state.db(),
        path.org_id,
        path.ws_id,
        user,
        changes,
    )
    .await?;
    let (records, overrides) = mcp_servers::workspace_list_authorized(
        state.db(),
        state.master_key(),
        path.org_id,
        path.ws_id,
        user,
    )
    .await?;
    let response = workspace_servers(&state, path.org_id, &records, &overrides);
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
            new_values: serde_json::to_value(&response).ok(),
        },
    )
    .await;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::Value;
    use serde_json::json;

    use super::*;
    use crate::db::mcp_servers::Secrets;

    const HEADER_SECRET: &str = "Bearer s3cr3t";
    const ENVIRONMENT_SECRET: &str = "v4lue";
    const REMOTE_URL: &str = "http://127.0.0.1:1/mcp";

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

    #[test]
    fn a_response_names_a_servers_secrets_without_their_values() {
        let mut record = record(Transport::Http);
        record.tools = Some(vec!["search".to_string()]);
        record.secrets = Secrets {
            environment: secret("TOKEN", ENVIRONMENT_SECRET),
            headers: secret("Authorization", HEADER_SECRET),
        };

        let response = row(None, true, &record);

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
                "error": status::HTTP_PENDING,
                "available_tools": [],
                "created_at": record.created_at.to_rfc3339(),
                "updated_at": record.updated_at.to_rfc3339(),
            })
        );
    }

    #[test]
    fn an_instance_row_has_no_identity_and_names_its_secrets_only() {
        let mut server = McpServer::command("magents", ["mcp"]);
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
                "arguments": ["mcp"],
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
            connection(None, &load::definition(&stdio), true),
            Connection::connecting()
        );
        assert_eq!(
            connection(None, &load::definition(&stdio), false),
            Connection::failed(status::COMMANDS_DISABLED)
        );

        let mut disabled = record(Transport::Stdio);
        disabled.enabled = false;
        assert_eq!(
            connection(None, &load::definition(&disabled), true),
            Connection::disabled()
        );

        assert_eq!(
            connection(None, &load::definition(&record(Transport::Http)), true),
            Connection::failed(status::HTTP_PENDING)
        );
        assert_eq!(
            connection(None, &load::definition(&record(Transport::Sse)), true),
            Connection::failed(status::SSE_UNSUPPORTED)
        );
    }

    #[test]
    fn a_reconnect_answers_connecting_whatever_the_server_showed() {
        let failed = row(None, true, &record(Transport::Http));
        assert_eq!(failed.status, Status::Failed);

        let response = failed.clone().connecting();

        assert_eq!(
            response,
            McpServerResponse {
                status: Status::Connecting,
                error: None,
                available_tools: Vec::new(),
                ..failed
            }
        );
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
        let organization = row(None, true, &record);
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
}
