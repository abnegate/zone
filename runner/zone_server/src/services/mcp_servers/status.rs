use std::ops::RangeInclusive;

use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::McpTransport;
use abnegate_secret::REDACTED;
use abnegate_secret::SecretValue;
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
const RELAYED: &str = "unexpected server response: ";
const ANSWER: &str = "HTTP ";
const STATUS_DIGITS: usize = 3;
const STATUSES: RangeInclusive<u16> = 100..=999;
const SCRUBBED_LENGTH: usize = 6;

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

const HANDSHAKES: [Lead; 16] = [
    Lead::Fixed("handshake timed out", Failure::Timeout),
    Lead::Fixed("request timeout after ", Failure::Timeout),
    Lead::Fixed("Auth required", Failure::Unauthorized),
    Lead::Fixed("Insufficient scope", Failure::Forbidden),
    Lead::Status(""),
    Lead::Status("Session expired ("),
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

const REDIRECTS: [Lead; 2] = [
    Lead::Fixed("refused a redirect to ", Failure::Redirect),
    Lead::Fixed("refused more than ", Failure::Redirect),
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
/// the failure text of the hub that connected `server` as `name`.
///
/// Nothing else of `error` is ever copied. The client reads up to 64 KiB of a refusal's body
/// into that text and private hosts are reachable on purpose, so the text itself would hand an
/// organization's administrator whatever any host this server can reach says back. The
/// sentence is chosen by the words the hub and its client open the text with, which precede
/// anything a remote supplied; the rest is never searched, so a remote cannot choose it.
///
/// The hub scrubs every value `server` binds out of its message, the client's own opening
/// words included wherever a piece of the URL or a header's value spells one. Those words are
/// read through the scrub: [`REDACTED`] stands for a stretch of an opening only where that
/// opening spells one of the server's own bound values whole, never for anything else. A
/// status is only ever the digits the text itself holds after a plain `HTTP `, and a text
/// that two kinds could have opened names neither.
pub fn summary(name: &str, server: &McpServer, error: &str) -> String {
    Failure::of(name, &Scrub::of(server), error).sentence()
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
    fn of(name: &str, scrub: &Scrub<'_>, error: &str) -> Self {
        if let Some(message) = staged(HANDSHAKE, name, error) {
            return Self::handshake(scrub, message);
        }
        if let Some(message) = staged(TRANSPORT, name, error) {
            return Lead::agreed(&TRANSPORTS, message, scrub).unwrap_or(Self::Unknown);
        }
        REFUSALS
            .iter()
            .find(|(text, _)| *text == error)
            .map_or(Self::Unknown, |(_, failure)| *failure)
    }

    fn handshake(scrub: &Scrub<'_>, message: &str) -> Self {
        let mut named = Vec::new();
        for message in scrub.unwrapped(message) {
            named.extend(
                HANDSHAKES
                    .iter()
                    .flat_map(|lead| lead.named(message, scrub)),
            );
            let relayed = scrub.after(message, RELAYED);
            named.extend(
                relayed
                    .into_iter()
                    .map(|message| Self::relayed(scrub, message)),
            );
        }
        agreed(named).unwrap_or(Self::Incomplete)
    }

    /// What the client's transport reports as a response it did not expect: a response it
    /// could not use, a redirect the hub refused, and any other failure of its own whose text
    /// the hub scrubbed, which the hub hands back as one of these.
    fn relayed(scrub: &Scrub<'_>, message: &str) -> Self {
        Lead::agreed(HANDSHAKES.iter().chain(&REDIRECTS), message, scrub).unwrap_or(Self::Protocol)
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

/// Words a failure's text opens with, and the failure they name: a fixed one, or the status
/// that follows them as `HTTP ` and its digits.
#[derive(Debug, Clone, Copy)]
enum Lead {
    Fixed(&'static str, Failure),
    Status(&'static str),
}

impl Lead {
    fn agreed<'a>(
        leads: impl IntoIterator<Item = &'a Self>,
        text: &str,
        scrub: &Scrub<'_>,
    ) -> Option<Failure> {
        agreed(leads.into_iter().flat_map(|lead| lead.named(text, scrub)))
    }

    fn named(self, text: &str, scrub: &Scrub<'_>) -> Vec<Failure> {
        match self {
            Self::Fixed(words, failure) => {
                scrub.after(text, words).iter().map(|_| failure).collect()
            }
            Self::Status(words) => scrub
                .after(text, words)
                .into_iter()
                .filter_map(|answer| status(answer.strip_prefix(ANSWER)?))
                .map(Failure::Status)
                .collect(),
        }
    }
}

/// The values the hub scrubs from one server's failure text: each the server binds that is
/// long enough for it to.
struct Scrub<'a> {
    values: Vec<&'a str>,
}

impl<'a> Scrub<'a> {
    fn of(server: &'a McpServer) -> Self {
        let mut values: Vec<&str> = server
            .secrets
            .values()
            .map(SecretValue::expose)
            .filter(|value| value.len() >= SCRUBBED_LENGTH)
            .collect();
        values.sort_unstable();
        values.dedup();
        Self { values }
    }

    /// Each way `text` goes on once it has opened with `words`. Where the text holds
    /// [`REDACTED`], the words have to spell one of the scrubbed values whole in its place.
    fn after<'t>(&self, mut text: &'t str, mut words: &str) -> Vec<&'t str> {
        loop {
            let Some(letter) = words.chars().next() else {
                return vec![text];
            };
            if let Some(text) = text.strip_prefix(REDACTED) {
                return self
                    .values
                    .iter()
                    .filter_map(|value| words.strip_prefix(*value))
                    .flat_map(|words| self.after(text, words))
                    .collect();
            }
            let Some(rest) = text.strip_prefix(letter) else {
                return Vec::new();
            };
            text = rest;
            words = &words[letter.len_utf8()..];
        }
    }

    fn unwrapped<'t>(&self, message: &'t str) -> Vec<&'t str> {
        let inner: Vec<&str> = WRAPPERS
            .iter()
            .flat_map(|wrapper| self.after(message, wrapper))
            .flat_map(|named| {
                named
                    .char_indices()
                    .map(|(start, _)| self.after(&named[start..], WRAPPED))
                    .find(|inner| !inner.is_empty())
                    .unwrap_or_default()
            })
            .collect();
        if inner.is_empty() {
            vec![message]
        } else {
            inner
        }
    }
}

fn agreed(failures: impl IntoIterator<Item = Failure>) -> Option<Failure> {
    let mut failures = failures.into_iter();
    let first = failures.next()?;
    failures.all(|failure| failure == first).then_some(first)
}

fn staged<'a>(stage: &str, name: &str, error: &'a str) -> Option<&'a str> {
    error.strip_prefix(&format!("{stage} '{name}': "))
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
    use std::cmp::Reverse;
    use zone_core::servers::mask;

    use super::super::stub::Stub;
    use super::super::testing::REMOTE_URL;
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
    const VEILED: &str = "Auth [REDACTED] HTTP 599 [REDACTED] error: [REDACTED] a redirect to \
         x] error: unexpected [REDACTED] response: HTTP 599 [REDACTED] timed out";
    const WORDS: [&str; 8] = [
        "required",
        "refused",
        "server",
        "response",
        "unexpected",
        "handshake",
        "JSON-RPC",
        "Transport",
    ];
    const REFUSED: &str = "unexpected server response: HTTP 400 Bad Request: remote-said-7f3e9a";
    const ORIGIN: &str = "http://127.0.0.1:1";
    const MODE_HEADER: &str = "X-Mode";
    const KEY_FLAG: &str = "--key";
    const KEY_HEADER: &str = "X-Key";
    const STUB_KEY: &str = "the-key-the-stub-wants";
    const WRONG_KEY: &str = "knock-knock-knock-knock";
    const TOKEN_FLAG: &str = "--token";
    const STUB_TOKEN: &str = "stub-token-51c2d9";
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

    /// A server that binds nothing, so its hub scrubs nothing from what it says.
    fn unbound() -> McpServer {
        McpServer::remote(REMOTE_URL)
    }

    /// A server whose URL's path is `segments` and that sends `headers`, bound the way Zone
    /// binds every server it connects.
    fn bound(segments: &[&str], headers: &[(&str, &str)]) -> McpServer {
        let mut server = headers.iter().fold(
            McpServer::remote(format!("{ORIGIN}/{}", segments.join("/"))),
            |server, (name, value)| server.with_header(*name, *value),
        );
        mask(&mut server);
        server
    }

    /// `message` as the hub leaves it for `server`: without any value the server binds that
    /// is long enough to scrub, the longest first.
    fn scrubbed(server: &McpServer, message: &str) -> String {
        let mut values: Vec<&str> = server
            .secrets
            .values()
            .map(SecretValue::expose)
            .filter(|value| value.len() >= SCRUBBED_LENGTH)
            .collect();
        values.sort_by_key(|value| Reverse(value.len()));
        values.iter().fold(message.to_string(), |message, value| {
            message.replace(value, REDACTED)
        })
    }

    fn handshake(message: &str) -> String {
        format!("MCP handshake failed for '{NAME}': {message}")
    }

    fn transport(message: &str) -> String {
        format!("MCP transport failed for '{NAME}': {message}")
    }

    fn initialize(inner: &str) -> String {
        format!(
            "Send message error Transport [{WORKER}] error: {inner}, when send initialize request"
        )
    }

    fn list(inner: &str) -> String {
        format!("Transport send error: Transport [{WORKER}] error: {inner}")
    }

    fn initializing(inner: &str) -> String {
        handshake(&initialize(inner))
    }

    fn listing(inner: &str) -> String {
        handshake(&list(inner))
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
            assert_eq!(summary(NAME, &unbound(), &text), sentence, "{text}");
        }
    }

    #[test]
    fn a_summary_is_one_line_of_zones_own_words() {
        let hostile = [HOSTILE, VEILED].into_iter().flat_map(|hostile| {
            [
                handshake(hostile),
                transport(hostile),
                initializing(hostile),
                listing(hostile),
                hostile.to_string(),
            ]
        });
        let texts: Vec<String> = shapes()
            .into_iter()
            .map(|(text, _)| text)
            .chain(hostile)
            .chain([String::new()])
            .collect();

        for (server, text) in [unbound(), bound(&WORDS, &[])]
            .iter()
            .flat_map(|server| texts.iter().map(move |text| (server, text)))
        {
            let summary = summary(NAME, server, text);
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

    /// A remote that knows what it was sent can write those values, which the hub scrubs, or
    /// the redaction itself. Neither is read: both stand after the words that named the kind.
    #[test]
    fn a_remote_body_cannot_choose_its_kind() {
        for (server, hostile) in [unbound(), bound(&WORDS, &[])]
            .iter()
            .flat_map(|server| [HOSTILE, VEILED].map(|hostile| (server, hostile)))
        {
            let refused = format!("unexpected server response: HTTP 400 Bad Request: {hostile}");
            let cases = [
                (initialize(&refused), answered(400)),
                (list(&refused), answered(400)),
                (
                    initialize(&format!(
                        "unexpected server response: refused a redirect to http://{hostile}"
                    )),
                    REDIRECTED.to_string(),
                ),
                (
                    initialize(&format!(
                        "unexpected server response: could not parse JSON response as \
                         ServerJsonRpcMessage: expected value at line 1 column 1: {hostile}"
                    )),
                    NOT_MCP.to_string(),
                ),
                (
                    initialize(&format!("Unexpected content type: Some(\"{hostile}\")")),
                    NOT_MCP.to_string(),
                ),
                (
                    format!(
                        "uncorrelated error response: expected id 0, error response carried \
                         {hostile}"
                    ),
                    NOT_MCP.to_string(),
                ),
                (
                    format!("JSON-RPC error: -32000: {hostile}"),
                    REJECTED.to_string(),
                ),
                (format!("JSON-RPC error: {hostile}"), REJECTED.to_string()),
                (
                    format!(
                        "JSON-RPC error: -32000: Send message error Transport [x] error: \
                         {hostile}"
                    ),
                    REJECTED.to_string(),
                ),
                (
                    format!("Mcp error: -32000: {hostile}"),
                    REJECTED.to_string(),
                ),
            ];

            for (message, sentence) in cases {
                let text = handshake(&scrubbed(server, &message));
                assert_eq!(summary(NAME, server, &text), sentence, "{text}");
            }
        }
    }

    #[test]
    fn a_shape_this_build_does_not_know_is_never_searched() {
        for (server, hostile) in [unbound(), bound(&WORDS, &[])]
            .iter()
            .flat_map(|server| [HOSTILE, VEILED].map(|hostile| (server, hostile)))
        {
            for message in [
                format!("task cancelled for reason {hostile}"),
                format!("connection closed: {hostile}"),
                initialize(&format!("SSE error: {hostile}")),
                list(&format!("something new: {hostile}")),
            ] {
                let text = handshake(&scrubbed(server, &message));
                assert_eq!(summary(NAME, server, &text), INCOMPLETE, "{text}");
            }
            for text in [
                transport(&format!("something new: {hostile}")),
                hostile.to_string(),
                format!("{SSE_REFUSED} {hostile}"),
            ] {
                assert_eq!(summary(NAME, server, &text), UNCONNECTED, "{text}");
            }
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

            assert_eq!(summary(name, &unbound(), &refused), FORBIDDEN, "{name}");
            assert_eq!(summary(name, &unbound(), &unparsed), INVALID_URL, "{name}");
        }
        assert_eq!(
            summary("other", &unbound(), &initializing("HTTP 500")),
            UNCONNECTED,
            "the text names a server the caller did not ask about"
        );
    }

    /// The hub scrubs a failure's message and leaves the name before it as it is, so a name
    /// spelled like something the server binds is read as written, and never as a redaction.
    #[test]
    fn a_name_the_server_also_binds_is_read_as_written() {
        for name in ["server", "response", "Transport"] {
            let server = bound(&[name, "mcp"], &[]);
            let message = scrubbed(&server, &initialize(REFUSED));
            let named = format!("MCP handshake failed for '{name}': {message}");
            let hidden = format!("MCP handshake failed for '{REDACTED}': {message}");

            assert!(message.contains(REDACTED), "{message}");
            assert_eq!(summary(name, &server, &named), answered(400), "{named}");
            assert_eq!(summary(name, &server, &hidden), UNCONNECTED, "{hidden}");
        }
    }

    #[test]
    fn a_bound_value_spelled_like_the_clients_words_keeps_the_kind() {
        let timed = "request timeout after PT30S";
        let redirected = "unexpected server response: refused a redirect to \
             http://127.0.0.1:1/remote-said-7f3e9a";
        let looped = "unexpected server response: refused more than 10 redirects";
        let typed = "Unexpected content type: Some(\"text/html\")";
        let unparsed = "unexpected server response: could not parse JSON response as \
             ServerJsonRpcMessage: expected value at line 1 column 1: remote-said-7f3e9a";
        let versioned = "no compatible protocol version (client: [ProtocolVersion(\
             \"2025-11-25\")], server: [ProtocolVersion(\"1999-01-01\")])";
        let rejected = "JSON-RPC error: -32000: remote-said-7f3e9a(\"remote-said-7f3e9a\")";
        let cases = [
            ("server", initialize(REFUSED), answered(400)),
            ("response", initialize(REFUSED), answered(400)),
            ("unexpected", initialize(REFUSED), answered(400)),
            ("Transport", initialize(REFUSED), answered(400)),
            ("message", initialize(REFUSED), answered(400)),
            ("transport", initialize(REFUSED), answered(400)),
            ("Remote", initialize(REFUSED), answered(400)),
            ("error:", initialize(REFUSED), answered(400)),
            ("initialize", initialize(REFUSED), answered(400)),
            ("server", list(REFUSED), answered(400)),
            ("Transport", list(REFUSED), answered(400)),
            ("error:", list(REFUSED), answered(400)),
            ("Session", list("Session expired (HTTP 404)"), answered(404)),
            ("expired", list("Session expired (HTTP 404)"), answered(404)),
            (
                "handshake",
                "handshake timed out".to_string(),
                TIMED_OUT.to_string(),
            ),
            ("timeout", timed.to_string(), TIMED_OUT.to_string()),
            ("request", timed.to_string(), TIMED_OUT.to_string()),
            ("refused", initialize(redirected), REDIRECTED.to_string()),
            ("redirect", initialize(redirected), REDIRECTED.to_string()),
            ("response", initialize(redirected), REDIRECTED.to_string()),
            ("refused", list(looped), REDIRECTED.to_string()),
            ("content", initialize(typed), NOT_MCP.to_string()),
            ("Unexpected", initialize(typed), NOT_MCP.to_string()),
            ("response", initialize(unparsed), NOT_MCP.to_string()),
            ("protocol", versioned.to_string(), NOT_MCP.to_string()),
            ("version", versioned.to_string(), NOT_MCP.to_string()),
            ("JSON-RPC", rejected.to_string(), REJECTED.to_string()),
        ];

        for (word, message, sentence) in cases {
            for server in [
                bound(&["api", word, "mcp"], &[]),
                bound(&["mcp"], &[(MODE_HEADER, word)]),
                bound(&["mcp"], &[("Authorization", &format!("Bearer {word}"))]),
            ] {
                let text = handshake(&scrubbed(&server, &message));

                assert!(text.contains(REDACTED), "{word} is not in {message}");
                assert_eq!(summary(NAME, &server, &text), sentence, "{text}");
            }
        }
    }

    /// The hub's client hands back, as a response it did not expect, any failure of its own
    /// whose text the hub scrubbed. The failure it was is still read behind those words.
    #[test]
    fn a_failure_the_hub_scrubbed_and_relayed_keeps_its_kind() {
        let cases = [
            (
                bound(&["required", "mcp"], &[]),
                initialize("unexpected server response: Auth [REDACTED]"),
                UNAUTHORIZED.to_string(),
            ),
            (
                bound(&["response", "required", "mcp"], &[]),
                initialize("unexpected server [REDACTED]: Auth [REDACTED]"),
                UNAUTHORIZED.to_string(),
            ),
            (
                bound(&["mcp"], &[(MODE_HEADER, "Insufficient")]),
                initialize("unexpected server response: [REDACTED] scope"),
                FORBIDDEN.to_string(),
            ),
            (
                bound(&["sending", "mcp"], &[]),
                initialize("unexpected server response: Client error: error [REDACTED] request"),
                UNREACHABLE.to_string(),
            ),
            (
                bound(&["Client", "mcp"], &[]),
                initialize("unexpected server response: [REDACTED] error: error sending request"),
                UNREACHABLE.to_string(),
            ),
            (
                bound(&["expired", "mcp"], &[]),
                list("unexpected server response: Session [REDACTED] (HTTP 404)"),
                answered(404),
            ),
            (
                bound(&["content", "mcp"], &[]),
                initialize(
                    "unexpected server response: Unexpected [REDACTED] type: Some(\"text/html\")",
                ),
                NOT_MCP.to_string(),
            ),
            (
                bound(&["exceeded", "mcp"], &[]),
                initialize(
                    "unexpected server response: HTTP response body [REDACTED] the maximum \
                     size of 65536 bytes",
                ),
                NOT_MCP.to_string(),
            ),
        ];

        for (server, message, sentence) in cases {
            let text = handshake(&message);

            assert_eq!(summary(NAME, &server, &text), sentence, "{text}");
        }
    }

    #[test]
    fn a_redaction_stands_only_for_a_value_the_server_binds() {
        let server = bound(&["server", "mcp"], &[]);
        let cases = [
            ("unexpected [REDACTED] response: HTTP 400", answered(400)),
            (
                "[REDACTED] server response: HTTP 400",
                INCOMPLETE.to_string(),
            ),
            (
                "unexpected server [REDACTED]: HTTP 400",
                INCOMPLETE.to_string(),
            ),
            ("unexpected [REDACTED]: HTTP 400", INCOMPLETE.to_string()),
            (
                "unexpected [REDACTED][REDACTED] response: HTTP 400",
                INCOMPLETE.to_string(),
            ),
            ("[REDACTED]", INCOMPLETE.to_string()),
            ("Auth [REDACTED]", INCOMPLETE.to_string()),
            ("[REDACTED] error: remote-said", INCOMPLETE.to_string()),
            ("[REDACTED] timed out", INCOMPLETE.to_string()),
            (
                "unexpected [REDACTED] response: [REDACTED] a redirect to x",
                NOT_MCP.to_string(),
            ),
        ];

        for (inner, sentence) in cases {
            for text in [handshake(inner), initializing(inner), listing(inner)] {
                assert_eq!(summary(NAME, &server, &text), sentence, "{text}");
            }
        }
        for text in [
            transport("invalid [REDACTED]"),
            "[REDACTED]".to_string(),
            format!("MCP handshake failed for '{NAME}'[REDACTED] HTTP 400"),
        ] {
            assert_eq!(summary(NAME, &server, &text), UNCONNECTED, "{text}");
        }
        let unwrapped = handshake(&format!(
            "Send message error Transport [{WORKER}[REDACTED]HTTP 400"
        ));
        assert_eq!(summary(NAME, &server, &unwrapped), INCOMPLETE);
    }

    /// The hub leaves a value shorter than six bytes where it is, so a redaction is never one.
    #[test]
    fn a_value_too_short_for_the_hub_to_scrub_is_never_read_for_a_redaction() {
        let server = bound(&["error", "scope", "Auth", "mcp"], &[(MODE_HEADER, "HTTP")]);
        let plain = initialize(REFUSED);

        assert_eq!(scrubbed(&server, &plain), plain);
        assert_eq!(summary(NAME, &server, &handshake(&plain)), answered(400));
        for inner in [
            "Insufficient [REDACTED]",
            "[REDACTED] required",
            "JSON-RPC [REDACTED]: remote-said",
            "[REDACTED] 400",
        ] {
            let text = initializing(inner);
            assert_eq!(summary(NAME, &server, &text), INCOMPLETE, "{text}");
        }
    }

    /// Scrubbing can leave a text that two kinds would both have opened. Nothing in it says
    /// which one did, so it names neither.
    #[test]
    fn a_text_two_kinds_could_have_opened_names_neither() {
        let rejected = "JSON-RPC error: -32000: remote-said-7f3e9a";
        let unreached = "Client error: error sending request";
        let either = bound(&["JSON-RPC", "Client", "mcp"], &[]);
        let phrased = bound(&["Auth%20required", "Insufficient%20scope", "mcp"], &[]);

        for (word, message, sentence) in [
            ("JSON-RPC", rejected, REJECTED),
            ("Client", unreached, UNREACHABLE),
        ] {
            let server = bound(&[word, "mcp"], &[]);
            let text = handshake(&scrubbed(&server, message));
            assert_eq!(summary(NAME, &server, &text), sentence, "{text}");

            let text = handshake(&scrubbed(&either, message));
            assert_eq!(summary(NAME, &either, &text), INCOMPLETE, "{text}");
        }
        for message in ["Auth required", "Insufficient scope"] {
            let text = handshake(&scrubbed(&phrased, &initialize(message)));
            assert_eq!(summary(NAME, &phrased, &text), INCOMPLETE, "{text}");

            let relayed = format!("unexpected server response: {message}");
            let text = handshake(&scrubbed(&phrased, &initialize(&relayed)));
            assert_eq!(summary(NAME, &phrased, &text), NOT_MCP, "{text}");
        }
    }

    /// A status is the digits the text holds after a plain `HTTP `. A value the server binds
    /// can hide them, which leaves the kind without a status, and can never supply them: not
    /// its own digits, and not the remote's, which a value ending where the client's words end
    /// would otherwise put straight after a redaction.
    #[test]
    fn a_bound_value_never_makes_or_changes_a_status() {
        let bare = "HTTP 400";
        let cases = [
            ("400%20Bad%20Request", initialize(REFUSED), NOT_MCP),
            ("HTTP%20400", initialize(REFUSED), NOT_MCP),
            ("HTTP%204", initialize(REFUSED), NOT_MCP),
            ("0%20Bad%20Request", initialize(REFUSED), NOT_MCP),
            ("TP%20400", initialize(REFUSED), NOT_MCP),
            ("response:%20HTTP%20400", initialize(REFUSED), INCOMPLETE),
            ("HTTP%20400", initialize(bare), INCOMPLETE),
            ("TP%20400", list(bare), INCOMPLETE),
            (
                "(HTTP%20404)",
                list("Session expired (HTTP 404)"),
                INCOMPLETE,
            ),
        ];

        for (segment, message, sentence) in cases {
            let server = bound(&[segment, "mcp"], &[]);
            let text = handshake(&scrubbed(&server, &message));

            assert!(text.contains(REDACTED), "{segment} is not in {message}");
            assert_eq!(summary(NAME, &server, &text), sentence, "{text}");
        }

        let ending = bound(
            &[
                "response:%20HTTP%20",
                "server%20response:%20HTTP",
                "Session%20expired%20(HTTP%20",
                "mcp",
            ],
            &[],
        );
        for inner in [
            "unexpected server [REDACTED]599 remote-said",
            "unexpected [REDACTED] 599 remote-said",
            "unexpected server [REDACTED]599",
            "[REDACTED]404)",
        ] {
            for text in [initializing(inner), listing(inner)] {
                assert_eq!(summary(NAME, &ending, &text), INCOMPLETE, "{text}");
            }
        }
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
            assert_eq!(
                summary(NAME, &unbound(), &initializing(inner)),
                sentence,
                "{inner}"
            );
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
            for server in [unbound(), bound(&["server", "response", "refused"], &[])] {
                for message in [initialize(inner), list(inner)] {
                    let text = handshake(&scrubbed(&server, &message));
                    assert_eq!(summary(NAME, &server, &text), sentence, "{text}");
                }
            }
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
            assert_eq!(
                summary(name, &configuration.servers[name], &refusal),
                sentence,
                "{name}"
            );
        }
        let refusal = refusal(&unrouted, "proxied");
        assert_eq!(refusal, staged("proxied", PROXY_REFUSED));
        assert_eq!(
            summary("proxied", &proxied.servers["proxied"], &refusal),
            CLIENT_UNBUILT
        );
    }

    /// What `summary` takes the hub to do is the hub's to change: it scrubs a failure's
    /// message and not the name before it, of every value its server binds that is six bytes
    /// or longer and of none shorter, its client's own words among them.
    #[tokio::test]
    async fn the_hub_scrubs_its_own_words_the_way_summary_reads_them() {
        let stub = Stub::start(&[KEY_FLAG, STUB_KEY]).await;
        let origin = format!("http://127.0.0.1:{}", stub.port());
        let rows = [
            ("server", "server", WRONG_KEY),
            ("short", "error", WRONG_KEY),
            ("headed", "lane", "Transport"),
            ("phrased", "response", "unexpected server"),
        ];
        let servers: Vec<(&str, McpServer, String)> = rows
            .into_iter()
            .map(|(name, segment, key)| {
                let mut server = McpServer::remote(format!("{origin}/{segment}/mcp"))
                    .with_header(KEY_HEADER, key);
                mask(&mut server);
                let said = initialize(&format!(
                    "unexpected server response: HTTP 400 Bad Request: refused {key} at \
                     /{segment}/mcp"
                ));
                (name, server, said)
            })
            .collect();
        let configuration = servers
            .iter()
            .fold(McpConfig::default(), |configuration, (name, server, _)| {
                configuration.with_server(*name, server.clone())
            });

        let hub = McpHub::connect_through(&configuration, &Proxy::default()).await;

        for (name, server, said) in &servers {
            let refusal = refusal(&hub, name);
            assert_eq!(
                refusal,
                format!(
                    "MCP handshake failed for '{name}': {}",
                    scrubbed(server, said)
                )
            );
            assert_eq!(summary(name, server, &refusal), answered(400), "{name}");
        }
        let redactions = |name: &str| refusal(&hub, name).matches(REDACTED).count();
        assert_eq!(redactions("server"), 3);
        assert_eq!(redactions("short"), 2);
        assert_eq!(redactions("headed"), 4);
        assert_eq!(redactions("phrased"), 4);
    }

    /// A failure of the client's own that the hub scrubbed reaches `summary` as a response
    /// the client did not expect.
    #[tokio::test]
    async fn the_hub_relays_a_scrubbed_failure_the_way_summary_reads_it() {
        let stub = Stub::start(&[TOKEN_FLAG, STUB_TOKEN]).await;
        let mut server = McpServer::remote(stub.url()).with_header(MODE_HEADER, "required");
        mask(&mut server);
        let configuration = McpConfig::default().with_server(NAME, server.clone());

        let hub = McpHub::connect_through(&configuration, &Proxy::default()).await;

        let refusal = refusal(&hub, NAME);
        assert_eq!(
            refusal,
            initializing("unexpected server response: Auth [REDACTED]")
        );
        assert_eq!(summary(NAME, &server, &refusal), UNAUTHORIZED);
    }
}
