use std::collections::BTreeMap;

use abnegate_agent::mcp::McpServer;
#[cfg(test)]
use abnegate_secret::SecretValue;
use chrono::DateTime;
#[cfg(test)]
use chrono::Duration;
use chrono::Utc;
use uuid::Uuid;

#[cfg(test)]
const MARKER: &str = "ZONE_STUB_MARKER";

pub type Revisions = BTreeMap<Uuid, DateTime<Utc>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub id: Uuid,
    pub revision: DateTime<Utc>,
    pub name: String,
    pub server: McpServer,
    pub enabled: bool,
    pub tools: Option<Vec<String>>,
}

/// One reading of what an organization stores: the revision of every row, and a definition for
/// each row that could be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub revisions: Revisions,
    pub definitions: Vec<Definition>,
}

#[cfg(test)]
impl From<Vec<Definition>> for Snapshot {
    fn from(definitions: Vec<Definition>) -> Self {
        Self {
            revisions: definitions
                .iter()
                .map(|definition| (definition.id, definition.revision))
                .collect(),
            definitions,
        }
    }
}

#[cfg(test)]
impl Definition {
    pub(crate) fn of(name: &str, server: McpServer) -> Self {
        Self {
            id: Uuid::new_v4(),
            revision: Utc::now(),
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
        Self::stub(name).marked(marker)
    }

    pub(crate) fn marked(mut self, marker: &str) -> Self {
        self.server
            .environment
            .insert(MARKER.to_string(), SecretValue::from(marker));
        self
    }

    pub(crate) fn marker(&self) -> Option<&str> {
        self.server.environment.get(MARKER).map(SecretValue::expose)
    }

    pub(crate) fn revised(mut self) -> Self {
        self.revision += Duration::microseconds(1);
        self
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

    #[test]
    fn a_revised_definition_keeps_its_identity() {
        let stored = Definition::stub("stub");

        let revised = stored.clone().revised();

        assert_eq!(revised.id, stored.id);
        assert_ne!(revised.revision, stored.revision);
        assert_eq!(revised.server, stored.server);
    }
}
