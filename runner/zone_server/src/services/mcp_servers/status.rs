use abnegate_agent::mcp::McpFailure;
use abnegate_agent::mcp::McpTransport;
use axum::http::StatusCode;
use serde::Serialize;

use super::definition::Definition;

pub const MCP_DISABLED: &str = "MCP servers are turned off on this instance (ZONE_MCP_ENABLED)";
pub const COMMANDS_DISABLED: &str =
    "Command servers are turned off on this instance (ZONE_MCP_COMMANDS)";
pub const SSE_UNSUPPORTED: &str = "SSE servers are not supported; use streamable HTTP (`http`)";
pub const UNCONNECTED: &str = "did not connect; see the server log";
pub const INVALID: &str = "The server's definition is not valid";
pub const UNRESOLVED: &str = "The URL or a header refers to a variable that is not set";
pub const INVALID_URL: &str = "The URL is not valid";
pub const METADATA_ADDRESS: &str = "The URL points at a link-local or cloud metadata address";
pub const INVALID_HEADER: &str = "A header is not valid";
pub const CLIENT_UNBUILT: &str = "Zone could not build an HTTP client for the server";
pub const TIMED_OUT: &str = "The server did not answer in time";
pub const REDIRECTED: &str = "The server answered with a redirect Zone does not follow";
pub const UNAUTHORIZED: &str = "The server refused the credentials (HTTP 401)";
pub const FORBIDDEN: &str = "The server refused the credentials (HTTP 403)";
pub const ANSWERED: &str = "The server answered HTTP";
pub const NOT_MCP: &str = "The server's answer is not MCP over streamable HTTP";
pub const REJECTED: &str = "The server answered the handshake with an error";
pub const UNREACHABLE: &str = "Zone could not reach the server";
pub const INCOMPLETE: &str = "The server did not complete the handshake";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Connected,
    Failed,
    Disabled,
    Connecting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub status: Status,
    pub error: Option<String>,
    pub tools: Vec<String>,
}

impl Connection {
    pub fn connected(tools: Vec<String>) -> Self {
        Self {
            status: Status::Connected,
            error: None,
            tools,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: Status::Failed,
            error: Some(error.into()),
            tools: Vec::new(),
        }
    }

    pub fn disabled() -> Self {
        Self::quiet(Status::Disabled)
    }

    pub fn connecting() -> Self {
        Self::quiet(Status::Connecting)
    }

    fn quiet(status: Status) -> Self {
        Self {
            status,
            error: None,
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub enabled: bool,
    pub commands: bool,
}

pub fn static_status(definition: &Definition, policy: Policy) -> Option<Connection> {
    if !policy.enabled {
        return Some(Connection::failed(MCP_DISABLED));
    }
    let server = &definition.server;
    if !definition.enabled || server.disabled {
        return Some(Connection::disabled());
    }
    if server.command.is_some() {
        if !policy.commands {
            return Some(Connection::failed(COMMANDS_DISABLED));
        }
    } else if server.transport == Some(McpTransport::Sse) {
        return Some(Connection::failed(SSE_UNSUPPORTED));
    }
    (!server.valid()).then(|| Connection::failed(UNCONNECTED))
}

/// What a tenant is told about a server reached by URL that did not connect: the sentence for
/// the kind of failure the hub recorded and, for an HTTP status no other kind names, that status.
pub fn summary(failure: McpFailure, status: Option<u16>) -> String {
    let sentence = match (failure, status) {
        (McpFailure::Status, Some(status)) => return format!("{ANSWERED} {status}"),
        (McpFailure::Invalid, _) => INVALID,
        (McpFailure::Unsupported, _) => SSE_UNSUPPORTED,
        (McpFailure::Unresolved, _) => UNRESOLVED,
        (McpFailure::Address, _) => INVALID_URL,
        (McpFailure::Metadata, _) => METADATA_ADDRESS,
        (McpFailure::Header, _) => INVALID_HEADER,
        (McpFailure::Client, _) => CLIENT_UNBUILT,
        (McpFailure::Timeout, _) => TIMED_OUT,
        (McpFailure::Redirect, _) => REDIRECTED,
        (McpFailure::Unauthorized, Some(status)) if StatusCode::FORBIDDEN == status => FORBIDDEN,
        (McpFailure::Unauthorized, _) => UNAUTHORIZED,
        (McpFailure::Protocol, _) => NOT_MCP,
        (McpFailure::Rejected, _) => REJECTED,
        (McpFailure::Unreachable, _) => UNREACHABLE,
        (McpFailure::Closed | McpFailure::Oversized, _) => INCOMPLETE,
        _ => UNCONNECTED,
    };
    sentence.to_string()
}

#[cfg(test)]
mod tests {
    use abnegate_agent::mcp::McpConfig;
    use abnegate_agent::mcp::McpHub;
    use abnegate_agent::mcp::McpOutcome;
    use abnegate_agent::mcp::McpServer;
    use abnegate_agent::tool::Proxy;
    use zone_core::servers::mask;

    use super::super::stub::Stub;
    use super::*;

    const COMMANDS: Policy = Policy {
        enabled: true,
        commands: true,
    };
    const REMOTE: Policy = Policy {
        enabled: true,
        commands: false,
    };
    const OFF: Policy = Policy {
        enabled: false,
        commands: true,
    };

    const NAME: &str = "docs";
    const STATUS_DIGITS: usize = 3;
    const KEY_FLAG: &str = "--key";
    const KEY_HEADER: &str = "X-Key";
    const STUB_KEY: &str = "the-key-the-stub-wants";
    const WRONG_KEY: &str = "knock-knock-knock-knock";
    const TOKEN_FLAG: &str = "--token";
    const STUB_TOKEN: &str = "stub-token-51c2d9";
    const AUTHORIZATION: &str = "Authorization";
    const WRONG_TOKEN: &str = "Bearer wrong-token-8e21c4";
    const SENTENCES: [&str; 16] = [
        SSE_UNSUPPORTED,
        UNCONNECTED,
        INVALID,
        UNRESOLVED,
        INVALID_URL,
        METADATA_ADDRESS,
        INVALID_HEADER,
        CLIENT_UNBUILT,
        TIMED_OUT,
        REDIRECTED,
        UNAUTHORIZED,
        FORBIDDEN,
        NOT_MCP,
        REJECTED,
        UNREACHABLE,
        INCOMPLETE,
    ];
    const STATUSES: [Option<u16>; 6] =
        [None, Some(100), Some(401), Some(403), Some(500), Some(999)];

    fn definition(server: McpServer) -> Definition {
        Definition::of("docs", server)
    }

    fn answered(status: u16) -> String {
        format!("The server answered HTTP {status}")
    }

    fn kinds() -> Vec<(McpFailure, Option<u16>, String)> {
        let fixed = |failure: McpFailure, status: Option<u16>, sentence: &str| {
            (failure, status, sentence.to_string())
        };
        vec![
            fixed(
                McpFailure::Invalid,
                None,
                "The server's definition is not valid",
            ),
            fixed(
                McpFailure::Unsupported,
                None,
                "SSE servers are not supported; use streamable HTTP (`http`)",
            ),
            fixed(
                McpFailure::Unresolved,
                None,
                "The URL or a header refers to a variable that is not set",
            ),
            fixed(McpFailure::Address, None, "The URL is not valid"),
            fixed(
                McpFailure::Metadata,
                None,
                "The URL points at a link-local or cloud metadata address",
            ),
            fixed(McpFailure::Header, None, "A header is not valid"),
            fixed(
                McpFailure::Client,
                None,
                "Zone could not build an HTTP client for the server",
            ),
            fixed(
                McpFailure::Timeout,
                None,
                "The server did not answer in time",
            ),
            fixed(
                McpFailure::Redirect,
                None,
                "The server answered with a redirect Zone does not follow",
            ),
            fixed(
                McpFailure::Redirect,
                Some(302),
                "The server answered with a redirect Zone does not follow",
            ),
            fixed(
                McpFailure::Unauthorized,
                Some(401),
                "The server refused the credentials (HTTP 401)",
            ),
            fixed(
                McpFailure::Unauthorized,
                Some(403),
                "The server refused the credentials (HTTP 403)",
            ),
            fixed(
                McpFailure::Unauthorized,
                None,
                "The server refused the credentials (HTTP 401)",
            ),
            (McpFailure::Status, Some(400), answered(400)),
            (McpFailure::Status, Some(401), answered(401)),
            (McpFailure::Status, Some(403), answered(403)),
            (McpFailure::Status, Some(404), answered(404)),
            (McpFailure::Status, Some(429), answered(429)),
            (McpFailure::Status, Some(500), answered(500)),
            fixed(
                McpFailure::Status,
                None,
                "did not connect; see the server log",
            ),
            fixed(
                McpFailure::Protocol,
                None,
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                McpFailure::Protocol,
                Some(502),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                McpFailure::Rejected,
                None,
                "The server answered the handshake with an error",
            ),
            fixed(
                McpFailure::Rejected,
                Some(400),
                "The server answered the handshake with an error",
            ),
            fixed(
                McpFailure::Unreachable,
                None,
                "Zone could not reach the server",
            ),
            fixed(
                McpFailure::Spawn,
                None,
                "did not connect; see the server log",
            ),
            fixed(
                McpFailure::Oversized,
                Some(400),
                "The server did not complete the handshake",
            ),
            fixed(
                McpFailure::Closed,
                None,
                "The server did not complete the handshake",
            ),
            fixed(
                McpFailure::Other,
                None,
                "did not connect; see the server log",
            ),
        ]
    }

    fn failure(hub: &McpHub, name: &str) -> (McpFailure, Option<u16>, String) {
        match hub.outcome(name) {
            Some(McpOutcome::Failed {
                failure,
                status,
                error,
                ..
            }) => (failure, status, error),
            outcome => panic!("{name} did not fail: {outcome:?}"),
        }
    }

    fn told(hub: &McpHub, name: &str) -> String {
        let (failure, status, _) = failure(hub, name);
        summary(failure, status)
    }

    #[test]
    fn the_master_switch_fails_every_definition_before_anything_else() {
        let stdio = definition(McpServer::command("python3", ["stub.py"]));
        let http = definition(McpServer::remote("http://127.0.0.1/mcp"));
        let mut disabled = definition(McpServer::command("python3", ["stub.py"]));
        disabled.enabled = false;

        for definition in [stdio, http, disabled] {
            assert_eq!(
                static_status(&definition, OFF),
                Some(Connection::failed(MCP_DISABLED)),
                "{definition:?}"
            );
        }
    }

    #[test]
    fn statuses_serialize_as_the_frontend_reads_them() {
        assert_eq!(
            serde_json::to_value([
                Status::Connected,
                Status::Failed,
                Status::Disabled,
                Status::Connecting
            ])
            .unwrap(),
            serde_json::json!(["connected", "failed", "disabled", "connecting"])
        );
        assert_eq!(
            serde_json::to_value(Connection::failed(UNCONNECTED)).unwrap(),
            serde_json::json!({"status": "failed", "error": UNCONNECTED, "tools": []})
        );
    }

    #[test]
    fn a_disabled_definition_is_disabled_whatever_its_server() {
        let mut stdio = definition(McpServer::command("python3", ["stub.py"]));
        stdio.enabled = false;
        assert_eq!(static_status(&stdio, REMOTE), Some(Connection::disabled()));

        let mut sse =
            definition(McpServer::remote("http://127.0.0.1/sse").with_transport(McpTransport::Sse));
        sse.enabled = false;
        assert_eq!(static_status(&sse, COMMANDS), Some(Connection::disabled()));
    }

    #[test]
    fn a_command_server_needs_commands_turned_on() {
        let stdio = definition(McpServer::command("python3", ["stub.py"]));
        assert_eq!(
            static_status(&stdio, REMOTE),
            Some(Connection::failed(COMMANDS_DISABLED))
        );
        assert_eq!(static_status(&stdio, COMMANDS), None);
    }

    #[test]
    fn an_http_server_is_connectable_and_an_sse_one_is_refused() {
        let sse =
            definition(McpServer::remote("http://127.0.0.1/sse").with_transport(McpTransport::Sse));
        let http = definition(McpServer::remote("http://127.0.0.1/mcp"));
        let typed = definition(
            McpServer::remote("http://127.0.0.1/mcp").with_transport(McpTransport::Http),
        );
        let mut disabled = http.clone();
        disabled.enabled = false;

        for policy in [COMMANDS, REMOTE] {
            assert_eq!(
                static_status(&sse, policy),
                Some(Connection::failed(SSE_UNSUPPORTED))
            );
            assert_eq!(static_status(&http, policy), None);
            assert_eq!(static_status(&typed, policy), None);
            assert_eq!(
                static_status(&disabled, policy),
                Some(Connection::disabled())
            );
        }
    }

    #[test]
    fn an_invalid_server_is_unconnected() {
        let blank = definition(McpServer::command(" ", Vec::<String>::new()));
        assert_eq!(
            static_status(&blank, COMMANDS),
            Some(Connection::failed(UNCONNECTED))
        );
    }

    #[test]
    fn each_kind_reads_as_its_sentence() {
        for (failure, status, sentence) in kinds() {
            assert_eq!(summary(failure, status), sentence, "{failure:?} {status:?}");
        }
    }

    #[test]
    fn a_summary_is_one_line_of_zones_own_words() {
        for (failure, status) in kinds()
            .into_iter()
            .flat_map(|(failure, _, _)| STATUSES.map(|status| (failure, status)))
        {
            let summary = summary(failure, status);
            let answered = summary
                .strip_prefix(ANSWERED)
                .and_then(|status| status.strip_prefix(' '))
                .is_some_and(|status| {
                    status.len() == STATUS_DIGITS
                        && status.bytes().all(|digit| digit.is_ascii_digit())
                });
            assert!(
                SENTENCES.contains(&summary.as_str()) || answered,
                "{failure:?} {status:?} became {summary}"
            );
            assert!(!summary.contains('\n'), "{summary}");
        }
    }

    #[tokio::test]
    async fn the_hub_names_its_refusals_by_the_kinds_summary_reads() {
        let configuration = McpConfig::default()
            .with_server(
                "events",
                McpServer::remote("http://127.0.0.1:1/sse").with_transport(McpTransport::Sse),
            )
            .with_server(
                "mismatched",
                McpServer::remote("http://127.0.0.1:1/mcp").with_transport(McpTransport::Stdio),
            )
            .with_server(
                "unresolved",
                McpServer::remote("http://127.0.0.1:1/${ZONE_TEST_MCP_UNSET}"),
            )
            .with_server("unparsed", McpServer::remote("not a url"))
            .with_server("schemed", McpServer::remote("ftp://127.0.0.1/mcp"))
            .with_server("metadata", McpServer::remote("http://169.254.169.254/mcp"))
            .with_server(
                "headed",
                McpServer::remote("http://127.0.0.1:1/mcp").with_header("Bad Header", "value"),
            );
        let proxied = McpConfig::default()
            .with_server("proxied", McpServer::remote("http://127.0.0.1:1/mcp"));

        let hub = McpHub::connect_through(&configuration, &Proxy::default()).await;
        let unrouted = McpHub::connect_through(&proxied, &Proxy::new("not a url")).await;

        for (name, kind, sentence) in [
            ("events", McpFailure::Unsupported, SSE_UNSUPPORTED),
            ("mismatched", McpFailure::Invalid, INVALID),
            ("unresolved", McpFailure::Unresolved, UNRESOLVED),
            ("unparsed", McpFailure::Address, INVALID_URL),
            ("schemed", McpFailure::Address, INVALID_URL),
            ("metadata", McpFailure::Metadata, METADATA_ADDRESS),
            ("headed", McpFailure::Header, INVALID_HEADER),
        ] {
            let (failure, status, _) = failure(&hub, name);
            assert_eq!((failure, status), (kind, None), "{name}");
            assert_eq!(told(&hub, name), sentence, "{name}");
        }
        let (failure, status, _) = failure(&unrouted, "proxied");
        assert_eq!((failure, status), (McpFailure::Client, None));
        assert_eq!(told(&unrouted, "proxied"), CLIENT_UNBUILT);
    }

    /// The stub answers a request without its key with a 400 whose body repeats the key it was
    /// sent and the path it was sent to. The hub keeps that answer's text, scrubbed, for the
    /// server log; what a tenant is told holds nothing the server was sent or said.
    #[tokio::test]
    async fn a_refusal_that_repeats_what_it_was_sent_reaches_a_tenant_as_its_status() {
        let stub = Stub::start(&[KEY_FLAG, STUB_KEY]).await;
        let origin = format!("http://127.0.0.1:{}", stub.port());
        let rows = [
            ("long", "tenants-acme-7d1", WRONG_KEY),
            ("short", "zq", "kx9"),
        ];
        let configuration = rows.iter().fold(
            McpConfig::default(),
            |configuration, (name, segment, key)| {
                let mut server = McpServer::remote(format!("{origin}/{segment}/mcp?tenant={key}"))
                    .with_header(KEY_HEADER, *key);
                mask(&mut server);
                configuration.with_server(*name, server)
            },
        );

        let hub = McpHub::connect_through(&configuration, &Proxy::default()).await;

        for (name, segment, key) in rows {
            let (failure, status, error) = failure(&hub, name);
            let sentence = summary(failure, status);

            assert_eq!(
                (failure, status),
                (McpFailure::Status, Some(400)),
                "{error}"
            );
            assert_eq!(sentence, answered(400), "{name}");
            for sent in [segment, key, "tenant"] {
                assert!(!sentence.contains(sent), "{name} told {sentence}");
            }
        }
    }

    #[tokio::test]
    async fn refused_credentials_reach_a_tenant_as_the_kind_alone() {
        let stub = Stub::start(&[TOKEN_FLAG, STUB_TOKEN]).await;
        let mut server = McpServer::remote(stub.url()).with_header(AUTHORIZATION, WRONG_TOKEN);
        mask(&mut server);
        let configuration = McpConfig::default().with_server(NAME, server);

        let hub = McpHub::connect_through(&configuration, &Proxy::default()).await;

        let (failure, status, error) = failure(&hub, NAME);
        assert_eq!(
            (failure, status),
            (McpFailure::Unauthorized, Some(401)),
            "{error}"
        );
        let sentence = told(&hub, NAME);
        assert_eq!(sentence, UNAUTHORIZED);
        assert!(!sentence.contains(WRONG_TOKEN) && !sentence.contains("unauthorized"));
    }
}
