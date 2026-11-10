use abnegate_agent::mcp::McpServer;
use uuid::Uuid;

use crate::db::mcp_servers::{self, McpServerRecord};
use crate::state::AppState;

use super::definition::Definition;
use super::transport::Transport;

pub fn definition(record: &McpServerRecord) -> Definition {
    Definition {
        id: Some(record.id),
        name: record.name.clone(),
        server: server(record),
        enabled: record.enabled,
        tools: record.tools.clone(),
    }
}

pub async fn definitions(state: &AppState, organization: Uuid) -> Vec<Definition> {
    match mcp_servers::for_organization(state.db(), state.master_key(), organization).await {
        Ok(records) => records.iter().map(definition).collect(),
        Err(error) => {
            tracing::warn!(%organization, error = %error, "Could not load the organization's MCP servers");
            Vec::new()
        }
    }
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
            server
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use abnegate_agent::mcp::McpTransport;
    use abnegate_secret::SecretValue;
    use chrono::Utc;

    use super::*;
    use crate::db::mcp_servers::Secrets;

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
    fn a_stdio_record_starts_from_its_own_environment_only() {
        let mut record = record(Transport::Stdio);
        record.command = Some("uvx".to_string());
        record.arguments = vec!["docs-server".to_string(), "--quiet".to_string()];
        record.tools = Some(vec!["search".to_string()]);
        record.enabled = false;
        record.secrets.environment = secrets(&[("TOKEN", "sealed")]);
        record.secrets.headers = secrets(&[("Authorization", "Bearer ignored")]);

        let definition = definition(&record);

        assert_eq!(definition.id, Some(record.id));
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
        record.url = Some("http://10.0.0.5:8080/mcp".to_string());
        record.secrets.headers = secrets(&[("Authorization", "Bearer token")]);
        record.secrets.environment = secrets(&[("IGNORED", "value")]);

        let definition = definition(&record);

        assert!(definition.enabled);
        assert!(definition.tools.is_none());
        let server = definition.server;
        assert_eq!(server.transport, Some(McpTransport::Http));
        assert_eq!(server.url.as_deref(), Some("http://10.0.0.5:8080/mcp"));
        assert_eq!(
            server.headers,
            secrets(&[("Authorization", "Bearer token")])
        );
        assert!(server.environment.is_empty());
        assert!(server.command.is_none());
        assert!(server.tools.is_empty());
    }

    #[test]
    fn an_sse_record_keeps_its_transport() {
        let mut record = record(Transport::Sse);
        record.url = Some("http://127.0.0.1:9000/sse".to_string());

        let server = definition(&record).server;

        assert_eq!(server.transport, Some(McpTransport::Sse));
        assert_eq!(server.url.as_deref(), Some("http://127.0.0.1:9000/sse"));
    }
}
