use abnegate_agent::mcp::McpServer;
use uuid::Uuid;

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
        Self::stub_marked(name, &[])
    }

    pub(crate) fn stub_marked(name: &str, markers: &[&str]) -> Self {
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stub_mcp_server.py");
        let arguments = std::iter::once(script.to_string_lossy().into_owned())
            .chain(markers.iter().map(|marker| marker.to_string()));
        Self::of(name, McpServer::command("python3", arguments))
    }
}
