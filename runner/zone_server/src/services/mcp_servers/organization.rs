use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use abnegate_agent::ToolRegistry;
use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::qualified_tool_name;
use abnegate_agent::tool::Proxy;
use tokio::sync::watch;
use uuid::Uuid;

use super::definition::Definition;
use super::definition::Revisions;
use super::sessions::Session;
use super::sessions::Sessions;
use super::status::Connection;
use super::status::Policy;
use super::status::UNCONNECTED;
use super::status::static_status;

struct Slot {
    definition: Definition,
    connecting: bool,
    generation: u64,
}

struct Inner {
    slots: BTreeMap<Uuid, Slot>,
    sessions: Sessions,
    generation: u64,
}

impl Inner {
    fn next(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    fn named(&self, name: &str) -> Option<(Uuid, &Slot)> {
        self.slots
            .iter()
            .find(|(_, slot)| slot.definition.name == name)
            .map(|(server, slot)| (*server, slot))
    }

    fn connecting(&self) -> bool {
        self.slots.values().any(|slot| slot.connecting)
    }
}

struct Opening {
    server: Uuid,
    generation: u64,
    name: String,
    target: McpServer,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Registered {
    pub servers: usize,
    pub tools: usize,
}

pub struct OrganizationHub {
    inner: RwLock<Inner>,
    policy: Policy,
    connecting: watch::Sender<bool>,
}

impl OrganizationHub {
    pub(super) fn new(proxy: Proxy, policy: Policy) -> Self {
        Self {
            inner: RwLock::new(Inner {
                slots: BTreeMap::new(),
                sessions: Sessions::new(proxy),
                generation: 0,
            }),
            policy,
            connecting: watch::Sender::new(false),
        }
    }

    pub fn connection(&self, name: &str) -> Connection {
        let inner = self.read();
        let Some((server, slot)) = inner.named(name) else {
            return Connection::failed(UNCONNECTED);
        };
        if let Some(connection) = static_status(&slot.definition, self.policy) {
            return connection;
        }
        if slot.connecting {
            return Connection::connecting();
        }
        if inner.sessions.connected(server) {
            return Connection::connected(inner.sessions.tools(server, name));
        }
        Connection::failed(UNCONNECTED)
    }

    pub fn register(
        &self,
        registry: &mut ToolRegistry,
        include: &dyn Fn(Uuid) -> bool,
    ) -> Registered {
        let mut used: HashSet<String> = registry.names().into_iter().map(str::to_string).collect();
        let inner = self.read();
        let mut slots: Vec<(Uuid, &Slot)> = inner
            .slots
            .iter()
            .map(|(server, slot)| (*server, slot))
            .filter(|(server, _)| include(*server))
            .collect();
        slots.sort_by(|(_, left), (_, right)| left.definition.name.cmp(&right.definition.name));
        let mut registered = Registered::default();
        for (server, slot) in slots {
            let mut tools = 0;
            for tool in inner.sessions.tools_avoiding(server, &mut used) {
                if allowed(&slot.definition, tool.name()) {
                    registry.register(tool);
                    tools += 1;
                }
            }
            registered.tools += tools;
            registered.servers += usize::from(tools > 0);
        }
        registered
    }

    pub fn connected(&self) -> usize {
        let inner = self.read();
        inner
            .slots
            .keys()
            .filter(|server| inner.sessions.connected(**server))
            .count()
    }

    pub fn connecting(&self) -> bool {
        *self.connecting.borrow()
    }

    pub(super) async fn settled(&self) {
        let mut connecting = self.connecting.subscribe();
        let _ = connecting.wait_for(|connecting| !connecting).await;
    }

    pub(super) fn revisions(&self) -> Revisions {
        self.read()
            .slots
            .iter()
            .map(|(server, slot)| (*server, slot.definition.revision))
            .collect()
    }

    pub(super) fn apply(self: &Arc<Self>, definitions: Vec<Definition>, forced: Option<Uuid>) {
        let mut stale = Vec::new();
        let mut openings = Vec::new();
        let proxy = {
            let mut guard = self.write();
            let inner = &mut *guard;
            let stored: BTreeSet<Uuid> =
                definitions.iter().map(|definition| definition.id).collect();
            let removed: Vec<Uuid> = inner
                .slots
                .keys()
                .filter(|server| !stored.contains(server))
                .copied()
                .collect();
            for server in removed {
                inner.slots.remove(&server);
                stale.extend(inner.sessions.remove(server));
            }
            for definition in definitions {
                let server = definition.id;
                if forced != Some(server)
                    && let Some(slot) = inner.slots.get_mut(&server)
                {
                    if slot.definition.revision == definition.revision {
                        continue;
                    }
                    if !reconnects(&slot.definition, &definition) {
                        slot.definition = definition;
                        continue;
                    }
                }
                let generation = inner.next();
                let connecting = static_status(&definition, self.policy).is_none();
                stale.extend(inner.sessions.remove(server));
                if connecting {
                    openings.push(Opening {
                        server,
                        generation,
                        name: definition.name.clone(),
                        target: definition.server.clone(),
                    });
                }
                inner.slots.insert(
                    server,
                    Slot {
                        definition,
                        connecting,
                        generation,
                    },
                );
            }
            self.connecting.send_replace(inner.connecting());
            inner.sessions.proxy()
        };
        drop(stale);
        for opening in openings {
            self.open(opening, proxy.clone());
        }
    }

    fn open(self: &Arc<Self>, opening: Opening, proxy: Proxy) {
        let hub = Arc::downgrade(self);
        tokio::spawn(async move {
            // A connect that panics still has to settle its slot, or `settled` never returns.
            let session = tokio::spawn(Sessions::open(opening.name, opening.target, proxy))
                .await
                .ok();
            if let Some(hub) = hub.upgrade() {
                hub.settle(opening.server, opening.generation, session);
            }
        });
    }

    fn settle(&self, server: Uuid, generation: u64, session: Option<Session>) {
        let stale = {
            let mut guard = self.write();
            let inner = &mut *guard;
            let stale = match inner.slots.get_mut(&server) {
                Some(slot) if slot.generation == generation => {
                    slot.connecting = false;
                    session.and_then(|session| inner.sessions.insert(server, session))
                }
                _ => session,
            };
            self.connecting.send_replace(inner.connecting());
            stale
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

fn reconnects(stored: &Definition, definition: &Definition) -> bool {
    stored.name != definition.name
        || stored.server != definition.server
        || stored.enabled != definition.enabled
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
    pub(crate) fn definition(&self, name: &str) -> Option<Definition> {
        self.read()
            .named(name)
            .map(|(_, slot)| slot.definition.clone())
    }

    pub(crate) fn session(&self, name: &str) -> Option<Session> {
        let inner = self.read();
        let (server, _) = inner.named(name)?;
        inner.sessions.get(server)
    }

    pub(crate) fn generation(&self, name: &str) -> Option<u64> {
        self.read().named(name).map(|(_, slot)| slot.generation)
    }

    pub(crate) fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .read()
            .slots
            .values()
            .map(|slot| slot.definition.name.clone())
            .collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent::mcp::McpTransport;

    use super::*;
    use crate::services::mcp_servers::status::Status;

    const POLICY: Policy = Policy {
        enabled: true,
        commands: true,
    };

    async fn settled(definitions: Vec<Definition>) -> Arc<OrganizationHub> {
        let hub = Arc::new(OrganizationHub::new(Proxy::default(), POLICY));
        hub.apply(definitions, None);
        hub.settled().await;
        hub
    }

    async fn connected(definitions: Vec<Definition>) -> Arc<OrganizationHub> {
        let hub = settled(definitions).await;
        assert_eq!(hub.connected(), 1, "the stub server has to connect");
        hub
    }

    fn allowing(tools: Option<&[&str]>) -> Definition {
        Definition {
            tools: tools.map(|tools| tools.iter().map(|tool| tool.to_string()).collect()),
            ..Definition::stub("stub")
        }
    }

    fn one() -> Registered {
        Registered {
            servers: 1,
            tools: 1,
        }
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

            let registered = hub.register(&mut registry, &|_| true);

            assert_eq!(
                registered,
                if expected {
                    one()
                } else {
                    Registered::default()
                },
                "{tools:?}"
            );
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
        assert_eq!(other.register(&mut registry, &|_| true), one());
        assert!(registry.get("stub__delegate").is_some());
        let hub = connected(vec![allowing(Some(&["delegate"]))]).await;

        let registered = hub.register(&mut registry, &|_| true);

        assert_eq!(registered, Registered::default(), "{:?}", registry.names());
        assert!(registry.get("stub__delegate_2").is_none());
    }

    #[tokio::test]
    async fn registration_asks_about_each_server_by_its_row() {
        let kept = Definition::stub("kept");
        let excluded = Definition::stub("excluded");
        let hub = settled(vec![kept.clone(), excluded.clone()]).await;
        assert_eq!(hub.connected(), 2, "both stub servers have to connect");
        let mut registry = ToolRegistry::new();

        let registered = hub.register(&mut registry, &|server| server != excluded.id);

        assert_eq!(registered, one());
        assert_eq!(registry.names(), ["kept__delegate"]);
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

        let hub = settled(vec![missing]).await;

        assert_eq!(hub.connected(), 0);
        assert!(!hub.connecting());
        assert_eq!(hub.connection("missing"), Connection::failed(UNCONNECTED));
    }

    #[tokio::test]
    async fn a_hub_with_nothing_to_connect_is_settled() {
        let hub = settled(Vec::new()).await;

        assert!(!hub.connecting());
        assert!(hub.revisions().is_empty());
    }
}
