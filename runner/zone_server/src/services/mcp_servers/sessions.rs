use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use abnegate_agent::Tool;
use abnegate_agent::mcp::McpConfig;
use abnegate_agent::mcp::McpHub;
use abnegate_agent::mcp::McpOutcome;
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

    pub(super) fn failure(&self, server: Uuid, name: &str) -> Option<String> {
        match self.hubs.get(&server)?.outcome(name)? {
            McpOutcome::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    /// The tools of `server` that `allowed` keeps, each named clear of `used`.
    ///
    /// `allowed` is asked about a tool by the name it has while nothing else is registered, so
    /// a name another server already took cannot hide a tool from its own allowlist. The hub
    /// names a tool only as it hands every tool over, so the kept ones are picked by position
    /// from a second listing that avoids a copy of `used`: a tool that is not kept reserves
    /// nothing.
    pub(super) fn tools_avoiding(
        &self,
        server: Uuid,
        used: &mut HashSet<String>,
        allowed: impl Fn(&str) -> bool,
    ) -> Vec<Arc<dyn Tool>> {
        let Some(hub) = self.hubs.get(&server) else {
            return Vec::new();
        };
        let mut reserved = used.clone();
        let tools: Vec<Arc<dyn Tool>> = hub
            .tools()
            .into_iter()
            .zip(hub.tools_avoiding(&mut reserved))
            .filter(|(own, _)| allowed(own.name()))
            .map(|(_, tool)| tool)
            .collect();
        used.extend(tools.iter().map(|tool| tool.name().to_string()));
        tools
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::mcp_servers::definition::Definition;

    const TOOL: &str = "stub__delegate";
    const COLLIDED: &str = "stub__delegate_2";

    async fn connected() -> (Sessions, Uuid) {
        let definition = Definition::stub("stub");
        let mut sessions = Sessions::new(Proxy::default());
        let session = Sessions::open(definition.name, definition.server, Proxy::default()).await;
        assert_eq!(session.server_count(), 1, "the stub server has to connect");
        sessions.insert(definition.id, session);
        (sessions, definition.id)
    }

    fn taken() -> HashSet<String> {
        HashSet::from([TOOL.to_string()])
    }

    fn names(tools: &[Arc<dyn Tool>]) -> Vec<&str> {
        tools.iter().map(|tool| tool.name()).collect()
    }

    #[tokio::test]
    async fn a_kept_tool_is_asked_about_by_its_own_name_and_named_clear_of_the_rest() {
        let (sessions, server) = connected().await;
        let mut used = taken();

        let kept = sessions.tools_avoiding(server, &mut used, |own| own == TOOL);

        assert_eq!(names(&kept), [COLLIDED]);
        assert_eq!(
            used,
            HashSet::from([TOOL.to_string(), COLLIDED.to_string()])
        );
    }

    #[tokio::test]
    async fn a_dropped_tool_reserves_no_name() {
        let (sessions, server) = connected().await;
        let mut used = taken();

        let kept = sessions.tools_avoiding(server, &mut used, |_| false);

        assert!(kept.is_empty(), "{:?}", names(&kept));
        assert_eq!(used, taken());
    }

    #[tokio::test]
    async fn a_server_with_no_session_offers_nothing() {
        let sessions = Sessions::new(Proxy::default());
        let mut used = taken();

        let kept = sessions.tools_avoiding(Uuid::new_v4(), &mut used, |_| true);

        assert!(kept.is_empty(), "{:?}", names(&kept));
        assert_eq!(used, taken());
    }
}
