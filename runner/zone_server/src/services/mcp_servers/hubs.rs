use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use abnegate_agent::mcp::McpHub;
use abnegate_agent::tool::Proxy;
use dashmap::DashMap;
use tokio::sync::OnceCell;
use uuid::Uuid;
use zone_core::servers;

use super::definition::Definition;
use super::organization::{OrganizationHub, advertised};
use super::status::{Connection, UNCONNECTED};

pub struct Hubs {
    proxy: Proxy,
    commands: bool,
    instance: OnceCell<Arc<McpHub>>,
    organizations: DashMap<Uuid, Arc<Entry>>,
}

#[derive(Default)]
struct Entry {
    cell: OnceCell<Arc<OrganizationHub>>,
    pending: Mutex<Pending>,
}

#[derive(Default)]
struct Pending {
    changes: Vec<Change>,
    hub: Option<Arc<OrganizationHub>>,
}

enum Change {
    Refresh(Box<Definition>),
    Remove(String),
}

impl Change {
    fn apply(self, hub: &Arc<OrganizationHub>) {
        match self {
            Self::Refresh(definition) => hub.refresh(*definition),
            Self::Remove(name) => hub.remove(&name),
        }
    }
}

impl Entry {
    fn submit(&self, change: Change) {
        let hub = {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(hub) = pending.hub.clone() else {
                pending.changes.push(change);
                return;
            };
            hub
        };
        change.apply(&hub);
    }

    fn drain(&self, hub: &Arc<OrganizationHub>) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        for change in std::mem::take(&mut pending.changes) {
            change.apply(hub);
        }
        pending.hub = Some(Arc::clone(hub));
    }
}

impl Hubs {
    pub fn new(proxy: Proxy, commands: bool) -> Self {
        Self {
            proxy,
            commands,
            instance: OnceCell::new(),
            organizations: DashMap::new(),
        }
    }

    pub async fn instance(&self) -> Arc<McpHub> {
        Arc::clone(
            self.instance
                .get_or_init(|| async {
                    Arc::new(McpHub::connect_through(&servers::config(), &self.proxy).await)
                })
                .await,
        )
    }

    pub fn existing_instance(&self) -> Option<Arc<McpHub>> {
        self.instance.get().cloned()
    }

    pub fn install_instance(&self, hub: McpHub) {
        let _ = self.instance.set(Arc::new(hub));
    }

    pub async fn organization<F, Fut>(&self, organization: Uuid, load: F) -> Arc<OrganizationHub>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Vec<Definition>>,
    {
        let entry = Arc::clone(self.organizations.entry(organization).or_default().value());
        let hub = entry
            .cell
            .get_or_init(|| async {
                let definitions = load().await;
                let hub = Arc::new(
                    OrganizationHub::connect(definitions, self.proxy.clone(), self.commands).await,
                );
                entry.drain(&hub);
                hub
            })
            .await;
        Arc::clone(hub)
    }

    pub fn existing_organization(&self, organization: Uuid) -> Option<Arc<OrganizationHub>> {
        self.organizations
            .get(&organization)
            .and_then(|entry| entry.cell.get().cloned())
    }

    pub fn organizations(&self) -> usize {
        self.organizations
            .iter()
            .filter(|entry| entry.cell.initialized())
            .count()
    }

    pub fn refresh(&self, organization: Uuid, definition: Definition) {
        self.submit(organization, Change::Refresh(Box::new(definition)));
    }

    pub fn remove(&self, organization: Uuid, name: &str) {
        self.submit(organization, Change::Remove(name.to_string()));
    }

    pub fn instance_connection(&self, name: &str) -> Connection {
        let Some(hub) = self.existing_instance() else {
            return Connection::connecting();
        };
        if !hub.server_names().iter().any(|server| server == name) {
            return Connection::failed(UNCONNECTED);
        }
        Connection::connected(advertised(name, &hub))
    }

    fn submit(&self, organization: Uuid, change: Change) {
        let Some(entry) = self
            .organizations
            .get(&organization)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return;
        };
        entry.submit(change);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use abnegate_agent::ToolRegistry;
    use abnegate_agent::mcp::{McpServer, McpTransport};
    use tokio::sync::Notify;

    use super::*;
    use crate::services::mcp_servers::status::{
        COMMANDS_DISABLED, HTTP_PENDING, SSE_UNSUPPORTED, Status,
    };

    const LIMIT: Duration = Duration::from_secs(30);

    fn hubs() -> Hubs {
        Hubs::new(Proxy::default(), true)
    }

    async fn built(
        hubs: &Hubs,
        organization: Uuid,
        definitions: Vec<Definition>,
    ) -> Arc<OrganizationHub> {
        hubs.organization(organization, || async move { definitions })
            .await
    }

    async fn settled(hub: &OrganizationHub, name: &str) -> Connection {
        tokio::time::timeout(LIMIT, async {
            loop {
                let connection = hub.connection(name);
                if connection.status != Status::Connecting {
                    return connection;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the connection settles")
    }

    fn delegate() -> Connection {
        Connection::connected(vec!["delegate".to_string()])
    }

    #[tokio::test]
    async fn the_lazy_load_runs_once() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let loads = AtomicUsize::new(0);
        let load = || async {
            loads.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            vec![Definition::stub("stub")]
        };

        let (first, second) = tokio::join!(
            hubs.organization(organization, load),
            hubs.organization(organization, load)
        );
        let third = hubs.organization(organization, load).await;

        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &third));
        assert_eq!(first.connection("stub"), delegate());
        assert_eq!(hubs.organizations(), 1);
    }

    #[tokio::test]
    async fn refresh_connects_a_new_server_through_connecting() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let hub = built(&hubs, organization, Vec::new()).await;

        hubs.refresh(organization, Definition::stub("stub"));

        assert_eq!(hub.connection("stub"), Connection::connecting());
        assert_eq!(settled(&hub, "stub").await, delegate());
    }

    #[tokio::test]
    async fn the_later_of_two_quick_refreshes_wins() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let hub = built(&hubs, organization, vec![Definition::stub("stub")]).await;
        let earlier = hub.session("stub").expect("the load connected the stub");

        hubs.refresh(organization, Definition::stub_marked("stub", &["first"]));
        hubs.refresh(organization, Definition::stub_marked("stub", &["second"]));

        assert_eq!(settled(&hub, "stub").await, delegate());
        let arguments = hub.definition("stub").unwrap().server.arguments;
        assert_eq!(arguments.last().map(String::as_str), Some("second"));
        let session = hub.session("stub").unwrap();
        assert!(!Arc::ptr_eq(&earlier, &session), "the session was replaced");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            Arc::ptr_eq(&session, &hub.session("stub").unwrap()),
            "the earlier connect never lands after the later one"
        );
    }

    #[tokio::test]
    async fn refresh_before_the_hub_exists_does_nothing() {
        let hubs = hubs();
        let organization = Uuid::new_v4();

        hubs.refresh(organization, Definition::stub("stub"));
        hubs.remove(organization, "stub");

        assert!(hubs.existing_organization(organization).is_none());
        assert_eq!(hubs.organizations(), 0);
    }

    #[tokio::test]
    async fn remove_drops_the_server() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let hub = built(&hubs, organization, vec![Definition::stub("stub")]).await;
        assert_eq!(hub.connected(), 1);

        hubs.remove(organization, "stub");

        assert_eq!(hub.connected(), 0);
        assert!(hub.session("stub").is_none());
        assert_eq!(hub.connection("stub"), Connection::failed(UNCONNECTED));
        let mut registry = ToolRegistry::new();
        assert_eq!(hub.register(&mut registry, &|_| true), 0);
    }

    #[tokio::test]
    async fn remove_then_refresh_reconnects() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let definition = Definition::stub("stub");
        let hub = built(&hubs, organization, vec![definition.clone()]).await;
        let earlier = hub.session("stub").unwrap();

        hubs.remove(organization, "stub");
        hubs.refresh(organization, definition);

        assert_eq!(hub.connection("stub"), Connection::connecting());
        assert_eq!(settled(&hub, "stub").await, delegate());
        assert!(!Arc::ptr_eq(&earlier, &hub.session("stub").unwrap()));
    }

    #[tokio::test]
    async fn a_tools_only_refresh_keeps_the_session() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let definition = Definition::stub("stub");
        let hub = built(&hubs, organization, vec![definition.clone()]).await;
        let session = hub.session("stub").unwrap();

        hubs.refresh(
            organization,
            Definition {
                tools: Some(vec!["other".to_string()]),
                ..definition
            },
        );

        assert_eq!(hub.connection("stub"), delegate());
        assert!(Arc::ptr_eq(&session, &hub.session("stub").unwrap()));
        let mut registry = ToolRegistry::new();
        assert_eq!(hub.register(&mut registry, &|_| true), 0);
    }

    #[tokio::test]
    async fn a_tools_only_refresh_during_a_connect_survives_it() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let hub = built(&hubs, organization, Vec::new()).await;
        let definition = Definition::stub("stub");

        hubs.refresh(organization, definition.clone());
        hubs.refresh(
            organization,
            Definition {
                tools: Some(vec!["other".to_string()]),
                ..definition
            },
        );

        assert_eq!(settled(&hub, "stub").await, delegate());
        assert_eq!(
            hub.definition("stub").unwrap().tools,
            Some(vec!["other".to_string()])
        );
    }

    #[tokio::test]
    async fn changes_made_during_the_load_are_applied() {
        let hubs = Arc::new(hubs());
        let organization = Uuid::new_v4();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let loading = tokio::spawn({
            let hubs = Arc::clone(&hubs);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            async move {
                hubs.organization(organization, || async move {
                    started.notify_one();
                    release.notified().await;
                    vec![Definition::stub("old")]
                })
                .await
            }
        });
        started.notified().await;

        hubs.refresh(organization, Definition::stub("new"));
        hubs.remove(organization, "old");
        assert!(hubs.existing_organization(organization).is_none());
        release.notify_one();
        let hub = loading.await.unwrap();

        assert!(hub.definition("old").is_none());
        assert!(hub.session("old").is_none());
        assert_eq!(settled(&hub, "new").await, delegate());
    }

    #[tokio::test]
    async fn a_refresh_after_the_load_returns_is_applied() {
        let hubs = Arc::new(hubs());
        let organization = Uuid::new_v4();
        let loaded = Arc::new(Notify::new());
        let loading = tokio::spawn({
            let hubs = Arc::clone(&hubs);
            let loaded = Arc::clone(&loaded);
            async move {
                hubs.organization(organization, || async move {
                    loaded.notify_one();
                    vec![Definition::stub("stub")]
                })
                .await
            }
        });
        loaded.notified().await;

        hubs.refresh(organization, Definition::stub_marked("stub", &["later"]));
        let hub = loading.await.unwrap();

        let arguments = hub.definition("stub").unwrap().server.arguments;
        assert_eq!(arguments.last().map(String::as_str), Some("later"));
        assert_eq!(settled(&hub, "stub").await, delegate());
    }

    #[tokio::test]
    async fn commands_off_never_spawns_a_command_server() {
        let hubs = Hubs::new(Proxy::default(), false);
        let organization = Uuid::new_v4();
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("spawned");
        let touching = |name: &str| {
            Definition::of(
                name,
                McpServer::command(
                    "sh",
                    [
                        "-c".to_string(),
                        "touch \"$1\"".to_string(),
                        "sh".to_string(),
                        marker.to_string_lossy().into_owned(),
                    ],
                ),
            )
        };

        let hub = built(&hubs, organization, vec![touching("loaded")]).await;
        hubs.refresh(organization, touching("refreshed"));

        assert_eq!(
            hub.connection("loaded"),
            Connection::failed(COMMANDS_DISABLED)
        );
        assert_eq!(
            hub.connection("refreshed"),
            Connection::failed(COMMANDS_DISABLED)
        );
        assert!(!marker.exists(), "a command server was spawned");
    }

    #[tokio::test]
    async fn remote_definitions_report_why_they_are_not_connected() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let sse = Definition::of(
            "events",
            McpServer::remote("http://127.0.0.1:1/sse").with_transport(McpTransport::Sse),
        );
        let http = Definition::of("remote", McpServer::remote("http://127.0.0.1:1/mcp"));

        let hub = built(&hubs, organization, vec![sse, http]).await;

        assert_eq!(
            hub.connection("events"),
            Connection::failed(SSE_UNSUPPORTED)
        );
        assert_eq!(hub.connection("remote"), Connection::failed(HTTP_PENDING));
        assert_eq!(hub.connected(), 0);
    }

    #[tokio::test]
    async fn disabling_a_server_drops_its_session() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let definition = Definition::stub("stub");
        let hub = built(&hubs, organization, vec![definition.clone()]).await;

        hubs.refresh(
            organization,
            Definition {
                enabled: false,
                ..definition
            },
        );

        assert_eq!(hub.connection("stub"), Connection::disabled());
        assert!(hub.session("stub").is_none());
    }

    #[tokio::test]
    async fn an_unknown_name_is_unconnected() {
        let hubs = hubs();
        let hub = built(&hubs, Uuid::new_v4(), Vec::new()).await;

        assert_eq!(hub.connection("absent"), Connection::failed(UNCONNECTED));
    }

    #[tokio::test]
    async fn the_existing_methods_never_connect() {
        let hubs = hubs();
        let organization = Uuid::new_v4();

        assert!(hubs.existing_instance().is_none());
        assert!(hubs.existing_organization(organization).is_none());

        assert!(!hubs.instance.initialized());
        assert!(hubs.organizations.is_empty());
        assert_eq!(hubs.organizations(), 0);
    }

    #[tokio::test]
    async fn the_instance_layer_is_connecting_until_its_hub_exists() {
        let hubs = hubs();
        assert_eq!(hubs.instance_connection("docs"), Connection::connecting());

        hubs.install_instance(McpHub::new());

        assert_eq!(
            hubs.instance_connection("docs"),
            Connection::failed(UNCONNECTED)
        );
        assert_eq!(hubs.instance().await.server_count(), 0);
    }

    #[tokio::test]
    async fn the_first_installed_instance_hub_wins() {
        let hubs = hubs();
        let definition = Definition::stub("stub");
        let stub = McpHub::connect_through(
            &abnegate_agent::mcp::McpConfig::default().with_server("stub", definition.server),
            &Proxy::default(),
        )
        .await;
        assert_eq!(stub.server_count(), 1, "the stub server has to connect");

        hubs.install_instance(stub);
        hubs.install_instance(McpHub::new());

        assert_eq!(hubs.instance_connection("stub"), delegate());
        assert_eq!(
            hubs.instance_connection("other"),
            Connection::failed(UNCONNECTED)
        );
    }
}
