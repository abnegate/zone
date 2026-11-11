use std::sync::Arc;

use abnegate_agent::mcp::McpServer;
use uuid::Uuid;
use zone_core::servers::mask;

use crate::db::mcp_servers;
use crate::db::mcp_servers::McpServerError;
use crate::db::mcp_servers::McpServerRecord;
use crate::state::AppState;

use super::definition::Definition;
use super::definition::Revisions;
use super::definition::Snapshot;
use super::organization::OrganizationHub;
use super::transport::Transport;

pub fn definition(record: &McpServerRecord) -> Definition {
    Definition {
        id: record.id,
        revision: record.updated_at,
        name: record.name.clone(),
        server: server(record),
        enabled: record.enabled,
        tools: record.tools.clone(),
    }
}

pub async fn organization(state: &AppState, organization: Uuid) -> Option<Arc<OrganizationHub>> {
    state
        .mcp()
        .organization(organization, |known| changed(state, organization, known))
        .await
        .map_or_else(|error| stale(state, organization, &error), Some)
}

pub async fn sync(state: &AppState, organization: Uuid) -> Option<Arc<OrganizationHub>> {
    state
        .mcp()
        .sync(organization, |known| changed(state, organization, known))
        .await
        .unwrap_or_else(|error| stale(state, organization, &error))
}

pub async fn reconnect(
    state: &AppState,
    organization: Uuid,
    server: Uuid,
) -> Option<Arc<OrganizationHub>> {
    state
        .mcp()
        .reconnect(organization, server, |known| {
            changed(state, organization, known)
        })
        .await
        .unwrap_or_else(|error| stale(state, organization, &error))
}

async fn changed(
    state: &AppState,
    organization: Uuid,
    known: Revisions,
) -> Result<Option<Snapshot>, McpServerError> {
    let revisions = mcp_servers::revisions(state.db(), organization).await?;
    if revisions == known {
        return Ok(None);
    }
    let records =
        mcp_servers::for_organization(state.db(), state.master_key(), organization).await?;
    Ok(Some(Snapshot {
        revisions,
        definitions: records.iter().map(definition).collect(),
    }))
}

fn stale(
    state: &AppState,
    organization: Uuid,
    error: &McpServerError,
) -> Option<Arc<OrganizationHub>> {
    tracing::warn!(
        %organization,
        %error,
        "Could not read the organization's MCP servers; keeping the ones already connected"
    );
    state.mcp().existing_organization(organization)
}

fn server(record: &McpServerRecord) -> McpServer {
    let implied = if record.command.is_some() {
        Transport::Stdio
    } else {
        Transport::Http
    };
    match record.transport.parse().unwrap_or(implied) {
        Transport::Stdio => {
            let mut server = McpServer::command(
                record.command.clone().unwrap_or_default(),
                record.arguments.iter().cloned(),
            );
            server.environment = record.secrets.environment.clone();
            server
        }
        transport => {
            let mut server = McpServer::remote(record.url.clone().unwrap_or_default())
                .with_transport(transport.into());
            server.headers = record.secrets.headers.clone();
            mask(&mut server);
            server
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use abnegate_agent::mcp::McpTransport;
    use abnegate_secret::MasterKey;
    use abnegate_secret::SecretValue;
    use abnegate_secret::encrypt_value;
    use chrono::Utc;

    use super::*;
    use crate::db::mcp_servers::McpServerChanges;
    use crate::db::mcp_servers::Secrets;
    use crate::services::mcp_servers::status::Connection;
    use crate::services::mcp_servers::status::UNCONNECTED;
    use crate::services::mcp_servers::testing;
    use crate::services::mcp_servers::testing::Tenant;
    use crate::services::mcp_servers::testing::stub;
    use crate::state::test_config;
    use crate::state::unreachable_pool;

    fn record(transport: Transport) -> McpServerRecord {
        McpServerRecord {
            id: Uuid::new_v4(),
            organization_id: Uuid::new_v4(),
            name: "docs".to_string(),
            transport: transport.as_str().to_string(),
            command: None,
            arguments: Vec::new(),
            url: None,
            tools: None,
            enabled: true,
            secrets: Secrets {
                environment: BTreeMap::new(),
                headers: BTreeMap::new(),
            },
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn secrets(pairs: &[(&str, &str)]) -> BTreeMap<String, SecretValue> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), SecretValue::from(*value)))
            .collect()
    }

    #[test]
    fn a_stdio_record_does_not_inherit_zones_environment() {
        let mut record = record(Transport::Stdio);
        record.command = Some("uvx".to_string());
        record.arguments = vec!["docs-server".to_string(), "--quiet".to_string()];
        record.tools = Some(vec!["search".to_string()]);
        record.enabled = false;
        record.secrets.environment = secrets(&[("TOKEN", "sealed")]);
        record.secrets.headers = secrets(&[("Authorization", "Bearer ignored")]);

        let definition = definition(&record);

        assert_eq!(definition.id, record.id);
        assert_eq!(definition.revision, record.updated_at);
        assert_eq!(definition.name, "docs");
        assert!(!definition.enabled);
        assert_eq!(definition.tools, Some(vec!["search".to_string()]));
        let server = definition.server;
        assert!(!server.inherit_environment);
        assert_eq!(server.command.as_deref(), Some("uvx"));
        assert_eq!(server.arguments, ["docs-server", "--quiet"]);
        assert_eq!(server.environment, secrets(&[("TOKEN", "sealed")]));
        assert!(server.headers.is_empty());
        assert!(server.url.is_none());
        assert!(server.tools.is_empty());
    }

    #[test]
    fn an_http_record_sends_its_headers() {
        let mut record = record(Transport::Http);
        record.url = Some("http://10.0.0.5:8080/mcp/tenant-7c1d?key=query-4b2e".to_string());
        record.secrets.headers = secrets(&[("Authorization", "Bearer token-9f3a7b")]);
        record.secrets.environment = secrets(&[("IGNORED", "value")]);

        let definition = definition(&record);

        assert!(definition.enabled);
        assert!(definition.tools.is_none());
        let server = definition.server;
        assert_eq!(server.transport, Some(McpTransport::Http));
        assert_eq!(
            server.url.as_deref(),
            Some("http://10.0.0.5:8080/mcp/tenant-7c1d?key=query-4b2e")
        );
        assert_eq!(
            server.headers,
            secrets(&[("Authorization", "Bearer token-9f3a7b")])
        );
        assert!(server.environment.is_empty());
        assert!(server.command.is_none());
        assert!(server.tools.is_empty());
        assert_eq!(
            server.secrets,
            secrets(&[
                ("header credential Authorization", "token-9f3a7b"),
                ("header value Authorization", "Bearer token-9f3a7b"),
                ("path segment 0", "mcp"),
                ("path segment 1", "tenant-7c1d"),
                ("query value 0", "query-4b2e"),
                ("url path", "/mcp/tenant-7c1d"),
                ("url query", "key=query-4b2e"),
            ])
        );
        let endpoint = server
            .endpoint()
            .expect("a stored server holds no reference");
        assert_eq!(
            endpoint.url,
            "http://10.0.0.5:8080/mcp/tenant-7c1d?key=query-4b2e"
        );
        assert_eq!(
            endpoint.headers,
            secrets(&[("Authorization", "Bearer token-9f3a7b")])
        );
    }

    #[test]
    fn a_changed_record_binds_nothing_it_sent_before() {
        let mut record = record(Transport::Http);
        record.url = Some("http://10.0.0.5:8080/mcp/tenant-7c1d?key=query-4b2e".to_string());
        record.secrets.headers = secrets(&[("Authorization", "Bearer token-9f3a7b")]);
        let before = definition(&record).server;

        record.url = Some("http://10.0.0.5:8080/mcp".to_string());
        record.secrets.headers = secrets(&[("X-Key", "key-51c2d9")]);
        let after = definition(&record).server;

        assert_eq!(before.secrets.len(), 7);
        assert_eq!(
            after.secrets,
            secrets(&[
                ("header value X-Key", "key-51c2d9"),
                ("path segment 0", "mcp"),
                ("url path", "/mcp"),
            ])
        );
    }

    #[test]
    fn a_stdio_record_binds_nothing() {
        let mut stdio = record(Transport::Stdio);
        stdio.command = Some("uvx".to_string());
        stdio.arguments = vec!["--token=token-9f3a7b".to_string()];
        stdio.secrets.environment = secrets(&[("TOKEN", "sealed-9f3a7b")]);

        assert!(definition(&stdio).server.secrets.is_empty());
    }

    #[test]
    fn an_sse_record_keeps_its_transport() {
        let mut record = record(Transport::Sse);
        record.url = Some("http://127.0.0.1:9000/sse".to_string());

        let server = definition(&record).server;

        assert_eq!(server.transport, Some(McpTransport::Sse));
        assert_eq!(server.url.as_deref(), Some("http://127.0.0.1:9000/sse"));
    }

    fn delegate() -> Connection {
        Connection::connected(vec!["delegate".to_string()])
    }

    fn renaming(name: &str) -> McpServerChanges {
        McpServerChanges {
            name: Some(name.to_string()),
            ..McpServerChanges::default()
        }
    }

    fn allowing(tool: &str) -> McpServerChanges {
        McpServerChanges {
            tools: Some(Some(vec![tool.to_string()])),
            ..McpServerChanges::default()
        }
    }

    #[tokio::test]
    async fn a_replica_converges_on_what_another_replica_wrote() {
        let tenant = Tenant::new().await;
        let writer = tenant.replica().await;
        let reader = tenant.replica().await;
        let hub = organization(&reader, tenant.organization)
            .await
            .expect("the reader's hub");
        let empty = hub.names();

        let created = tenant.store(&writer, stub("stub")).await;
        sync(&reader, tenant.organization).await;
        let connecting = hub.named("stub");
        organization(&reader, tenant.organization).await;
        let connected = hub.named("stub");
        let session = hub.session("stub").expect("the created server connected");
        let generation = hub.generation("stub");

        sync(&reader, tenant.organization).await;
        let untouched = hub.generation("stub");

        tenant.change(&writer, created.id, allowing("other")).await;
        sync(&reader, tenant.organization).await;
        let narrowed = hub
            .definition("stub")
            .and_then(|definition| definition.tools);
        let kept = hub
            .session("stub")
            .is_some_and(|kept| Arc::ptr_eq(&kept, &session));
        let narrowing = (hub.named("stub"), hub.generation("stub"));

        tenant
            .change(&writer, created.id, renaming("renamed"))
            .await;
        organization(&reader, tenant.organization).await;
        let renamed = (hub.names(), hub.named("stub"), hub.named("renamed"));

        let disabling = McpServerChanges {
            enabled: Some(false),
            ..McpServerChanges::default()
        };
        tenant.change(&writer, created.id, disabling).await;
        sync(&reader, tenant.organization).await;
        let disabled = (hub.named("renamed"), hub.session("renamed").is_none());

        tenant.delete(created.id).await;
        sync(&reader, tenant.organization).await;
        let deleted = hub.names();
        let unbuilt = writer.mcp().organizations();
        tenant.remove().await;

        assert!(empty.is_empty(), "{empty:?}");
        assert_eq!(connecting, Connection::connecting());
        assert_eq!(connected, delegate());
        assert_eq!(untouched, generation, "an unchanged row reconnected");
        assert_eq!(narrowed, Some(vec!["other".to_string()]));
        assert!(kept, "an allowlist change replaced the session");
        assert_eq!(narrowing, (delegate(), generation));
        assert_eq!(
            renamed,
            (
                vec!["renamed".to_string()],
                Connection::failed(UNCONNECTED),
                delegate()
            )
        );
        assert_eq!(disabled, (Connection::disabled(), true));
        assert!(deleted.is_empty(), "{deleted:?}");
        assert_eq!(unbuilt, 0, "the writer never built a hub");
    }

    #[tokio::test]
    async fn a_reconnect_reads_the_row_again() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        let created = tenant.store(&state, stub("stub")).await;
        let unbuilt = reconnect(&state, tenant.organization, created.id)
            .await
            .is_none();
        let hub = organization(&state, tenant.organization)
            .await
            .expect("the hub");
        let session = hub.session("stub").expect("the server connected");

        reconnect(&state, tenant.organization, created.id).await;
        let connecting = hub.named("stub");
        organization(&state, tenant.organization).await;
        let reconnected = hub.named("stub");
        let replaced = hub
            .session("stub")
            .is_some_and(|replaced| !Arc::ptr_eq(&replaced, &session));
        tenant.remove().await;

        assert!(unbuilt, "a reconnect built the organization's hub");
        assert_eq!(connecting, Connection::connecting());
        assert_eq!(reconnected, delegate());
        assert!(replaced, "the session was kept");
    }

    #[tokio::test]
    async fn a_server_whose_secrets_do_not_open_is_read_once_for_each_revision() {
        let tenant = Tenant::new().await;
        let state = tenant.replica().await;
        tenant.store(&state, stub("stub")).await;
        let sealed = tenant.store(&state, stub("sealed")).await;
        let other = MasterKey::generate().expect("a key");
        let foreign = encrypt_value(&SecretValue::from("{}"), &other).expect("an envelope");
        sqlx::query("UPDATE mcp_servers SET secrets = $2 WHERE id = $1")
            .bind(sealed.id)
            .bind(&foreign)
            .execute(&tenant.pool)
            .await
            .expect("the stored secrets are replaced");

        let hub = organization(&state, tenant.organization)
            .await
            .expect("the hub");
        let names = hub.names();
        let known = hub.revisions();
        let unchanged = changed(&state, tenant.organization, hub.revisions())
            .await
            .expect("the rows are read");
        sqlx::query(
            "UPDATE mcp_servers SET updated_at = updated_at + interval '1 microsecond' \
             WHERE id = $1",
        )
        .bind(sealed.id)
        .execute(&tenant.pool)
        .await
        .expect("the stored revision moves");
        let revised = changed(&state, tenant.organization, hub.revisions())
            .await
            .expect("the rows are read");
        tenant.remove().await;

        assert_eq!(names, ["stub"]);
        assert_eq!(known.get(&sealed.id), Some(&sealed.updated_at));
        assert_eq!(
            unchanged, None,
            "a server that did not open was read again before it changed"
        );
        assert_eq!(
            revised.map(|snapshot| snapshot.definitions.len()),
            Some(1),
            "a changed server was not read again"
        );
    }

    #[tokio::test]
    async fn an_unreadable_database_builds_no_hub() {
        let state = testing::state(test_config(), unreachable_pool()).await;
        let organization = Uuid::new_v4();

        let turn = super::organization(&state, organization).await;
        let synced = sync(&state, organization).await;

        assert!(turn.is_none());
        assert!(synced.is_none());
        assert!(state.mcp().existing_organization(organization).is_none());
        assert_eq!(state.mcp().organizations(), 0);
    }
}
