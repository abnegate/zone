use std::future::Future;
use std::sync::Arc;
use std::sync::OnceLock;

use abnegate_agent::mcp::McpConfig;
use abnegate_agent::mcp::McpHub;
use abnegate_agent::tool::Proxy;
use dashmap::DashMap;
use tokio::sync::Mutex;
use tokio::sync::OnceCell;
use uuid::Uuid;
use zone_core::servers;

use super::definition::Definition;
use super::definition::Revisions;
use super::organization::OrganizationHub;
use super::sessions::advertised;
use super::status::Connection;
use super::status::Policy;
use super::status::UNCONNECTED;

pub struct Hubs {
    proxy: Proxy,
    policy: Policy,
    configuration: OnceCell<Arc<McpConfig>>,
    instance: OnceCell<Arc<McpHub>>,
    organizations: DashMap<Uuid, Arc<Entry>>,
}

// `hub` is written only while `gate` is held, and only after the rows read under that same hold
// were applied, so whoever holds `gate` last leaves the hub equal to the database.
#[derive(Default)]
struct Entry {
    hub: OnceLock<Arc<OrganizationHub>>,
    gate: Mutex<()>,
}

impl Hubs {
    pub fn new(proxy: Proxy, commands: bool) -> Self {
        Self::with(
            proxy,
            Policy {
                enabled: servers::enabled(),
                commands,
            },
        )
    }

    fn with(proxy: Proxy, policy: Policy) -> Self {
        Self {
            proxy,
            policy,
            configuration: OnceCell::new(),
            instance: OnceCell::new(),
            organizations: DashMap::new(),
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub async fn configured(&self) -> Arc<McpConfig> {
        let read = self
            .configuration
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(servers::config)
                    .await
                    .map(Arc::new)
            })
            .await;
        match read {
            Ok(configuration) => Arc::clone(configuration),
            Err(error) => {
                tracing::warn!(%error, "Could not read the instance's MCP servers");
                Arc::new(McpConfig::default())
            }
        }
    }

    pub async fn instance(&self) -> Arc<McpHub> {
        Arc::clone(
            self.instance
                .get_or_init(|| async {
                    let configuration = self.configured().await;
                    Arc::new(McpHub::connect_through(&configuration, &self.proxy).await)
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

    pub async fn organization<F, Fut, E>(
        &self,
        organization: Uuid,
        load: F,
    ) -> Result<Arc<OrganizationHub>, E>
    where
        F: FnOnce(Revisions) -> Fut,
        Fut: Future<Output = Result<Option<Vec<Definition>>, E>>,
    {
        let entry = Arc::clone(self.organizations.entry(organization).or_default().value());
        let hub = {
            let _gate = entry.gate.lock().await;
            let hub = match entry.hub.get() {
                Some(hub) => Arc::clone(hub),
                None => Arc::new(OrganizationHub::new(self.proxy.clone(), self.policy)),
            };
            reconcile(&entry, hub, None, load).await?
        };
        hub.settled().await;
        Ok(hub)
    }

    pub async fn sync<F, Fut, E>(
        &self,
        organization: Uuid,
        load: F,
    ) -> Result<Option<Arc<OrganizationHub>>, E>
    where
        F: FnOnce(Revisions) -> Fut,
        Fut: Future<Output = Result<Option<Vec<Definition>>, E>>,
    {
        self.built(organization, None, load).await
    }

    pub async fn reconnect<F, Fut, E>(
        &self,
        organization: Uuid,
        server: Uuid,
        load: F,
    ) -> Result<Option<Arc<OrganizationHub>>, E>
    where
        F: FnOnce(Revisions) -> Fut,
        Fut: Future<Output = Result<Option<Vec<Definition>>, E>>,
    {
        self.built(organization, Some(server), load).await
    }

    pub fn forget(&self, organization: Uuid) {
        self.organizations.remove(&organization);
    }

    pub fn existing_organization(&self, organization: Uuid) -> Option<Arc<OrganizationHub>> {
        self.entry(organization)
            .and_then(|entry| entry.hub.get().cloned())
    }

    pub fn organizations(&self) -> usize {
        self.organizations
            .iter()
            .filter(|entry| entry.hub.get().is_some())
            .count()
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

    fn entry(&self, organization: Uuid) -> Option<Arc<Entry>> {
        self.organizations
            .get(&organization)
            .map(|entry| Arc::clone(entry.value()))
    }

    async fn built<F, Fut, E>(
        &self,
        organization: Uuid,
        forced: Option<Uuid>,
        load: F,
    ) -> Result<Option<Arc<OrganizationHub>>, E>
    where
        F: FnOnce(Revisions) -> Fut,
        Fut: Future<Output = Result<Option<Vec<Definition>>, E>>,
    {
        let Some(entry) = self.entry(organization) else {
            return Ok(None);
        };
        let _gate = entry.gate.lock().await;
        let Some(hub) = entry.hub.get().cloned() else {
            return Ok(None);
        };
        reconcile(&entry, hub, forced, load).await.map(Some)
    }
}

async fn reconcile<F, Fut, E>(
    entry: &Entry,
    hub: Arc<OrganizationHub>,
    forced: Option<Uuid>,
    load: F,
) -> Result<Arc<OrganizationHub>, E>
where
    F: FnOnce(Revisions) -> Fut,
    Fut: Future<Output = Result<Option<Vec<Definition>>, E>>,
{
    let known = match forced {
        Some(_) => Revisions::new(),
        None => hub.revisions(),
    };
    if let Some(definitions) = load(known).await? {
        hub.apply(definitions, forced);
    }
    Ok(Arc::clone(entry.hub.get_or_init(|| hub)))
}

#[cfg(test)]
mod tests {
    use std::future::Ready;
    use std::future::ready;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use abnegate_agent::ToolRegistry;
    use abnegate_agent::mcp::McpServer;
    use abnegate_agent::mcp::McpTransport;
    use tokio::sync::Notify;
    use zone_core::variables::Variables;

    use super::*;
    use crate::services::mcp_servers::organization::Registered;
    use crate::services::mcp_servers::status::COMMANDS_DISABLED;
    use crate::services::mcp_servers::status::HTTP_PENDING;
    use crate::services::mcp_servers::status::MCP_DISABLED;
    use crate::services::mcp_servers::status::SSE_UNSUPPORTED;
    use crate::services::mcp_servers::status::Status;

    const LIMIT: Duration = Duration::from_secs(30);
    const ENABLED: &str = "ZONE_MCP_ENABLED";
    const UNREADABLE: &str = "the database is away";

    const POLICY: Policy = Policy {
        enabled: true,
        commands: true,
    };

    type Loaded = Result<Option<Vec<Definition>>, &'static str>;

    /// The rows an organization stores, read the way `load` reads the database: nothing is handed
    /// over while the revisions the hub knows are the stored ones.
    #[derive(Default)]
    struct Rows {
        stored: std::sync::Mutex<Vec<Definition>>,
        loads: AtomicUsize,
    }

    impl Rows {
        fn storing(definitions: Vec<Definition>) -> Self {
            let rows = Self::default();
            rows.store(definitions);
            rows
        }

        fn store(&self, definitions: Vec<Definition>) {
            *self.stored.lock().unwrap() = definitions;
        }

        fn load(&self) -> impl FnOnce(Revisions) -> Ready<Loaded> + '_ {
            move |known| {
                self.loads.fetch_add(1, Ordering::SeqCst);
                let stored = self.stored.lock().unwrap().clone();
                let revisions: Revisions = stored
                    .iter()
                    .map(|definition| (definition.id, definition.revision))
                    .collect();
                ready(Ok((revisions != known).then_some(stored)))
            }
        }
    }

    fn hubs() -> Hubs {
        Hubs::with(Proxy::default(), POLICY)
    }

    fn unreadable(_: Revisions) -> Ready<Loaded> {
        ready(Err(UNREADABLE))
    }

    async fn built(hubs: &Hubs, organization: Uuid, rows: &Rows) -> Arc<OrganizationHub> {
        hubs.organization(organization, rows.load())
            .await
            .expect("the rows load")
    }

    async fn synced(hubs: &Hubs, organization: Uuid, rows: &Rows) -> Arc<OrganizationHub> {
        hubs.sync(organization, rows.load())
            .await
            .expect("the rows load")
            .expect("the organization's hub is built")
    }

    async fn settled(hub: &OrganizationHub, name: &str) -> Connection {
        tokio::time::timeout(LIMIT, hub.settled())
            .await
            .expect("the connection settles");
        hub.connection(name)
    }

    fn delegate() -> Connection {
        Connection::connected(vec!["delegate".to_string()])
    }

    fn narrowed(definition: &Definition) -> Definition {
        Definition {
            tools: Some(vec!["other".to_string()]),
            ..definition.clone().revised()
        }
    }

    #[tokio::test]
    async fn a_hub_is_built_once_and_reconciled_on_every_use() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::storing(vec![Definition::stub("stub")]);

        let (first, second) = tokio::join!(
            hubs.organization(organization, rows.load()),
            hubs.organization(organization, rows.load())
        );
        let (first, second) = (first.unwrap(), second.unwrap());
        let generation = first.generation("stub");
        let session = first.session("stub").expect("the load connected the stub");
        let third = built(&hubs, organization, &rows).await;

        assert_eq!(rows.loads.load(Ordering::SeqCst), 3);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &third));
        assert_eq!(first.connection("stub"), delegate());
        assert_eq!(hubs.organizations(), 1);
        assert_eq!(third.generation("stub"), generation);
        assert!(Arc::ptr_eq(&session, &third.session("stub").unwrap()));
    }

    #[tokio::test]
    async fn an_unchanged_row_does_not_reconnect() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let definition = Definition::stub("stub");
        let rows = Rows::storing(vec![definition.clone()]);
        let hub = built(&hubs, organization, &rows).await;
        let generation = hub.generation("stub");
        let session = hub.session("stub").unwrap();

        hubs.sync(organization, |_| {
            ready(Loaded::Ok(Some(vec![definition.clone()])))
        })
        .await
        .unwrap();

        assert_eq!(hub.connection("stub"), delegate());
        assert_eq!(hub.generation("stub"), generation);
        assert!(Arc::ptr_eq(&session, &hub.session("stub").unwrap()));
    }

    #[tokio::test]
    async fn a_new_row_connects_through_connecting() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::default();
        let hub = built(&hubs, organization, &rows).await;

        rows.store(vec![Definition::stub("stub")]);
        synced(&hubs, organization, &rows).await;

        assert_eq!(hub.connection("stub"), Connection::connecting());
        assert!(hub.connecting());
        assert_eq!(settled(&hub, "stub").await, delegate());
        assert!(!hub.connecting());
    }

    #[tokio::test]
    async fn the_later_of_two_quick_changes_wins() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let stored = Definition::stub("stub");
        let rows = Rows::storing(vec![stored.clone()]);
        let hub = built(&hubs, organization, &rows).await;
        let earlier = hub.session("stub").expect("the load connected the stub");

        let first = stored.revised().marked("first");
        rows.store(vec![first.clone()]);
        synced(&hubs, organization, &rows).await;
        rows.store(vec![first.revised().marked("second")]);
        synced(&hubs, organization, &rows).await;

        assert_eq!(settled(&hub, "stub").await, delegate());
        let stored = hub.definition("stub").unwrap();
        assert_eq!(stored.marker(), Some("second"));
        let session = hub.session("stub").unwrap();
        assert!(!Arc::ptr_eq(&earlier, &session), "the session was replaced");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            Arc::ptr_eq(&session, &hub.session("stub").unwrap()),
            "the earlier connect never lands after the later one"
        );
    }

    #[tokio::test]
    async fn nothing_is_read_before_the_hub_exists() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::storing(vec![Definition::stub("stub")]);

        let synced = hubs.sync(organization, rows.load()).await.unwrap();
        let reconnected = hubs
            .reconnect(organization, Uuid::new_v4(), rows.load())
            .await
            .unwrap();

        assert!(synced.is_none());
        assert!(reconnected.is_none());
        assert_eq!(rows.loads.load(Ordering::SeqCst), 0);
        assert!(hubs.existing_organization(organization).is_none());
        assert_eq!(hubs.organizations(), 0);
    }

    #[tokio::test]
    async fn a_removed_row_drops_its_server() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::storing(vec![Definition::stub("stub")]);
        let hub = built(&hubs, organization, &rows).await;
        assert_eq!(hub.connected(), 1);
        let session = Arc::downgrade(&hub.session("stub").unwrap());

        rows.store(Vec::new());
        synced(&hubs, organization, &rows).await;

        assert_eq!(hub.connected(), 0);
        assert!(hub.names().is_empty());
        assert!(session.upgrade().is_none(), "the session was not dropped");
        assert_eq!(hub.connection("stub"), Connection::failed(UNCONNECTED));
        let mut registry = ToolRegistry::new();
        assert_eq!(
            hub.register(&mut registry, &|_| true),
            Registered::default()
        );
    }

    #[tokio::test]
    async fn a_renamed_row_replaces_the_slot_under_its_old_name() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let stored = Definition::stub("old");
        let rows = Rows::storing(vec![stored.clone()]);
        let hub = built(&hubs, organization, &rows).await;
        let earlier = Arc::downgrade(&hub.session("old").unwrap());

        rows.store(vec![Definition {
            name: "new".to_string(),
            ..stored.revised()
        }]);
        synced(&hubs, organization, &rows).await;

        assert_eq!(hub.names(), ["new"]);
        assert_eq!(hub.connection("old"), Connection::failed(UNCONNECTED));
        assert_eq!(hub.connection("new"), Connection::connecting());
        assert!(earlier.upgrade().is_none(), "the old session was kept");
        assert_eq!(settled(&hub, "new").await, delegate());
        let mut registry = ToolRegistry::new();
        hub.register(&mut registry, &|_| true);
        assert_eq!(registry.names(), ["new__delegate"]);
    }

    #[tokio::test]
    async fn a_reconnect_replaces_the_session_of_an_unchanged_row() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let definition = Definition::stub("stub");
        let other = Definition::stub("other");
        let rows = Rows::storing(vec![definition.clone(), other]);
        let hub = built(&hubs, organization, &rows).await;
        let earlier = hub.session("stub").unwrap();
        let untouched = hub.session("other").unwrap();
        let generation = hub.generation("other");

        hubs.reconnect(organization, definition.id, rows.load())
            .await
            .unwrap();

        assert_eq!(hub.connection("stub"), Connection::connecting());
        assert_eq!(hub.connection("other"), delegate());
        assert_eq!(settled(&hub, "stub").await, delegate());
        assert!(!Arc::ptr_eq(&earlier, &hub.session("stub").unwrap()));
        assert!(Arc::ptr_eq(&untouched, &hub.session("other").unwrap()));
        assert_eq!(hub.generation("other"), generation);
    }

    #[tokio::test]
    async fn a_tools_only_change_keeps_the_session() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let definition = Definition::stub("stub");
        let rows = Rows::storing(vec![definition.clone()]);
        let hub = built(&hubs, organization, &rows).await;
        let session = hub.session("stub").unwrap();
        let generation = hub.generation("stub");

        rows.store(vec![narrowed(&definition)]);
        synced(&hubs, organization, &rows).await;

        assert_eq!(hub.connection("stub"), delegate());
        assert_eq!(hub.generation("stub"), generation);
        assert!(Arc::ptr_eq(&session, &hub.session("stub").unwrap()));
        let mut registry = ToolRegistry::new();
        assert_eq!(
            hub.register(&mut registry, &|_| true),
            Registered::default()
        );
    }

    #[tokio::test]
    async fn a_tools_only_change_during_a_connect_survives_it() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::default();
        let hub = built(&hubs, organization, &rows).await;
        let definition = Definition::stub("stub");

        rows.store(vec![definition.clone()]);
        synced(&hubs, organization, &rows).await;
        rows.store(vec![narrowed(&definition)]);
        synced(&hubs, organization, &rows).await;

        assert_eq!(settled(&hub, "stub").await, delegate());
        assert_eq!(
            hub.definition("stub").unwrap().tools,
            Some(vec!["other".to_string()])
        );
    }

    #[tokio::test]
    async fn a_change_committed_during_the_first_load_is_not_lost() {
        let hubs = Arc::new(hubs());
        let organization = Uuid::new_v4();
        let rows = Arc::new(Rows::storing(vec![Definition::stub("old")]));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let loading = tokio::spawn({
            let hubs = Arc::clone(&hubs);
            let rows = Arc::clone(&rows);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            async move {
                hubs.organization(organization, |known| async move {
                    let read = rows.load()(known).await;
                    started.notify_one();
                    release.notified().await;
                    read
                })
                .await
            }
        });
        started.notified().await;

        rows.store(vec![Definition::stub("new")]);
        let writing = tokio::spawn({
            let hubs = Arc::clone(&hubs);
            let rows = Arc::clone(&rows);
            async move { hubs.sync(organization, rows.load()).await }
        });
        tokio::task::yield_now().await;
        assert!(hubs.existing_organization(organization).is_none());
        release.notify_one();
        let hub = loading.await.unwrap().unwrap();
        writing.await.unwrap().unwrap();

        assert_eq!(hub.names(), ["new"]);
        assert!(hub.session("old").is_none());
        assert_eq!(settled(&hub, "new").await, delegate());
    }

    /// Two requests change one organization's rows: the first commits `earlier`, the second
    /// commits `later`, and the first request's read, taken before the second committed, reaches
    /// the hub last.
    async fn raced(
        stored: Vec<Definition>,
        earlier: Vec<Definition>,
        later: Vec<Definition>,
    ) -> Arc<OrganizationHub> {
        let hubs = Arc::new(hubs());
        let organization = Uuid::new_v4();
        let rows = Arc::new(Rows::storing(stored));
        let hub = built(&hubs, organization, &rows).await;
        let read = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());

        rows.store(earlier);
        let first = tokio::spawn({
            let hubs = Arc::clone(&hubs);
            let rows = Arc::clone(&rows);
            let read = Arc::clone(&read);
            let release = Arc::clone(&release);
            async move {
                hubs.sync(organization, |known| async move {
                    let stale = rows.load()(known).await;
                    read.notify_one();
                    release.notified().await;
                    stale
                })
                .await
            }
        });
        read.notified().await;
        rows.store(later);
        let second = tokio::spawn({
            let hubs = Arc::clone(&hubs);
            let rows = Arc::clone(&rows);
            async move { hubs.sync(organization, rows.load()).await }
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        release.notify_one();
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        tokio::time::timeout(LIMIT, hub.settled())
            .await
            .expect("the connections settle");
        hub
    }

    #[tokio::test]
    async fn two_racing_renames_leave_one_slot_for_the_row() {
        let stored = Definition::stub("x");
        let first = Definition {
            name: "y".to_string(),
            ..stored.clone().revised()
        };
        let second = Definition {
            name: "z".to_string(),
            ..first.clone().revised()
        };

        let hub = raced(vec![stored], vec![first], vec![second]).await;

        assert_eq!(hub.names(), ["z"]);
        assert_eq!(hub.connection("z"), delegate());
        assert_eq!(hub.connection("y"), Connection::failed(UNCONNECTED));
    }

    #[tokio::test]
    async fn an_update_racing_a_delete_leaves_no_slot() {
        let stored = Definition::stub("x");
        let updated = stored.clone().revised().marked("updated");

        let hub = raced(vec![stored], vec![updated], Vec::new()).await;

        assert!(hub.names().is_empty(), "{:?}", hub.names());
        assert_eq!(hub.connected(), 0);
    }

    #[tokio::test]
    async fn two_racing_updates_leave_the_stored_allowlist() {
        let stored = Definition::stub("x");
        let first = narrowed(&stored);
        let second = Definition {
            tools: Some(vec!["delegate".to_string()]),
            ..first.clone().revised()
        };

        let hub = raced(vec![stored], vec![first], vec![second]).await;

        assert_eq!(
            hub.definition("x").unwrap().tools,
            Some(vec!["delegate".to_string()])
        );
        let mut registry = ToolRegistry::new();
        hub.register(&mut registry, &|_| true);
        assert_eq!(registry.names(), ["x__delegate"]);
    }

    #[tokio::test]
    async fn a_failed_first_load_is_not_kept() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::storing(vec![Definition::stub("stub")]);

        let failed = hubs.organization(organization, unreadable).await;
        let unbuilt = hubs.existing_organization(organization).is_none();
        let counted = hubs.organizations();
        let hub = built(&hubs, organization, &rows).await;

        assert_eq!(failed.err(), Some(UNREADABLE));
        assert!(unbuilt, "a failed load left a hub behind");
        assert_eq!(counted, 0);
        assert_eq!(hub.connection("stub"), delegate());
        assert_eq!(hubs.organizations(), 1);
    }

    #[tokio::test]
    async fn a_failed_read_keeps_what_is_connected() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::storing(vec![Definition::stub("stub")]);
        let hub = built(&hubs, organization, &rows).await;

        let failed = hubs.sync(organization, unreadable).await;

        assert_eq!(failed.err(), Some(UNREADABLE));
        assert_eq!(hub.connection("stub"), delegate());
    }

    #[tokio::test]
    async fn forgetting_an_organization_drops_its_hub_and_its_sessions() {
        let hubs = hubs();
        let organization = Uuid::new_v4();
        let rows = Rows::storing(vec![Definition::stub("stub")]);
        let hub = built(&hubs, organization, &rows).await;
        let session = Arc::downgrade(&hub.session("stub").unwrap());
        let held = Arc::downgrade(&hub);
        drop(hub);

        hubs.forget(organization);

        assert!(hubs.existing_organization(organization).is_none());
        assert_eq!(hubs.organizations(), 0);
        assert!(
            held.upgrade().is_none(),
            "the hub outlived its organization"
        );
        assert!(
            session.upgrade().is_none(),
            "the session outlived its organization"
        );
    }

    #[tokio::test]
    async fn commands_off_never_spawns_a_command_server() {
        let hubs = Hubs::with(
            Proxy::default(),
            Policy {
                enabled: true,
                commands: false,
            },
        );
        let organization = Uuid::new_v4();
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("spawned");
        let rows = Rows::storing(vec![touching("loaded", &marker)]);

        let hub = built(&hubs, organization, &rows).await;
        rows.store(vec![
            touching("loaded", &marker),
            touching("added", &marker),
        ]);
        synced(&hubs, organization, &rows).await;

        for name in ["loaded", "added"] {
            assert_eq!(
                settled(&hub, name).await,
                Connection::failed(COMMANDS_DISABLED)
            );
        }
        assert!(!marker.exists(), "a command server was spawned");
    }

    #[tokio::test]
    async fn the_master_switch_off_never_spawns_a_server() {
        let hubs = Hubs::with(
            Proxy::default(),
            Policy {
                enabled: false,
                commands: true,
            },
        );
        let organization = Uuid::new_v4();
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("spawned");
        let loaded = touching("loaded", &marker);
        let rows = Rows::storing(vec![loaded.clone()]);

        let hub = built(&hubs, organization, &rows).await;
        rows.store(vec![loaded.clone(), touching("added", &marker)]);
        synced(&hubs, organization, &rows).await;
        hubs.reconnect(organization, loaded.id, rows.load())
            .await
            .unwrap();

        for name in ["loaded", "added"] {
            assert_eq!(settled(&hub, name).await, Connection::failed(MCP_DISABLED));
        }
        assert_eq!(hub.connected(), 0);
        let mut registry = ToolRegistry::new();
        assert_eq!(
            hub.register(&mut registry, &|_| true),
            Registered::default()
        );
        assert!(!marker.exists(), "a server was spawned");
    }

    #[test]
    fn the_master_switch_is_read_when_the_hubs_are_made() {
        let mut variables = Variables::isolated(&[ENABLED]);
        let on = Hubs::new(Proxy::default(), true).policy();
        variables.set(ENABLED, "false");
        let off = Hubs::new(Proxy::default(), true).policy();
        variables.set(ENABLED, "true");

        assert_eq!(on, POLICY);
        assert_eq!(
            off,
            Policy {
                enabled: false,
                commands: true,
            }
        );
    }

    fn touching(name: &str, marker: &std::path::Path) -> Definition {
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

        let hub = built(&hubs, organization, &Rows::storing(vec![sse, http])).await;

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
        let rows = Rows::storing(vec![definition.clone()]);
        let hub = built(&hubs, organization, &rows).await;

        rows.store(vec![Definition {
            enabled: false,
            ..definition.revised()
        }]);
        synced(&hubs, organization, &rows).await;

        assert_eq!(hub.connection("stub"), Connection::disabled());
        assert!(hub.session("stub").is_none());
    }

    #[tokio::test]
    async fn an_unknown_name_is_unconnected() {
        let hubs = hubs();
        let hub = built(&hubs, Uuid::new_v4(), &Rows::default()).await;

        assert_eq!(hub.connection("absent"), Connection::failed(UNCONNECTED));
        assert_eq!(hub.connection("absent").status, Status::Failed);
    }

    #[tokio::test]
    async fn the_existing_methods_never_connect() {
        let hubs = hubs();
        let organization = Uuid::new_v4();

        assert!(hubs.existing_instance().is_none());
        assert!(hubs.existing_organization(organization).is_none());

        assert!(!hubs.instance.initialized());
        assert!(!hubs.configuration.initialized());
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
            &McpConfig::default().with_server("stub", definition.server),
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

    #[tokio::test]
    async fn the_instance_configuration_is_read_once() {
        let mut variables = Variables::isolated(&[
            ENABLED,
            "ZONE_MCP_SERVERS",
            "ZONE_MCP_CONFIG",
            "ZONE_MCP_AUTO_CONNECT",
            "ZONE_MCP_AUTO_MAGENTS",
        ]);
        variables.set(
            "ZONE_MCP_SERVERS",
            r#"{"mcpServers":{"first":{"command":"true"}}}"#,
        );
        let hubs = hubs();

        let first = hubs.configured().await;
        variables.set(
            "ZONE_MCP_SERVERS",
            r#"{"mcpServers":{"second":{"command":"true"}}}"#,
        );
        let second = hubs.configured().await;

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            second.servers.keys().collect::<Vec<_>>(),
            ["first"],
            "the configuration was read again"
        );
    }
}
