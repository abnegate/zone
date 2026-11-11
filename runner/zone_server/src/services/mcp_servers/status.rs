use std::ops::RangeInclusive;

use abnegate_agent::mcp::McpTransport;
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

const HANDSHAKE: &str = "MCP handshake failed for";
const TRANSPORT: &str = "MCP transport failed for";
const WRAPPERS: [&str; 2] = [
    "Send message error Transport [",
    "Transport send error: Transport [",
];
const WRAPPED: &str = "] error: ";
const STATUS_DIGITS: usize = 3;
const STATUSES: RangeInclusive<u16> = 100..=999;

const REFUSALS: [(&str, Failure); 2] = [
    (
        "SSE servers are not supported; use streamable HTTP",
        Failure::Unsupported,
    ),
    (
        "set exactly one of `command` and `url`, and a `type`, if any, of `stdio`, `http` or `sse` that matches it",
        Failure::Invalid,
    ),
];

const TRANSPORTS: [Lead; 6] = [
    Lead::Fixed(
        "its URL or headers do not resolve against the server's bound secrets",
        Failure::Unresolved,
    ),
    Lead::Fixed("invalid URL", Failure::Address),
    Lead::Fixed(
        "refusing a link-local or cloud metadata address",
        Failure::Metadata,
    ),
    Lead::Fixed("invalid header `", Failure::Header),
    Lead::Fixed("invalid proxy URL", Failure::Client),
    Lead::Fixed("could not build an HTTP client", Failure::Client),
];

const HANDSHAKES: [Lead; 20] = [
    Lead::Fixed("handshake timed out", Failure::Timeout),
    Lead::Fixed("request timeout after ", Failure::Timeout),
    Lead::Fixed("Auth required", Failure::Unauthorized),
    Lead::Fixed("Insufficient scope", Failure::Forbidden),
    Lead::Status("HTTP "),
    Lead::Status("unexpected server response: HTTP "),
    Lead::Status("Session expired (HTTP "),
    Lead::Fixed(
        "unexpected server response: refused a redirect to ",
        Failure::Redirect,
    ),
    Lead::Fixed(
        "unexpected server response: refused more than ",
        Failure::Redirect,
    ),
    Lead::Fixed("unexpected server response: ", Failure::Protocol),
    Lead::Fixed("Unexpected content type: ", Failure::Protocol),
    Lead::Fixed("Unexpected response type", Failure::Protocol),
    Lead::Fixed("Deserialize error: ", Failure::Protocol),
    Lead::Fixed("expect initialized ", Failure::Protocol),
    Lead::Fixed("conflict initialized response id: ", Failure::Protocol),
    Lead::Fixed("uncorrelated error response: ", Failure::Protocol),
    Lead::Fixed("no compatible protocol version ", Failure::Protocol),
    Lead::Fixed("JSON-RPC error: ", Failure::Rejected),
    Lead::Fixed("Mcp error: ", Failure::Rejected),
    Lead::Fixed("Client error: ", Failure::Unreachable),
];

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

/// What a tenant is told about a server reached by URL that did not connect: one of this
/// module's own sentences and, where the server answered with one, its HTTP status. `error` is
/// the hub's failure text for the server it knows as `name`.
///
/// Nothing else of `error` is ever copied. The client reads up to 64 KiB of a refusal's body
/// into that text and private hosts are reachable on purpose, so the text itself would hand an
/// organization's administrator whatever any host this server can reach says back. The
/// sentence is chosen by the words the hub and its client open the text with, which precede
/// anything a remote supplied; the rest is never searched, so a remote cannot choose it.
pub fn summary(name: &str, error: &str) -> String {
    Failure::of(name, error).sentence()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Unsupported,
    Invalid,
    Unresolved,
    Address,
    Metadata,
    Header,
    Client,
    Timeout,
    Redirect,
    Unauthorized,
    Forbidden,
    Status(u16),
    Protocol,
    Rejected,
    Unreachable,
    Incomplete,
    Unknown,
}

impl Failure {
    fn of(name: &str, error: &str) -> Self {
        if let Some(message) = staged(HANDSHAKE, name, error) {
            return Lead::longest(&HANDSHAKES, unwrapped(message)).unwrap_or(Self::Incomplete);
        }
        if let Some(message) = staged(TRANSPORT, name, error) {
            return Lead::longest(&TRANSPORTS, message).unwrap_or(Self::Unknown);
        }
        REFUSALS
            .iter()
            .find(|(text, _)| *text == error)
            .map_or(Self::Unknown, |(_, failure)| *failure)
    }

    fn sentence(self) -> String {
        let fixed = match self {
            Self::Status(status) => return format!("{ANSWERED} {status}"),
            Self::Unsupported => SSE_UNSUPPORTED,
            Self::Invalid => INVALID,
            Self::Unresolved => UNRESOLVED,
            Self::Address => INVALID_URL,
            Self::Metadata => METADATA_ADDRESS,
            Self::Header => INVALID_HEADER,
            Self::Client => CLIENT_UNBUILT,
            Self::Timeout => TIMED_OUT,
            Self::Redirect => REDIRECTED,
            Self::Unauthorized => UNAUTHORIZED,
            Self::Forbidden => FORBIDDEN,
            Self::Protocol => NOT_MCP,
            Self::Rejected => REJECTED,
            Self::Unreachable => UNREACHABLE,
            Self::Incomplete => INCOMPLETE,
            Self::Unknown => UNCONNECTED,
        };
        fixed.to_string()
    }
}

/// Words a failure's text opens with, and the failure they name.
#[derive(Debug, Clone, Copy)]
enum Lead {
    Fixed(&'static str, Failure),
    Status(&'static str),
}

struct Opened {
    length: usize,
    failure: Failure,
}

impl Lead {
    fn longest(leads: &[Self], text: &str) -> Option<Failure> {
        leads
            .iter()
            .filter_map(|lead| lead.opened(text))
            .max_by_key(|opened| opened.length)
            .map(|opened| opened.failure)
    }

    fn opened(self, text: &str) -> Option<Opened> {
        let (words, failure) = match self {
            Self::Fixed(words, failure) => (words, failure),
            Self::Status(words) => (words, Failure::Status(status(text.strip_prefix(words)?)?)),
        };
        text.starts_with(words).then_some(Opened {
            length: words.len(),
            failure,
        })
    }
}

fn staged<'a>(stage: &str, name: &str, error: &'a str) -> Option<&'a str> {
    error.strip_prefix(&format!("{stage} '{name}': "))
}

fn unwrapped(message: &str) -> &str {
    WRAPPERS
        .iter()
        .find_map(|wrapper| message.strip_prefix(wrapper))
        .and_then(|named| named.split_once(WRAPPED))
        .map_or(message, |(_, inner)| inner)
}

fn status(following: &str) -> Option<u16> {
    let (digits, rest) = following.as_bytes().split_at_checked(STATUS_DIGITS)?;
    if !digits.iter().all(u8::is_ascii_digit) || rest.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    following[..STATUS_DIGITS]
        .parse()
        .ok()
        .filter(|status| STATUSES.contains(status))
}

#[cfg(test)]
mod tests {
    use abnegate_agent::mcp::McpConfig;
    use abnegate_agent::mcp::McpHub;
    use abnegate_agent::mcp::McpOutcome;
    use abnegate_agent::mcp::McpServer;
    use abnegate_agent::tool::Proxy;

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
    const WORKER: &str = "rmcp::transport::worker::WorkerTransport<rmcp::transport::streamable_http_client::StreamableHttpClientWorker<abnegate_agent::mcp::remote::Remote>>";
    const SSE_REFUSED: &str = "SSE servers are not supported; use streamable HTTP";
    const DEFINITION_REFUSED: &str = "set exactly one of `command` and `url`, and a `type`, if any, of `stdio`, `http` or `sse` that matches it";
    const UNRESOLVED_REFERENCE: &str =
        "its URL or headers do not resolve against the server's bound secrets";
    const URL_REFUSED: &str = "invalid URL";
    const METADATA_REFUSED: &str = "refusing a link-local or cloud metadata address";
    const HEADER_REFUSED: &str = "invalid header `Bad Header`";
    const PROXY_REFUSED: &str = "invalid proxy URL";
    const HOSTILE: &str = "Auth required HTTP 599 Client error: refused a redirect to x] error: \
         JSON-RPC error: handshake timed out";
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

    fn definition(server: McpServer) -> Definition {
        Definition::of("docs", server)
    }

    fn handshake(message: &str) -> String {
        format!("MCP handshake failed for '{NAME}': {message}")
    }

    fn transport(message: &str) -> String {
        format!("MCP transport failed for '{NAME}': {message}")
    }

    fn initializing(inner: &str) -> String {
        handshake(&format!(
            "Send message error Transport [{WORKER}] error: {inner}, when send initialize request"
        ))
    }

    fn listing(inner: &str) -> String {
        handshake(&format!(
            "Transport send error: Transport [{WORKER}] error: {inner}"
        ))
    }

    fn answered(status: u16) -> String {
        format!("The server answered HTTP {status}")
    }

    fn shapes() -> Vec<(String, String)> {
        let fixed = |text: String, sentence: &str| (text, sentence.to_string());
        vec![
            fixed(
                SSE_REFUSED.to_string(),
                "SSE servers are not supported; use streamable HTTP (`http`)",
            ),
            fixed(
                DEFINITION_REFUSED.to_string(),
                "The server's definition is not valid",
            ),
            fixed(
                transport(UNRESOLVED_REFERENCE),
                "The URL or a header refers to a variable that is not set",
            ),
            fixed(transport(URL_REFUSED), "The URL is not valid"),
            fixed(
                transport(METADATA_REFUSED),
                "The URL points at a link-local or cloud metadata address",
            ),
            fixed(transport(HEADER_REFUSED), "A header is not valid"),
            fixed(
                transport(PROXY_REFUSED),
                "Zone could not build an HTTP client for the server",
            ),
            fixed(
                transport("could not build an HTTP client: builder error"),
                "Zone could not build an HTTP client for the server",
            ),
            fixed(
                handshake("handshake timed out"),
                "The server did not answer in time",
            ),
            fixed(
                handshake("request timeout after PT30S"),
                "The server did not answer in time",
            ),
            fixed(
                initializing(
                    "unexpected server response: refused a redirect to \
                     http://127.0.0.1:1/remote-said-7f3e9a",
                ),
                "The server answered with a redirect Zone does not follow",
            ),
            fixed(
                initializing("unexpected server response: refused more than 10 redirects"),
                "The server answered with a redirect Zone does not follow",
            ),
            fixed(
                listing(
                    "unexpected server response: refused a redirect to \
                     http://127.0.0.1:61479/remote-said-7f3e9a",
                ),
                "The server answered with a redirect Zone does not follow",
            ),
            fixed(
                initializing("Auth required"),
                "The server refused the credentials (HTTP 401)",
            ),
            fixed(
                listing("Auth required"),
                "The server refused the credentials (HTTP 401)",
            ),
            fixed(
                initializing("Insufficient scope"),
                "The server refused the credentials (HTTP 403)",
            ),
            (initializing("HTTP 500"), answered(500)),
            (initializing("HTTP 429"), answered(429)),
            (listing("HTTP 500"), answered(500)),
            (
                initializing(
                    "unexpected server response: HTTP 401 Unauthorized: remote-said-7f3e9a",
                ),
                answered(401),
            ),
            (
                initializing("unexpected server response: HTTP 403 Forbidden: remote-said-7f3e9a"),
                answered(403),
            ),
            (
                initializing("unexpected server response: HTTP 404 Not Found: remote-said-7f3e9a"),
                answered(404),
            ),
            (
                initializing(
                    "unexpected server response: HTTP 400 Bad Request: remote-said-7f3e9a: \
                     unknown key [REDACTED]",
                ),
                answered(400),
            ),
            (
                listing("unexpected server response: HTTP 400 Bad Request: remote-said-7f3e9a"),
                answered(400),
            ),
            (listing("Session expired (HTTP 404)"), answered(404)),
            fixed(
                initializing("Unexpected content type: Some(\"text/html\")"),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                initializing("Unexpected content type: None"),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                initializing(
                    "unexpected server response: could not parse JSON response as \
                     ServerJsonRpcMessage: key must be a string at line 1 column 2: \
                     {remote-said-7f3e9a",
                ),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                initializing("Deserialize error: expected value at line 1 column 1"),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake(
                    "expect initialized result, but received: Some(ListToolsResult(\
                     ListToolsResult { result_type: None, meta: None, next_cursor: None, \
                     ttl_ms: None, cache_scope: None, tools: [] }))",
                ),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake("expect initialized response, but received: None"),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake(
                    "uncorrelated error response: expected id 0, error response carried \
                     remote-said-7f3e9a",
                ),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake("conflict initialized response id: expected 0, got 7"),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake(
                    "no compatible protocol version (client: [ProtocolVersion(\"2025-11-25\")], \
                     server: [ProtocolVersion(\"1999-01-01\")])",
                ),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake("Unexpected response type"),
                "The server's answer is not MCP over streamable HTTP",
            ),
            fixed(
                handshake("JSON-RPC error: -32000: remote-said-7f3e9a(\"remote-said-7f3e9a\")"),
                "The server answered the handshake with an error",
            ),
            fixed(
                handshake("Mcp error: -32000: remote-said-7f3e9a(\"remote-said-7f3e9a\")"),
                "The server answered the handshake with an error",
            ),
            fixed(
                initializing("Client error: error sending request"),
                "Zone could not reach the server",
            ),
            fixed(
                handshake(&format!(
                    "Send message error Transport [{WORKER}] error: Transport channel closed, \
                     when send initialized notification"
                )),
                "The server did not complete the handshake",
            ),
            fixed(
                initializing("HTTP response body exceeded the maximum size of 65536 bytes"),
                "The server did not complete the handshake",
            ),
            fixed(
                transport("something this build has never seen"),
                "did not connect; see the server log",
            ),
            fixed(
                "failed to start MCP server 'docs': No such file or directory".to_string(),
                "did not connect; see the server log",
            ),
        ]
    }

    fn refusal(hub: &McpHub, name: &str) -> String {
        match hub.outcome(name) {
            Some(McpOutcome::Failed { error, .. }) => error,
            outcome => panic!("{name} did not fail: {outcome:?}"),
        }
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
    fn each_failure_shape_reads_as_its_sentence() {
        for (text, sentence) in shapes() {
            assert_eq!(summary(NAME, &text), sentence, "{text}");
        }
    }

    #[test]
    fn a_summary_is_one_line_of_zones_own_words() {
        let hostile = [
            handshake(HOSTILE),
            transport(HOSTILE),
            initializing(HOSTILE),
            listing(HOSTILE),
            HOSTILE.to_string(),
            String::new(),
        ];

        for text in shapes().into_iter().map(|(text, _)| text).chain(hostile) {
            let summary = summary(NAME, &text);
            let status = summary
                .strip_prefix(ANSWERED)
                .and_then(|status| status.strip_prefix(' '))
                .is_some_and(|status| {
                    status.len() == STATUS_DIGITS
                        && status.bytes().all(|digit| digit.is_ascii_digit())
                });
            assert!(
                SENTENCES.contains(&summary.as_str()) || status,
                "{text} became {summary}"
            );
            assert!(!summary.contains('\n'), "{summary}");
        }
    }

    #[test]
    fn a_remote_body_cannot_choose_its_kind() {
        let cases = [
            (
                initializing(&format!(
                    "unexpected server response: HTTP 400 Bad Request: {HOSTILE}"
                )),
                answered(400),
            ),
            (
                listing(&format!(
                    "unexpected server response: HTTP 400 Bad Request: {HOSTILE}"
                )),
                answered(400),
            ),
            (
                initializing(&format!(
                    "unexpected server response: refused a redirect to http://{HOSTILE}"
                )),
                REDIRECTED.to_string(),
            ),
            (
                initializing(&format!(
                    "unexpected server response: could not parse JSON response as \
                     ServerJsonRpcMessage: expected value at line 1 column 1: {HOSTILE}"
                )),
                NOT_MCP.to_string(),
            ),
            (
                initializing(&format!("Unexpected content type: Some(\"{HOSTILE}\")")),
                NOT_MCP.to_string(),
            ),
            (
                handshake(&format!(
                    "uncorrelated error response: expected id 0, error response carried {HOSTILE}"
                )),
                NOT_MCP.to_string(),
            ),
            (
                handshake(&format!("JSON-RPC error: -32000: {HOSTILE}")),
                REJECTED.to_string(),
            ),
            (
                handshake(&format!(
                    "JSON-RPC error: -32000: Send message error Transport [x] error: {HOSTILE}"
                )),
                REJECTED.to_string(),
            ),
            (
                handshake(&format!("Mcp error: -32000: {HOSTILE}")),
                REJECTED.to_string(),
            ),
        ];

        for (text, sentence) in cases {
            assert_eq!(summary(NAME, &text), sentence, "{text}");
        }
    }

    #[test]
    fn a_shape_this_build_does_not_know_is_never_searched() {
        for text in [
            handshake(&format!("task cancelled for reason {HOSTILE}")),
            handshake(&format!("connection closed: {HOSTILE}")),
            initializing(&format!("SSE error: {HOSTILE}")),
            listing(&format!("something new: {HOSTILE}")),
        ] {
            assert_eq!(summary(NAME, &text), INCOMPLETE, "{text}");
        }
        for text in [
            transport(&format!("something new: {HOSTILE}")),
            HOSTILE.to_string(),
            format!("{SSE_REFUSED} {HOSTILE}"),
        ] {
            assert_eq!(summary(NAME, &text), UNCONNECTED, "{text}");
        }
    }

    #[test]
    fn a_marker_in_the_servers_name_is_not_read() {
        for name in [
            "HTTP 123",
            "Auth required",
            "x': Client error: ",
            "MCP transport failed for 'x': invalid URL",
        ] {
            let refused = format!(
                "MCP handshake failed for '{name}': Send message error Transport [{WORKER}] \
                 error: Insufficient scope, when send initialize request"
            );
            let unparsed = format!("MCP transport failed for '{name}': invalid URL");

            assert_eq!(summary(name, &refused), FORBIDDEN, "{name}");
            assert_eq!(summary(name, &unparsed), INVALID_URL, "{name}");
        }
        assert_eq!(
            summary("other", &initializing("HTTP 500")),
            UNCONNECTED,
            "the text names a server the caller did not ask about"
        );
    }

    #[test]
    fn a_status_is_read_only_from_exactly_three_digits() {
        for (inner, sentence) in [
            ("HTTP 500", answered(500)),
            ("HTTP 100", answered(100)),
            ("HTTP 999", answered(999)),
            (
                "unexpected server response: HTTP 418 I'm a teapot: HTTP 599",
                answered(418),
            ),
            (
                "HTTP response body exceeded the maximum size of 65536 bytes",
                INCOMPLETE.to_string(),
            ),
            ("HTTP 5000", INCOMPLETE.to_string()),
            ("HTTP 50", INCOMPLETE.to_string()),
            ("HTTP 099", INCOMPLETE.to_string()),
            ("HTTP ٥٠٠", INCOMPLETE.to_string()),
            ("HTTP ", INCOMPLETE.to_string()),
            ("HTTP", INCOMPLETE.to_string()),
            (
                "unexpected server response: HTTP 5000 x",
                NOT_MCP.to_string(),
            ),
            (
                "unexpected server response: HTTP response",
                NOT_MCP.to_string(),
            ),
        ] {
            assert_eq!(summary(NAME, &initializing(inner)), sentence, "{inner}");
        }
    }

    #[test]
    fn the_longest_words_a_text_opens_with_decide() {
        for (inner, sentence) in [
            (
                "unexpected server response: HTTP 302 Found: ",
                answered(302),
            ),
            (
                "unexpected server response: refused a redirect to http://127.0.0.1:1/",
                REDIRECTED.to_string(),
            ),
            (
                "unexpected server response: refused more than 10 redirects",
                REDIRECTED.to_string(),
            ),
            ("unexpected server response: refused", NOT_MCP.to_string()),
            (
                "unexpected server response: invalid www-authenticate header value",
                NOT_MCP.to_string(),
            ),
        ] {
            assert_eq!(summary(NAME, &initializing(inner)), sentence, "{inner}");
            assert_eq!(summary(NAME, &listing(inner)), sentence, "{inner}");
        }
    }

    #[tokio::test]
    async fn the_hub_words_its_refusals_the_way_summary_reads_them() {
        let unresolved = McpServer::remote("http://127.0.0.1:1/${ZONE_TEST_MCP_UNSET}");
        let configuration = McpConfig::default()
            .with_server(
                "events",
                McpServer::remote("http://127.0.0.1:1/sse").with_transport(McpTransport::Sse),
            )
            .with_server(
                "mismatched",
                McpServer::remote("http://127.0.0.1:1/mcp").with_transport(McpTransport::Stdio),
            )
            .with_server("unresolved", unresolved)
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

        let staged =
            |name: &str, message: &str| format!("MCP transport failed for '{name}': {message}");
        let expected = [
            ("events", SSE_REFUSED.to_string(), SSE_UNSUPPORTED),
            ("mismatched", DEFINITION_REFUSED.to_string(), INVALID),
            (
                "unresolved",
                staged("unresolved", UNRESOLVED_REFERENCE),
                UNRESOLVED,
            ),
            ("unparsed", staged("unparsed", URL_REFUSED), INVALID_URL),
            ("schemed", staged("schemed", URL_REFUSED), INVALID_URL),
            (
                "metadata",
                staged("metadata", METADATA_REFUSED),
                METADATA_ADDRESS,
            ),
            ("headed", staged("headed", HEADER_REFUSED), INVALID_HEADER),
        ];
        for (name, text, sentence) in expected {
            let refusal = refusal(&hub, name);
            assert_eq!(refusal, text, "{name}");
            assert_eq!(summary(name, &refusal), sentence, "{name}");
        }
        let refusal = refusal(&unrouted, "proxied");
        assert_eq!(refusal, staged("proxied", PROXY_REFUSED));
        assert_eq!(summary("proxied", &refusal), CLIENT_UNBUILT);
    }
}
