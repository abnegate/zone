use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use abnegate_agent::Tool;
use abnegate_agent::mcp::McpConfig;
use abnegate_agent::mcp::McpHub;
use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::SEPARATOR;
use abnegate_agent::tool::Proxy;
use uuid::Uuid;

pub(super) type Session = Arc<McpHub>;

pub(super) struct Sessions {
    proxy: Proxy,
    hubs: BTreeMap<Uuid, Session>,
}

impl Sessions {
    pub(super) fn new(proxy: Proxy) -> Self {
        Self {
            proxy,
            hubs: BTreeMap::new(),
        }
    }

    pub(super) fn proxy(&self) -> Proxy {
        self.proxy.clone()
    }

    pub(super) async fn open(name: String, server: McpServer, proxy: Proxy) -> Session {
        let config = McpConfig::default().with_server(name, server);
        Arc::new(McpHub::connect_through(&config, &proxy).await)
    }

    pub(super) fn insert(&mut self, server: Uuid, session: Session) -> Option<Session> {
        self.hubs.insert(server, session)
    }

    pub(super) fn remove(&mut self, server: Uuid) -> Option<Session> {
        self.hubs.remove(&server)
    }

    pub(super) fn connected(&self, server: Uuid) -> bool {
        self.hubs
            .get(&server)
            .is_some_and(|hub| hub.server_count() == 1)
    }

    pub(super) fn tools(&self, server: Uuid, name: &str) -> Vec<String> {
        self.hubs
            .get(&server)
            .map(|hub| advertised(name, hub))
            .unwrap_or_default()
    }

    pub(super) fn tools_avoiding(
        &self,
        server: Uuid,
        used: &mut HashSet<String>,
    ) -> Vec<Arc<dyn Tool>> {
        self.hubs
            .get(&server)
            .map(|hub| hub.tools_avoiding(used))
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn get(&self, server: Uuid) -> Option<Session> {
        self.hubs.get(&server).cloned()
    }
}

/// The tools `hub` advertises for the server `name`, without the prefix
/// that qualifies them.
pub(super) fn advertised(name: &str, hub: &McpHub) -> Vec<String> {
    let prefix = format!("{name}{SEPARATOR}");
    hub.tools()
        .iter()
        .filter_map(|tool| tool.name().strip_prefix(&prefix).map(str::to_string))
        .collect()
}
