use abnegate_agent::mcp::McpServer;
#[cfg(test)]
use abnegate_secret::SecretValue;
use uuid::Uuid;

#[cfg(test)]
const MARKER: &str = "ZONE_STUB_MARKER";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub id: Option<Uuid>,
    pub name: String,
    pub server: McpServer,
    pub enabled: bool,
    pub tools: Option<Vec<String>>,
}

#[cfg(test)]
impl Definition {
    pub(crate) fn of(name: &str, server: McpServer) -> Self {
        Self {
            id: Some(Uuid::new_v4()),
            name: name.to_string(),
            server,
            enabled: true,
            tools: None,
        }
    }

    pub(crate) fn stub(name: &str) -> Self {
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stub_mcp_server.py");
        Self::of(
            name,
            McpServer::command("python3", [script.to_string_lossy().into_owned()]),
        )
    }

    pub(crate) fn stub_marked(name: &str, marker: &str) -> Self {
        let mut definition = Self::stub(name);
        definition
            .server
            .environment
            .insert(MARKER.to_string(), SecretValue::from(marker));
        definition
    }

    pub(crate) fn marker(&self) -> Option<&str> {
        self.server.environment.get(MARKER).map(SecretValue::expose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_never_reaches_the_stubs_arguments() {
        let plain = Definition::stub("stub");
        let marked = Definition::stub_marked("stub", "first");

        assert_eq!(plain.server.arguments.len(), 1);
        assert_eq!(marked.server.arguments, plain.server.arguments);
        assert_eq!(plain.marker(), None);
        assert_eq!(marked.marker(), Some("first"));
        assert_ne!(marked.server, plain.server);
    }
}
