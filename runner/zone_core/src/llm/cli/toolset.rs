use abnegate_agent_cli::McpServer;
use abnegate_secret::SecretValue;

use super::{STARTUP_TIMEOUT, TURN_TIMEOUT};

/// The header a turn's token authenticates Zone's MCP server with.
const AUTHORIZATION: &str = "Authorization";

/// Zone's tools, served to a spawned agent over MCP.
///
/// A coding agent runs its own tool loop and cannot be handed zone's schemas
/// over a completions API, so it reaches them the way it reaches any other MCP
/// server. The calls come back to zone, where the chat's approval policy
/// decides each one before it runs.
#[derive(Debug, Clone)]
pub struct Toolset {
    /// Where zone serves MCP for this turn.
    pub endpoint: String,
    /// The bearer token that authenticates this turn, and only this turn.
    pub token: SecretValue,
    /// Zone's tool names, as zone's registry spells them.
    pub tools: Vec<String>,
}

impl Toolset {
    /// The MCP server name zone's tools arrive under. The agent prefixes every
    /// tool it loads with it, so `read_file` reaches the model as
    /// `mcp__zone__read_file`.
    pub const SERVER: &'static str = "zone";

    /// The variable the server's authorization header refers to, bound to
    /// [`Toolset::token`] as the server's own secret, so the token reaches
    /// neither an argv nor any other server.
    pub const TOKEN_VARIABLE: &'static str = "ZONE_MCP_TOKEN";

    /// The tool an agent asks before running one of its own.
    ///
    /// Served by zone over the same MCP endpoint as every other tool, and named
    /// to the agent on its command line. It is not a workspace tool: nothing in a
    /// prompt teaches it, and it spends none of a turn's call budget, because the
    /// agent asking whether a call may run is not the call.
    pub const PERMISSION: &'static str = "approve_tool_call";

    pub fn new(
        endpoint: impl Into<String>,
        token: impl Into<SecretValue>,
        tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            token: token.into(),
            tools: tools
                .into_iter()
                .map(Into::into)
                .filter(|tool: &String| !tool.trim().is_empty())
                .collect(),
        }
    }

    /// Zone's server as the agent attaches it: required and listed up front,
    /// held to exactly this turn's tools, and waited on for as long as an
    /// approval may take.
    pub fn server(&self) -> McpServer {
        McpServer::remote(self.endpoint.clone())
            .with_header(
                AUTHORIZATION,
                format!("Bearer ${{{}}}", Self::TOKEN_VARIABLE),
            )
            .with_secret(Self::TOKEN_VARIABLE, self.token.clone())
            .with_tools(self.tools.iter().cloned())
            .exclusive()
            .with_tool_timeout(TURN_TIMEOUT)
            .with_startup_timeout(STARTUP_TIMEOUT)
            .require()
            .eager()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "http://127.0.0.1:8421/mcp";
    const TOKEN: &str = "zone-turn-notarealtoken";

    #[test]
    fn a_toolset_keeps_only_the_tools_it_was_named() {
        let toolset = Toolset::new(ENDPOINT, "token", ["read_file", "  ", ""]);

        assert_eq!(toolset.tools, ["read_file"]);
        assert_eq!(toolset.endpoint, ENDPOINT);
        assert_eq!(toolset.token.expose(), "token");
    }

    #[test]
    fn the_permission_tool_keeps_its_wire_name() {
        assert_eq!(Toolset::PERMISSION, "approve_tool_call");
    }

    #[test]
    fn debug_never_prints_the_turn_token() {
        let toolset = Toolset::new(ENDPOINT, TOKEN, ["read_file"]);

        for rendered in [format!("{toolset:?}"), format!("{:?}", toolset.server())] {
            assert!(!rendered.contains(TOKEN), "turn token leaked: {rendered}");
            assert!(rendered.contains(ENDPOINT));
        }
    }

    #[test]
    fn the_server_names_the_token_rather_than_carrying_it_in_its_header() {
        let server = Toolset::new(ENDPOINT, TOKEN, ["read_file"]).server();

        assert_eq!(server.url.as_deref(), Some(ENDPOINT));
        assert_eq!(
            server
                .headers
                .get(AUTHORIZATION)
                .map(|value| value.expose().to_string()),
            Some("Bearer ${ZONE_MCP_TOKEN}".to_string())
        );
        assert_eq!(
            server
                .secrets
                .get(Toolset::TOKEN_VARIABLE)
                .map(|value| value.expose().to_string()),
            Some(TOKEN.to_string())
        );
        assert_eq!(server.tools, ["read_file"]);
    }
}
