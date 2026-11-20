//! What an organization's agent policy lets a tool set reach.
//!
//! A policy that restricts the network turns a chat or run offline, the way a
//! chat that opts out of the public web is, and then admits back only what it
//! can hold to its domains: `fetch_url`, which checks every host it is sent
//! to, and MCP servers reached over HTTP at an admitted host.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::RwLock;

use abnegate_agent::ToolRegistry;
use abnegate_agent::mcp;
use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::McpTransport;
use reqwest::Url;
use zone_enterprise::policy::Denial;
use zone_enterprise::policy::Effective;

/// Stands in for a tool's name when only its server's part is wanted.
const PROBE: &str = "tool";

/// Told of every request a policy refused at run time: the denial, and the
/// host or server it refused.
pub type Refusing = dyn Fn(Denial, &str) + Send + Sync;

/// Shared by the tool set and every tool that refuses, so a hook set after
/// assembly reaches them all.
#[derive(Clone, Default)]
pub struct Refusals(Arc<RwLock<Option<Arc<Refusing>>>>);

impl Refusals {
    pub fn set(&self, refusing: Arc<Refusing>) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Some(refusing);
    }

    pub fn current(&self) -> Option<Arc<Refusing>> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn record(&self, denial: Denial, subject: &str) {
        if let Some(refusing) = self.current() {
            refusing(denial, subject);
        }
    }
}

/// Under a network restriction a server is admitted only when it is reached
/// over HTTP at an admitted host: a command server reaches whatever its
/// process does, and a server whose definition is unknown here cannot be
/// placed at all.
pub fn admits(policy: &Effective, name: &str, server: Option<&McpServer>) -> bool {
    if policy.server(name).is_err() {
        return false;
    }
    if !policy.restricts_network() {
        return true;
    }
    server
        .filter(|server| {
            server.command.is_none() && matches!(server.transport, None | Some(McpTransport::Http))
        })
        .and_then(McpServer::endpoint)
        .and_then(|endpoint| Url::parse(&endpoint.url).ok())
        .is_some_and(|url| {
            url.host_str()
                .is_some_and(|host| policy.domain(host).is_ok())
        })
}

pub fn withhold(
    registry: &mut ToolRegistry,
    before: &HashSet<String>,
    servers: &[String],
) -> usize {
    let identifiers: HashSet<String> = servers.iter().map(|server| identifier(server)).collect();
    let withheld: Vec<String> = registry
        .names()
        .into_iter()
        .filter(|name| !before.contains(*name))
        .filter(|name| {
            name.split_once(mcp::SEPARATOR)
                .is_some_and(|(server, _)| identifiers.contains(server))
        })
        .map(str::to_string)
        .collect();
    for name in &withheld {
        registry.remove(name);
    }
    withheld.len()
}

/// Holds no separator itself, so a tool's name splits at its first.
fn identifier(server: &str) -> String {
    let qualified = mcp::qualified_tool_name(server, PROBE);
    match qualified.split_once(mcp::SEPARATOR) {
        Some((identifier, _)) => identifier.to_string(),
        None => qualified,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use zone_enterprise::policy::Rules;

    use super::*;

    fn policy(mcp_servers: Option<&[&str]>, domains: Option<&[&str]>) -> Effective {
        let owned = |entries: &[&str]| entries.iter().map(|entry| entry.to_string()).collect();
        Effective::new(
            Rules {
                mcp_servers: mcp_servers.map(owned),
                domains: domains.map(owned),
                ..Rules::default()
            },
            Rules::default(),
        )
    }

    #[test]
    fn a_server_outside_the_list_is_refused_whatever_the_network() {
        let remote = McpServer::remote("https://mcp.example.com/mcp");
        let named = policy(Some(&["docs"]), None);
        assert!(admits(&named, "docs", Some(&remote)));
        assert!(!admits(&named, "other", Some(&remote)));
        assert!(admits(&policy(None, None), "other", None));
    }

    #[test]
    fn under_a_network_restriction_only_an_http_server_at_an_admitted_host_is_admitted() {
        let restricted = policy(None, Some(&["example.com"]));
        for (url, admitted) in [
            ("https://mcp.example.com/mcp", true),
            ("https://example.com:8443/mcp", true),
            ("https://evil.test/mcp", false),
            ("https://example.com.evil.test/mcp", false),
        ] {
            assert_eq!(
                admits(&restricted, "docs", Some(&McpServer::remote(url))),
                admitted,
                "{url}"
            );
        }
        let command = McpServer::command("python3", ["server.py"]);
        assert!(!admits(&restricted, "docs", Some(&command)));
        assert!(!admits(&restricted, "docs", None));
        let events =
            McpServer::remote("https://mcp.example.com/sse").with_transport(McpTransport::Sse);
        assert!(!admits(&restricted, "docs", Some(&events)));
    }

    #[test]
    fn a_hook_set_after_cloning_reaches_every_clone() {
        let refusals = Refusals::default();
        let held = refusals.clone();
        held.record(Denial::Domain, "unrecorded.test");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        refusals.set(Arc::new(move |denial, subject| {
            sink.lock()
                .expect("the sink is readable")
                .push((denial, subject.to_string()));
        }));
        held.record(Denial::Domain, "evil.test");
        assert_eq!(
            *seen.lock().expect("the sink is readable"),
            [(Denial::Domain, "evil.test".to_string())]
        );
    }

    #[test]
    fn a_server_is_known_by_the_part_its_tools_are_named_under() {
        assert_eq!(identifier("docs"), "docs");
        assert_eq!(identifier("my__server_"), "my_server");
        assert_eq!(identifier("a.b"), "a_b");
    }
}
