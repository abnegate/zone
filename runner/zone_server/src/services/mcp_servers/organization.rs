use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use abnegate_agent::mcp::{McpConfig, McpHub, McpServer, SEPARATOR, qualified_tool_name};
use abnegate_agent::tool::Proxy;
use abnegate_agent::{Tool, ToolRegistry};
use futures::future::join_all;
use uuid::Uuid;

use super::definition::Definition;
use super::status::{Connection, UNCONNECTED, static_status};

type Session = Arc<McpHub>;

struct Sessions {
    proxy: Proxy,
    hubs: BTreeMap<String, Session>,
}

impl Sessions {
    fn new(proxy: Proxy) -> Self {
        Self {
            proxy,
            hubs: BTreeMap::new(),
        }
    }

    fn proxy(&self) -> Proxy {
        self.proxy.clone()
    }

    async fn open(name: String, server: McpServer, proxy: Proxy) -> (String, Session) {
        let config = McpConfig::default().with_server(name.clone(), server);
        let hub = McpHub::connect_through(&config, &proxy).await;
        (name, Arc::new(hub))
    }

    fn insert(&mut self, name: &str, session: Session) -> Option<Session> {
        self.hubs.insert(name.to_string(), session)
    }

    fn remove(&mut self, name: &str) -> Option<Session> {
        self.hubs.remove(name)
    }

    fn connected(&self, name: &str) -> bool {
        self.hubs
            .get(name)
            .is_some_and(|hub| hub.server_count() == 1)
    }

    fn tools(&self, name: &str) -> Vec<String> {
        self.hubs
            .get(name)
            .map(|hub| advertised(name, hub))
            .unwrap_or_default()
    }

    fn tools_avoiding(&self, name: &str, used: &mut HashSet<String>) -> Vec<Arc<dyn Tool>> {
        self.hubs
            .get(name)
            .map(|hub| hub.tools_avoiding(used))
            .unwrap_or_default()
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

struct Slot {
    definition: Definition,
    connecting: bool,
    generation: u64,
}

struct Inner {
    slots: BTreeMap<String, Slot>,
    sessions: Sessions,
    generation: u64,
}

impl Inner {
    fn next(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }
}

pub struct OrganizationHub {
    inner: RwLock<Inner>,
    commands: bool,
}

impl OrganizationHub {
    pub(super) async fn connect(
        definitions: Vec<Definition>,
        proxy: Proxy,
        commands: bool,
    ) -> Self {
        let mut inner = Inner {
            slots: BTreeMap::new(),
            sessions: Sessions::new(proxy),
            generation: 0,
        };
        for definition in definitions {
            let generation = inner.next();
            inner.slots.insert(
                definition.name.clone(),
                Slot {
                    definition,
                    connecting: false,
                    generation,
                },
            );
        }
        let opened = join_all(
            inner
                .slots
                .values()
                .filter(|slot| static_status(&slot.definition, commands).is_none())
                .map(|slot| {
                    Sessions::open(
                        slot.definition.name.clone(),
                        slot.definition.server.clone(),
                        inner.sessions.proxy(),
                    )
                })
                .collect::<Vec<_>>(),
        )
        .await;
        for (name, session) in opened {
            inner.sessions.insert(&name, session);
        }
        Self {
            inner: RwLock::new(inner),
            commands,
        }
    }

    pub fn connection(&self, name: &str) -> Connection {
        let inner = self.read();
        let Some(slot) = inner.slots.get(name) else {
            return Connection::failed(UNCONNECTED);
        };
        if let Some(connection) = static_status(&slot.definition, self.commands) {
            return connection;
        }
        if slot.connecting {
            return Connection::connecting();
        }
        if inner.sessions.connected(name) {
            return Connection::connected(inner.sessions.tools(name));
        }
        Connection::failed(UNCONNECTED)
    }

    pub fn register(&self, registry: &mut ToolRegistry, include: &dyn Fn(&str) -> bool) -> usize {
        let mut used: HashSet<String> = registry.names().into_iter().map(str::to_string).collect();
        let inner = self.read();
        let mut added = 0;
        for (name, slot) in &inner.slots {
            if !include(name) {
                continue;
            }
            for tool in inner.sessions.tools_avoiding(name, &mut used) {
                if allowed(&slot.definition, tool.name()) {
                    registry.register(tool);
                    added += 1;
                }
            }
        }
        added
    }

    pub fn connected(&self) -> usize {
        let inner = self.read();
        inner
            .slots
            .keys()
            .filter(|name| inner.sessions.connected(name))
            .count()
    }

    pub fn identifiers(&self) -> BTreeMap<String, Uuid> {
        self.read()
            .slots
            .iter()
            .filter_map(|(name, slot)| Some((name.clone(), slot.definition.id?)))
            .collect()
    }

    pub(super) fn refresh(self: &Arc<Self>, definition: Definition) {
        let name = definition.name.clone();
        let connectable = static_status(&definition, self.commands).is_none();
        let (stale, pending) = {
            let mut guard = self.write();
            let inner = &mut *guard;
            if let Some(slot) = inner.slots.get_mut(&name)
                && slot.definition.server == definition.server
                && slot.definition.enabled == definition.enabled
            {
                slot.definition = definition;
                return;
            }
            let generation = inner.next();
            let server = definition.server.clone();
            inner.slots.insert(
                name.clone(),
                Slot {
                    definition,
                    connecting: connectable,
                    generation,
                },
            );
            if connectable {
                (None, Some((server, generation, inner.sessions.proxy())))
            } else {
                (inner.sessions.remove(&name), None)
            }
        };
        drop(stale);
        let Some((server, generation, proxy)) = pending else {
            return;
        };
        let hub = Arc::downgrade(self);
        tokio::spawn(async move {
            let (name, session) = Sessions::open(name, server, proxy).await;
            if let Some(hub) = hub.upgrade() {
                hub.settle(&name, generation, session);
            }
        });
    }

    pub(super) fn remove(&self, name: &str) {
        let stale = {
            let mut inner = self.write();
            inner.slots.remove(name);
            inner.sessions.remove(name)
        };
        drop(stale);
    }

    fn settle(&self, name: &str, generation: u64, session: Session) {
        let stale = {
            let mut guard = self.write();
            let inner = &mut *guard;
            match inner.slots.get_mut(name) {
                Some(slot) if slot.generation == generation => {
                    slot.connecting = false;
                    inner.sessions.insert(name, session)
                }
                _ => Some(session),
            }
        };
        drop(stale);
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(PoisonError::into_inner)
    }
}

fn allowed(definition: &Definition, tool: &str) -> bool {
    definition.tools.as_ref().is_none_or(|entries| {
        entries
            .iter()
            .any(|entry| qualified_tool_name(&definition.name, entry) == tool)
    })
}

#[cfg(test)]
impl OrganizationHub {
    pub(super) fn definition(&self, name: &str) -> Option<Definition> {
        self.read()
            .slots
            .get(name)
            .map(|slot| slot.definition.clone())
    }

    pub(super) fn session(&self, name: &str) -> Option<Session> {
        self.read().sessions.hubs.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent::mcp::McpTransport;

    use super::*;
    use crate::services::mcp_servers::status::Status;

    async fn connected(definitions: Vec<Definition>) -> OrganizationHub {
        let hub = OrganizationHub::connect(definitions, Proxy::default(), true).await;
        assert_eq!(hub.connected(), 1, "the stub server has to connect");
        hub
    }

    fn allowing(tools: Option<&[&str]>) -> Definition {
        Definition {
            tools: tools.map(|tools| tools.iter().map(|tool| tool.to_string()).collect()),
            ..Definition::stub("stub")
        }
    }

    #[tokio::test]
    async fn a_stored_server_is_identified_by_its_name() {
        let stored = Definition {
            enabled: false,
            ..Definition::stub("stored")
        };
        let unstored = Definition {
            id: None,
            enabled: false,
            ..Definition::stub("unstored")
        };
        let identifier = stored.id.expect("a stored definition has an identifier");
        let hub = OrganizationHub::connect(vec![stored, unstored], Proxy::default(), true).await;

        assert_eq!(
            hub.identifiers(),
            BTreeMap::from([("stored".to_string(), identifier)])
        );
    }

    #[tokio::test]
    async fn a_connected_server_lists_its_tools_unqualified() {
        let hub = connected(vec![Definition::stub("stub")]).await;

        assert_eq!(
            hub.connection("stub"),
            Connection::connected(vec!["delegate".to_string()])
        );
    }

    #[tokio::test]
    async fn the_allowlist_filters_registration() {
        for (tools, expected) in [
            (None, true),
            (Some(&["delegate"][..]), true),
            (Some(&["other"][..]), false),
        ] {
            let hub = connected(vec![allowing(tools)]).await;
            let mut registry = ToolRegistry::new();

            let added = hub.register(&mut registry, &|_| true);

            assert_eq!(added, usize::from(expected), "{tools:?}");
            assert_eq!(
                registry.get("stub__delegate").is_some(),
                expected,
                "{tools:?}: {:?}",
                registry.names()
            );
            assert_eq!(
                hub.connection("stub").tools,
                ["delegate"],
                "the allowlist hides nothing from the status"
            );
        }
    }

    #[tokio::test]
    async fn a_collided_tool_does_not_match_its_allowlist_entry() {
        let other = connected(vec![Definition::stub("stub")]).await;
        let mut registry = ToolRegistry::new();
        assert_eq!(other.register(&mut registry, &|_| true), 1);
        assert!(registry.get("stub__delegate").is_some());
        let hub = connected(vec![allowing(Some(&["delegate"]))]).await;

        let added = hub.register(&mut registry, &|_| true);

        assert_eq!(added, 0, "{:?}", registry.names());
        assert!(registry.get("stub__delegate_2").is_none());
    }

    #[tokio::test]
    async fn an_excluded_server_registers_nothing() {
        let hub = connected(vec![Definition::stub("stub")]).await;
        let mut registry = ToolRegistry::new();

        let added = hub.register(&mut registry, &|name| name != "stub");

        assert_eq!(added, 0);
        assert!(registry.names().is_empty(), "{:?}", registry.names());
    }

    #[tokio::test]
    async fn only_connected_servers_count() {
        let disabled = Definition {
            enabled: false,
            ..Definition::stub("off")
        };
        let http = Definition::of("remote", McpServer::remote("http://127.0.0.1:1/mcp"));
        let sse = Definition::of(
            "events",
            McpServer::remote("http://127.0.0.1:1/sse").with_transport(McpTransport::Sse),
        );

        let hub = connected(vec![Definition::stub("stub"), disabled, http, sse]).await;

        assert_eq!(hub.connection("off").status, Status::Disabled);
        assert!(hub.session("off").is_none());
        assert!(hub.session("remote").is_none());
        assert!(hub.session("events").is_none());
    }

    #[tokio::test]
    async fn a_server_that_fails_to_start_is_unconnected() {
        let missing = Definition::of(
            "missing",
            McpServer::command("/nonexistent/zone-mcp-stub", Vec::<String>::new()),
        );

        let hub = OrganizationHub::connect(vec![missing], Proxy::default(), true).await;

        assert_eq!(hub.connected(), 0);
        assert_eq!(hub.connection("missing"), Connection::failed(UNCONNECTED));
    }
}
