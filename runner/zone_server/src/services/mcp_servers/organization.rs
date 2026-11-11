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
use super::definition::Snapshot;
use super::sessions::Session;
use super::sessions::Sessions;
use super::status::Connection;
use super::status::Policy;
use super::status::UNCONNECTED;
use super::status::static_status;
use super::status::summary;

struct Slot {
    definition: Definition,
    connecting: bool,
    generation: u64,
}

struct Inner {
    slots: BTreeMap<Uuid, Slot>,
    unread: Revisions,
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
                unread: Revisions::new(),
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
        if slot.definition.server.url.is_some()
            && let Some(error) = inner.sessions.failure(server, name)
        {
            return Connection::failed(summary(name, &slot.definition.server, &error));
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
            let tools = inner
                .sessions
                .tools_avoiding(server, &mut used, |tool| allowed(&slot.definition, tool));
            registered.tools += tools.len();
            registered.servers += usize::from(!tools.is_empty());
            for tool in tools {
                registry.register(tool);
            }
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
        let inner = self.read();
        let mut revisions = inner.unread.clone();
        revisions.extend(
            inner
                .slots
                .iter()
                .map(|(server, slot)| (*server, slot.definition.revision)),
        );
        revisions
    }

    pub(super) fn apply(self: &Arc<Self>, snapshot: Snapshot, forced: Option<Uuid>) {
        let Snapshot {
            revisions: mut unread,
            definitions,
        } = snapshot;
        let mut stale = Vec::new();
        let mut openings = Vec::new();
        let proxy = {
            let mut guard = self.write();
            let inner = &mut *guard;
            let stored: BTreeSet<Uuid> =
                definitions.iter().map(|definition| definition.id).collect();
            unread.retain(|server, _| !stored.contains(server));
            inner.unread = unread;
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

    pub(crate) fn failure(&self, name: &str) -> Option<String> {
        let inner = self.read();
        let (server, _) = inner.named(name)?;
        inner.sessions.failure(server, name)
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
    use std::time::Duration;

    use abnegate_agent::mcp::McpTransport;
    use abnegate_secret::REDACTED;
    use serde_json::Value;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::Request;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::any;
    use zone_core::servers::mask;

    use super::*;
    use crate::services::mcp_servers::status::ANSWERED;
    use crate::services::mcp_servers::status::FORBIDDEN;
    use crate::services::mcp_servers::status::INCOMPLETE;
    use crate::services::mcp_servers::status::METADATA_ADDRESS;
    use crate::services::mcp_servers::status::NOT_MCP;
    use crate::services::mcp_servers::status::REDIRECTED;
    use crate::services::mcp_servers::status::REJECTED;
    use crate::services::mcp_servers::status::SSE_UNSUPPORTED;
    use crate::services::mcp_servers::status::Status;
    use crate::services::mcp_servers::status::UNAUTHORIZED;
    use crate::services::mcp_servers::status::UNREACHABLE;
    use crate::services::mcp_servers::stub::Stub;

    const POLICY: Policy = Policy {
        enabled: true,
        commands: true,
    };
    const PROMPT: Duration = Duration::from_secs(5);
    const REMOTE: &str = "remote";
    const AUTHORIZATION: &str = "Authorization";
    const TOKEN: &str = "stub-token-51c2d9";
    const KEY_HEADER: &str = "X-Key";
    const KEY: &str = "key-9f3a7b-sent";
    const SPOKEN: &str = "remote-said-7f3e9a";
    const CHALLENGE_HEADER: &str = "WWW-Authenticate";
    const CHALLENGE: &str = r#"Bearer error="insufficient_scope", scope="remote-said-7f3e9a""#;
    const LOCATION: &str = "Location";
    const OVERSIZED: usize = 4096;
    const INITIALIZE: &str = "initialize";
    const LIST: &str = "tools/list";

    fn remote(url: String) -> Definition {
        Definition::of(REMOTE, McpServer::remote(url))
    }

    async fn answering(
        respond: impl Fn(&Request) -> ResponseTemplate + Send + Sync + 'static,
    ) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(respond)
            .mount(&server)
            .await;
        server
    }

    async fn listing(answer: fn(&Request) -> ResponseTemplate) -> MockServer {
        answering(move |request| match method(request).as_deref() {
            Some(INITIALIZE) => ResponseTemplate::new(200).set_body_json(initialized(request)),
            Some(LIST) => answer(request),
            _ => ResponseTemplate::new(202),
        })
        .await
    }

    fn spoken(status: u16) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_string(SPOKEN)
    }

    fn typed(body: String, kind: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body, kind)
    }

    fn message(request: &Request) -> Value {
        request.body_json().unwrap_or(Value::Null)
    }

    fn method(request: &Request) -> Option<String> {
        message(request)["method"].as_str().map(str::to_string)
    }

    fn refusal(request: &Request) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": message(request)["id"],
            "error": { "code": -32000, "message": SPOKEN, "data": SPOKEN },
        })
    }

    fn initialized(request: &Request) -> Value {
        let message = message(request);
        json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "protocolVersion": message["params"]["protocolVersion"],
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "stub", "version": "0.0.1" },
            },
        })
    }

    async fn settled(definitions: Vec<Definition>) -> Arc<OrganizationHub> {
        let hub = Arc::new(OrganizationHub::new(Proxy::default(), POLICY));
        hub.apply(definitions.into(), None);
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

    async fn taken() -> ToolRegistry {
        let other = connected(vec![Definition::stub("stub")]).await;
        let mut registry = ToolRegistry::new();
        assert_eq!(other.register(&mut registry, &|_| true), one());
        assert_eq!(registry.names(), ["stub__delegate"]);
        registry
    }

    #[tokio::test]
    async fn an_allowlisted_tool_whose_name_is_taken_is_offered_under_a_suffixed_one() {
        for tools in [None, Some(&["delegate"][..])] {
            let mut registry = taken().await;
            let hub = connected(vec![allowing(tools)]).await;

            let registered = hub.register(&mut registry, &|_| true);

            assert_eq!(registered, one(), "{tools:?}");
            let mut names = registry.names();
            names.sort_unstable();
            assert_eq!(names, ["stub__delegate", "stub__delegate_2"], "{tools:?}");
        }
    }

    #[tokio::test]
    async fn an_allowlist_names_a_tool_as_its_server_does_whatever_name_it_is_offered_under() {
        for tools in [&["other"][..], &["delegate_2"][..]] {
            let mut registry = taken().await;
            let hub = connected(vec![allowing(Some(tools))]).await;

            let registered = hub.register(&mut registry, &|_| true);

            assert_eq!(registered, Registered::default(), "{tools:?}");
            assert_eq!(registry.names(), ["stub__delegate"], "{tools:?}");
        }
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
        assert_eq!(hub.connection("remote"), Connection::failed(UNREACHABLE));
        assert_eq!(
            hub.session("remote").map(|session| session.server_count()),
            Some(0),
            "the hub of a connect that failed is kept, for the reason it holds"
        );
        assert_eq!(
            hub.connection("events"),
            Connection::failed(SSE_UNSUPPORTED)
        );
        assert!(hub.session("events").is_none());
    }

    #[tokio::test]
    async fn an_http_server_connects_and_lists_its_tools() {
        let stub = Stub::start(&[]).await;
        let hub = connected(vec![remote(stub.url())]).await;
        let mut registry = ToolRegistry::new();

        let registered = hub.register(&mut registry, &|_| true);

        assert_eq!(
            hub.connection(REMOTE),
            Connection::connected(vec!["delegate".to_string()])
        );
        assert_eq!(registered, one());
        assert_eq!(registry.names(), ["remote__delegate"]);
    }

    #[tokio::test]
    async fn a_refused_http_server_fails_with_its_kind_and_status() {
        let stub = Stub::start(&["--token", TOKEN]).await;
        let admitted = Definition::of(
            "admitted",
            McpServer::remote(stub.url()).with_header(AUTHORIZATION, format!("Bearer {TOKEN}")),
        );
        let refused = Definition::of(
            "refused",
            McpServer::remote(stub.url()).with_header(AUTHORIZATION, "Bearer another-token"),
        );

        let hub = connected(vec![admitted, refused]).await;

        assert_eq!(
            hub.connection("admitted"),
            Connection::connected(vec!["delegate".to_string()])
        );
        assert_eq!(hub.connection("refused"), Connection::failed(UNAUTHORIZED));
    }

    #[tokio::test]
    async fn each_failure_reaches_a_tenant_as_its_kind() {
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
            format!("http://{}/mcp", listener.local_addr().expect("its address"))
        };
        let servers = [
            (
                "unchallenged",
                answering(|_| spoken(401)).await,
                format!("{ANSWERED} 401"),
            ),
            (
                "challenged",
                answering(|_| spoken(401).insert_header(CHALLENGE_HEADER, "Bearer")).await,
                UNAUTHORIZED.to_string(),
            ),
            (
                "forbidden",
                answering(|_| spoken(403).insert_header(CHALLENGE_HEADER, CHALLENGE)).await,
                FORBIDDEN.to_string(),
            ),
            (
                "echoing",
                answering(|request| {
                    let sent = request
                        .headers
                        .get(KEY_HEADER)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default();
                    ResponseTemplate::new(400)
                        .set_body_string(format!("{SPOKEN}: unknown key {sent}"))
                })
                .await,
                format!("{ANSWERED} 400"),
            ),
            (
                "rejecting",
                answering(|request| ResponseTemplate::new(400).set_body_json(refusal(request)))
                    .await,
                REJECTED.to_string(),
            ),
            (
                "broken",
                answering(|_| spoken(500)).await,
                format!("{ANSWERED} 500"),
            ),
            (
                "limiting",
                answering(|_| spoken(429)).await,
                format!("{ANSWERED} 429"),
            ),
            (
                "oversized",
                answering(|_| ResponseTemplate::new(400).set_body_string(SPOKEN.repeat(OVERSIZED)))
                    .await,
                INCOMPLETE.to_string(),
            ),
            (
                "page",
                answering(|_| typed(format!("<p>{SPOKEN}</p>"), "text/html")).await,
                NOT_MCP.to_string(),
            ),
            (
                "garbled",
                answering(|_| typed(format!("{{{SPOKEN}"), "application/json")).await,
                NOT_MCP.to_string(),
            ),
            (
                "uninitialized",
                answering(|request| ResponseTemplate::new(200).set_body_json(refusal(request)))
                    .await,
                REJECTED.to_string(),
            ),
            (
                "uncorrelated",
                answering(|_| {
                    ResponseTemplate::new(200).set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": SPOKEN,
                        "error": { "code": -32000, "message": SPOKEN },
                    }))
                })
                .await,
                NOT_MCP.to_string(),
            ),
            (
                "elsewhere",
                answering(|_| {
                    ResponseTemplate::new(307)
                        .insert_header(LOCATION, format!("http://127.0.0.1:1/{SPOKEN}"))
                })
                .await,
                REDIRECTED.to_string(),
            ),
            (
                "moved",
                answering(|_| {
                    ResponseTemplate::new(302).insert_header(LOCATION, format!("/{SPOKEN}"))
                })
                .await,
                REDIRECTED.to_string(),
            ),
            (
                "looping",
                answering(|request| {
                    ResponseTemplate::new(307).insert_header(LOCATION, request.url.path())
                })
                .await,
                REDIRECTED.to_string(),
            ),
            (
                "unlisted",
                listing(|request| ResponseTemplate::new(200).set_body_json(refusal(request))).await,
                REJECTED.to_string(),
            ),
            (
                "unlistable",
                listing(|_| spoken(500)).await,
                format!("{ANSWERED} 500"),
            ),
            (
                "mislisted",
                listing(|request| ResponseTemplate::new(200).set_body_json(initialized(request)))
                    .await,
                NOT_MCP.to_string(),
            ),
        ];
        let cases: Vec<(&str, String, String)> = servers
            .iter()
            .map(|(name, server, expected)| (*name, server.uri(), expected.clone()))
            .chain([("closed", closed, UNREACHABLE.to_string())])
            .collect();
        let definitions = cases
            .iter()
            .map(|(name, url, _)| {
                let mut server = McpServer::remote(url.clone()).with_header(KEY_HEADER, KEY);
                mask(&mut server);
                Definition::of(name, server)
            })
            .collect();

        let hub = settled(definitions).await;

        for (name, _, expected) in cases {
            let text = hub.failure(name).expect("the connect failed");
            assert_eq!(hub.connection(name), Connection::failed(expected), "{name}");
            assert!(!text.contains(KEY), "{name} kept the header it sent");
        }
        assert!(
            hub.failure("echoing")
                .is_some_and(|text| text.contains(REDACTED)),
            "the header the server echoed was never in its answer"
        );
        assert_eq!(hub.connected(), 0);
    }

    #[tokio::test]
    async fn a_metadata_address_is_refused_before_any_request() {
        let metadata = Definition::of(REMOTE, McpServer::remote("http://169.254.169.254/mcp"));

        let hub = tokio::time::timeout(PROMPT, settled(vec![metadata]))
            .await
            .expect("no request is made, so nothing is waited for");

        assert_eq!(hub.connection(REMOTE), Connection::failed(METADATA_ADDRESS));
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
