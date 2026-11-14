//! An organization that stores MCP servers in the test database, and the replicas that serve it.

use sqlx::PgPool;
use uuid::Uuid;
use zone_core::variables::Variables;

use crate::config::Config;
use crate::db::mcp_servers;
use crate::db::mcp_servers::McpServerChanges;
use crate::db::mcp_servers::McpServerRecord;
use crate::db::mcp_servers::NewMcpServer;
use crate::db::organization_members;
use crate::db::organization_members::OrgRole;
use crate::db::organizations;
use crate::db::users;
use crate::db::workspace_members;
use crate::db::workspace_members::WorkspaceRole;
use crate::db::workspaces;
use crate::state::AppState;
use crate::state::test_config;

use super::definition::Definition;
use super::transport::Transport;

pub(crate) const REMOTE_URL: &str = "http://127.0.0.1:1/mcp";
pub(crate) const SERVERS: &str = "ZONE_MCP_SERVERS";

const ENABLED: &str = "ZONE_MCP_ENABLED";
const CONFIGURATION: &str = "ZONE_MCP_CONFIG";
const AUTO_CONNECT: &str = "ZONE_MCP_AUTO_CONNECT";
const AUTO_MAGENTS: &str = "ZONE_MCP_AUTO_MAGENTS";
const NONE: &str = r#"{"mcpServers":{}}"#;
const OFF: &str = "false";

/// The process environment with the master switch as shipped and no instance MCP server, held
/// for the caller: not `magents` on the PATH, nor what the shell exports, nor a
/// `~/.zone/mcp.json`. That file is read whenever neither `ZONE_MCP_SERVERS` nor
/// `ZONE_MCP_CONFIG` names a document, so the first carries a document with no server in it
/// instead of being cleared.
pub(crate) fn isolated() -> Variables {
    let mut variables = Variables::isolated(&[ENABLED, CONFIGURATION, AUTO_MAGENTS]);
    variables.set(SERVERS, NONE);
    variables.set(AUTO_CONNECT, OFF);
    variables
}

/// A state that took nothing about MCP servers from the machine the test runs on, and attaches
/// no instance server.
pub(crate) async fn state(config: Config, pool: PgPool) -> AppState {
    let environment = isolated();
    let state = AppState::new(config, pool, None);
    // The instance's servers are read on first use, so they are read here, before `environment`
    // gives the lock back to the other tests.
    state.mcp().configured().await;
    drop(environment);
    state.disable_mcp();
    state
}

/// [`state`] on an instance whose operator turned MCP off.
pub(crate) async fn switched_off(config: Config, pool: PgPool) -> AppState {
    state(
        Config {
            mcp_enabled: false,
            ..config
        },
        pool,
    )
    .await
}

pub(crate) struct Tenant {
    pub pool: PgPool,
    pub organization: Uuid,
    pub workspace: Uuid,
    pub user: Uuid,
    pub email: String,
}

impl Tenant {
    pub(crate) async fn new() -> Self {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("{suffix}@example.com");
        let user = users::create_user(&pool, &email, "hash", None, false)
            .await
            .expect("a user")
            .id;
        let organization = organizations::create_organization(&pool, "Servers", &suffix, None)
            .await
            .expect("an organization")
            .id;
        let workspace = workspaces::create_workspace(&pool, organization, "Servers", &suffix, None)
            .await
            .expect("a workspace")
            .id;
        organization_members::add_member(&pool, organization, user, OrgRole::Admin, None)
            .await
            .expect("an organization administrator");
        workspace_members::add_member(&pool, workspace, user, WorkspaceRole::Member, None)
            .await
            .expect("a workspace member");
        Self {
            pool,
            organization,
            workspace,
            user,
            email,
        }
    }

    /// A server process of its own over the tenant's database, which runs an organization's
    /// commands and attaches no instance server.
    pub(crate) async fn replica(&self) -> AppState {
        self.configured(Config {
            mcp_commands: true,
            ..test_config()
        })
        .await
    }

    pub(crate) async fn configured(&self, config: Config) -> AppState {
        state(config, self.pool.clone()).await
    }

    pub(crate) async fn store(&self, state: &AppState, new: NewMcpServer) -> McpServerRecord {
        mcp_servers::create_authorized(
            &self.pool,
            state.master_key(),
            self.organization,
            self.user,
            new,
        )
        .await
        .expect("the server is stored")
    }

    pub(crate) async fn change(
        &self,
        state: &AppState,
        server: Uuid,
        changes: McpServerChanges,
    ) -> McpServerRecord {
        mcp_servers::update_authorized(
            &self.pool,
            state.master_key(),
            self.organization,
            server,
            self.user,
            changes,
        )
        .await
        .expect("the server is changed")
    }

    pub(crate) async fn delete(&self, server: Uuid) {
        mcp_servers::delete_authorized(&self.pool, self.organization, server, self.user)
            .await
            .expect("the server is deleted");
    }

    pub(crate) async fn overriding(&self, server: Uuid, enabled: bool) {
        sqlx::query(
            "INSERT INTO workspace_mcp_servers (workspace_id, server_id, organization_id, enabled) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (workspace_id, server_id) DO UPDATE SET enabled = EXCLUDED.enabled",
        )
        .bind(self.workspace)
        .bind(server)
        .bind(self.organization)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .expect("the workspace's override is stored");
    }

    pub(crate) async fn remove(self) {
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(self.organization)
            .execute(&self.pool)
            .await
            .expect("the organization is removed");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(self.user)
            .execute(&self.pool)
            .await
            .expect("the user is removed");
    }
}

pub(crate) fn stub(name: &str) -> NewMcpServer {
    let stub = Definition::stub(name).server;
    NewMcpServer {
        name: name.to_string(),
        transport: Transport::Stdio.as_str().to_string(),
        command: stub.command,
        arguments: stub.arguments,
        url: None,
        tools: None,
        enabled: true,
        environment: Default::default(),
        headers: Default::default(),
    }
}

pub(crate) fn remote(name: &str) -> NewMcpServer {
    NewMcpServer {
        name: name.to_string(),
        transport: Transport::Http.as_str().to_string(),
        command: None,
        arguments: Vec::new(),
        url: Some(REMOTE_URL.to_string()),
        tools: None,
        enabled: true,
        environment: Default::default(),
        headers: Default::default(),
    }
}
