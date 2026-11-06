//! Shell a stand-in agent reads what Zone handed it with, as abnegate-agent-cli hands it over.

/// Prints the bearer token the agent was handed for Zone's MCP server. It arrives as the whole
/// `Authorization` header, in a variable abnegate-agent-cli generates and the agent's MCP
/// configuration names.
pub const MCP_TOKEN: &str =
    "$(env | sed -n 's/^ABNEGATE_MCP_[A-Za-z0-9_]*=Bearer //p' | head -n 1)";

/// Prints the MCP configuration file the agent was pointed at with `--mcp-config`, without its
/// whitespace, so it reads as one compact definition.
pub const MCP_CONFIGURATION: &str = "previous=; for argument in \"$@\"; do if [ \"$previous\" = \
     --mcp-config ]; then tr -d ' \\n' < \"$argument\"; fi; previous=$argument; done";
