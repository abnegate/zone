use std::collections::BTreeSet;

use abnegate_agent::mcp::McpServer;
use abnegate_http::metadata;
use reqwest::Url;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;

use crate::services::mcp_servers::status;
use crate::services::mcp_servers::transport::Transport;

pub const NAME_LIMIT: usize = 32;

const NULL: char = '\0';
const RESERVED: [&str; 24] = [
    "accept",
    "connection",
    "content-length",
    "content-type",
    "host",
    "keep-alive",
    "last-event-id",
    "mcp-method",
    "mcp-name",
    "mcp-protocol-version",
    "mcp-session-id",
    "metadata",
    "metadata-flavor",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "x-aliyun-ecs-metadata-token",
    "x-aliyun-ecs-metadata-token-ttl-seconds",
    "x-aws-ec2-metadata-token",
    "x-aws-ec2-metadata-token-ttl-seconds",
    "x-google-metadata-request",
];
const RESERVED_PREFIX: &str = "mcp-param-";
const PLACEHOLDER: &str = "${";
const SCHEMES: [&str; 2] = ["http", "https"];
const PROBE: &str = "probe";
const NAME: &str = "name";
pub(crate) const URL: &str = "url";
pub(crate) const COMMAND: &str = "command";
pub(crate) const ARGUMENTS: &str = "arguments";
const TRANSPORT: &str = "transport";
pub(crate) const ENVIRONMENT: &str = "environment";
pub(crate) const HEADERS: &str = "headers";
const TOOLS: &str = "tools";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("`{field}` must be at most {limit} characters")]
    Length { field: String, limit: usize },
    #[error(
        "`{field}` must hold only letters, digits, `_` and `-`, must not contain `__` and must not end in `_`"
    )]
    Name { field: String },
    #[error("`{field}` must be an absolute URL")]
    Unparseable { field: String },
    #[error("`{field}` must use http or https")]
    Scheme { field: String },
    #[error("`{field}` must not include a username or password; put credentials in headers")]
    Userinfo { field: String },
    #[error("`{field}` must not include a fragment")]
    Fragment { field: String },
    #[error("`{field}` must not point at a link-local or cloud metadata address")]
    Metadata { field: String },
    #[error("`{field}` must not contain `${{`: Zone would expand it from its own environment")]
    Placeholder { field: String },
    #[error("`{field}` must not contain a NUL character")]
    Null { field: String },
    #[error("`{field}` must not start or end with whitespace")]
    Whitespace { field: String },
    #[error(
        "`{field}` name `{name}` must start with a letter or `_` and hold only letters, digits and `_`"
    )]
    EnvironmentName { field: String, name: String },
    #[error("`{field}` name `{name}` is not a valid HTTP header name")]
    HeaderName { field: String, name: String },
    #[error("`{field}` name `{name}` is set by the connection itself and cannot be configured")]
    ReservedHeader { field: String, name: String },
    #[error("`{field}` names `{name}` twice: header names ignore case")]
    DuplicateHeader { field: String, name: String },
    #[error("`{field}` values must not contain line breaks or other control characters")]
    HeaderValue { field: String },
    #[error(
        "`{field}` entry `{name}` must be non-empty and hold only letters, digits, `_` and `-`"
    )]
    Tool { field: String, name: String },
    #[error("`{field}` is required for `{transport}` servers")]
    Missing { field: String, transport: Transport },
    #[error("`{field}` is not allowed for `{transport}` servers")]
    Unexpected { field: String, transport: Transport },
    #[error("{}", status::SSE_UNSUPPORTED)]
    Unsupported { field: String },
    #[error("`{field}` must be `stdio`, `http` or `sse`, not `{value}`")]
    Transport { field: String, value: String },
}

pub fn name(name: &str) -> Result<(), ValidationError> {
    if !McpServer::default().nameable(name) {
        return Err(ValidationError::Name {
            field: NAME.to_string(),
        });
    }
    if name.chars().count() > NAME_LIMIT {
        return Err(ValidationError::Length {
            field: NAME.to_string(),
            limit: NAME_LIMIT,
        });
    }
    Ok(())
}

pub fn url(raw: &str) -> Result<Url, ValidationError> {
    let field = || URL.to_string();
    literal(URL, raw)?;
    let url =
        Url::parse(raw.trim()).map_err(|_| ValidationError::Unparseable { field: field() })?;
    literal(URL, url.as_str())?;
    if !SCHEMES.contains(&url.scheme()) {
        return Err(ValidationError::Scheme { field: field() });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ValidationError::Userinfo { field: field() });
    }
    if url.fragment().is_some() {
        return Err(ValidationError::Fragment { field: field() });
    }
    if metadata::is_url(&url) {
        return Err(ValidationError::Metadata { field: field() });
    }
    Ok(url)
}

pub fn literal(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.contains(PLACEHOLDER) {
        return Err(ValidationError::Placeholder {
            field: field.to_string(),
        });
    }
    if value.contains(NULL) {
        return Err(ValidationError::Null {
            field: field.to_string(),
        });
    }
    if field == HEADERS {
        header_value(value)?;
    }
    Ok(())
}

pub fn environment_name(name: &str) -> Result<(), ValidationError> {
    let mut characters = name.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid {
        return Err(ValidationError::EnvironmentName {
            field: ENVIRONMENT.to_string(),
            name: name.to_string(),
        });
    }
    Ok(())
}

pub fn header_name(name: &str) -> Result<(), ValidationError> {
    let header =
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| ValidationError::HeaderName {
            field: HEADERS.to_string(),
            name: name.to_string(),
        })?;
    if reserved(header.as_str()) {
        return Err(ValidationError::ReservedHeader {
            field: HEADERS.to_string(),
            name: name.to_string(),
        });
    }
    Ok(())
}

pub fn header_value(value: &str) -> Result<(), ValidationError> {
    HeaderValue::from_str(value)
        .map(|_| ())
        .map_err(|_| ValidationError::HeaderValue {
            field: HEADERS.to_string(),
        })
}

pub fn headers<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<(), ValidationError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(ValidationError::DuplicateHeader {
                field: HEADERS.to_string(),
                name: name.to_string(),
            });
        }
    }
    Ok(())
}

pub fn tools(tools: &[String]) -> Result<(), ValidationError> {
    match tools.iter().find(|tool| !nameable_tool(tool)) {
        Some(tool) => Err(ValidationError::Tool {
            field: TOOLS.to_string(),
            name: tool.clone(),
        }),
        None => Ok(()),
    }
}

pub fn target(
    transport: Transport,
    command: Option<&str>,
    url: Option<&str>,
) -> Result<(), ValidationError> {
    let command = command.filter(|command| !command.trim().is_empty());
    let url = url.filter(|url| !url.trim().is_empty());
    let (required, required_value, forbidden, forbidden_value) = match transport {
        Transport::Sse => {
            return Err(ValidationError::Unsupported {
                field: TRANSPORT.to_string(),
            });
        }
        Transport::Stdio => (COMMAND, command, URL, url),
        Transport::Http => (URL, url, COMMAND, command),
    };
    if required_value.is_none() {
        return Err(ValidationError::Missing {
            field: required.to_string(),
            transport,
        });
    }
    if forbidden_value.is_some() {
        return Err(ValidationError::Unexpected {
            field: forbidden.to_string(),
            transport,
        });
    }
    if command.is_some_and(|command| command != command.trim()) {
        return Err(ValidationError::Whitespace {
            field: COMMAND.to_string(),
        });
    }
    Ok(())
}

fn nameable_tool(tool: &str) -> bool {
    McpServer::default().with_tools([tool]).nameable(PROBE)
}

fn reserved(header: &str) -> bool {
    RESERVED.contains(&header) || header.starts_with(RESERVED_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_list(tools: &[&str]) -> Vec<String> {
        tools.iter().map(|tool| (*tool).to_string()).collect()
    }

    #[test]
    fn url_allows_private_loopback_and_public_hosts() {
        for raw in [
            "http://127.0.0.1:8080/mcp",
            "http://192.168.1.5/mcp",
            "http://localhost/mcp?x=1",
            "https://example.com/mcp",
        ] {
            assert_eq!(url(raw).map(String::from), Ok(raw.to_string()), "{raw}");
        }
    }

    #[test]
    fn url_refuses_each_unsafe_shape_with_its_variant() {
        let field = || URL.to_string();
        let cases = [
            (
                "http://169.254.169.254/",
                ValidationError::Metadata { field: field() },
            ),
            (
                "http://[fe80::1]/",
                ValidationError::Metadata { field: field() },
            ),
            (
                "http://metadata.google.internal/",
                ValidationError::Metadata { field: field() },
            ),
            (
                "http://user:pw@h/",
                ValidationError::Userinfo { field: field() },
            ),
            ("https://h/#f", ValidationError::Fragment { field: field() }),
            ("ftp://h/", ValidationError::Scheme { field: field() }),
            ("not a url", ValidationError::Unparseable { field: field() }),
            (
                "file:///etc/passwd",
                ValidationError::Scheme { field: field() },
            ),
        ];
        for (raw, expected) in cases {
            assert_eq!(url(raw), Err(expected), "{raw}");
        }
    }

    #[test]
    fn url_refuses_a_placeholder_the_query_would_keep() {
        assert_eq!(
            url("https://example.com/mcp?token=${HOME}"),
            Err(ValidationError::Placeholder {
                field: URL.to_string()
            })
        );
    }

    #[test]
    fn url_refuses_a_placeholder_the_parser_would_assemble() {
        for raw in [
            "https://h/mcp?token=$\n{X}",
            "https://h/mcp?token=$\t{X}",
            "https://h/mcp?token=$\r{X}",
            "https://$\n{X}/mcp",
            "https://%24%7BX%7D/mcp",
            "https://%24{X}/mcp",
        ] {
            assert_eq!(
                url(raw),
                Err(ValidationError::Placeholder {
                    field: URL.to_string()
                }),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn url_accepts_a_placeholder_that_stays_encoded_in_what_is_stored() {
        for (raw, stored) in [
            ("https://h/%24%7BX%7D/mcp", "https://h/%24%7BX%7D/mcp"),
            (
                "https://h/mcp?token=%24%7BX%7D",
                "https://h/mcp?token=%24%7BX%7D",
            ),
            ("https://h/$\n{X}/mcp", "https://h/$%7BX%7D/mcp"),
        ] {
            assert_eq!(
                url(raw).map(String::from),
                Ok(stored.to_string()),
                "{raw:?}"
            );
            assert!(!stored.contains(PLACEHOLDER), "{stored}");
        }
    }

    #[test]
    fn url_is_normalised_before_the_metadata_check() {
        for raw in [
            "http://2852039166/",
            "http://0xa9.0xfe.0xa9.0xfe/",
            "http://0251.0376.0251.0376/",
            "http://169.254.43518/",
            "http://169.254.169.254./",
            "http://[::ffff:169.254.169.254]/",
            "http://METADATA.GOOGLE.INTERNAL/",
            "http://metadata.google.internal./",
            "http://%6d%65%74%61%64%61%74%61.google.internal/",
        ] {
            assert_eq!(
                url(raw),
                Err(ValidationError::Metadata {
                    field: URL.to_string()
                }),
                "{raw}"
            );
        }
        for spelling in ["2852039166", "0xa9.0xfe.0xa9.0xfe", "0251.0376.0251.0376"] {
            assert!(
                !metadata::is_host(spelling),
                "{spelling} is refused only once the parser has rewritten it"
            );
        }
    }

    #[test]
    fn url_without_a_host_does_not_parse() {
        for raw in ["http://", "https://:443/mcp", "http://?x=1", "https:///"] {
            assert_eq!(
                url(raw),
                Err(ValidationError::Unparseable {
                    field: URL.to_string()
                }),
                "{raw}"
            );
        }
    }

    #[test]
    fn a_null_character_is_refused_in_every_field() {
        for field in [COMMAND, "arguments", ENVIRONMENT, HEADERS, URL] {
            assert_eq!(
                literal(field, "before\0after"),
                Err(ValidationError::Null {
                    field: field.to_string()
                })
            );
        }
        for raw in [
            "https://h/mcp\0",
            "https://h/mcp?token=\0",
            "https://h\0/mcp",
        ] {
            assert_eq!(
                url(raw),
                Err(ValidationError::Null {
                    field: URL.to_string()
                }),
                "{raw:?}"
            );
        }
        assert_eq!(
            name("git\0hub"),
            Err(ValidationError::Name {
                field: NAME.to_string()
            })
        );
        assert_eq!(
            tools(&tool_list(&["sea\0rch"])),
            Err(ValidationError::Tool {
                field: TOOLS.to_string(),
                name: "sea\0rch".to_string()
            })
        );
        assert_eq!(
            environment_name("TO\0KEN"),
            Err(ValidationError::EnvironmentName {
                field: ENVIRONMENT.to_string(),
                name: "TO\0KEN".to_string()
            })
        );
        assert_eq!(
            header_name("X-\0Key"),
            Err(ValidationError::HeaderName {
                field: HEADERS.to_string(),
                name: "X-\0Key".to_string()
            })
        );
        assert_eq!(
            literal(COMMAND, "np\0x").unwrap_err().to_string(),
            "`command` must not contain a NUL character"
        );
    }

    #[test]
    fn literal_refuses_placeholders_that_expanded_would_fill_from_zone_environment() {
        let lookup = |variable: &str| (variable == "HOME").then(|| "/Users/zone".to_string());
        let server =
            McpServer::command("${HOME}", ["${HOME}"]).with_environment("TOKEN", "${HOME}");
        let expanded = server.expanded(&lookup);
        assert_eq!(expanded.command.as_deref(), Some("/Users/zone"));
        assert_eq!(expanded.arguments, ["/Users/zone"]);
        assert_eq!(expanded.environment["TOKEN"].expose(), "/Users/zone");

        for field in [
            "command",
            "arguments",
            "environment.TOKEN",
            "headers.Authorization",
            "url",
        ] {
            assert_eq!(
                literal(field, "${HOME}"),
                Err(ValidationError::Placeholder {
                    field: field.to_string()
                })
            );
            assert_eq!(
                literal(field, "prefix-${HOME:-x}-suffix"),
                Err(ValidationError::Placeholder {
                    field: field.to_string()
                })
            );
        }
        assert_eq!(
            literal("command", "${HOME}").unwrap_err().to_string(),
            "`command` must not contain `${`: Zone would expand it from its own environment"
        );
        assert_eq!(literal("command", "$HOME {x} npx"), Ok(()));
    }

    #[test]
    fn name_refuses_names_a_cli_rule_could_misread() {
        for raw in ["a__b", "a_", "a.b", "a b", ""] {
            assert_eq!(
                name(raw),
                Err(ValidationError::Name {
                    field: NAME.to_string()
                }),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn name_caps_length_at_the_limit() {
        assert_eq!(name(&"a".repeat(NAME_LIMIT)), Ok(()));
        assert_eq!(
            name(&"a".repeat(NAME_LIMIT + 1)),
            Err(ValidationError::Length {
                field: NAME.to_string(),
                limit: NAME_LIMIT
            })
        );
        assert_eq!(name("github-tools_2"), Ok(()));
    }

    #[test]
    fn environment_name_follows_shell_rules() {
        for valid in ["TOKEN", "_x", "a1_B"] {
            assert_eq!(environment_name(valid), Ok(()), "{valid}");
        }
        for invalid in ["", "1TOKEN", "TO-KEN", "TO KEN", "TOKEN="] {
            assert_eq!(
                environment_name(invalid),
                Err(ValidationError::EnvironmentName {
                    field: ENVIRONMENT.to_string(),
                    name: invalid.to_string()
                }),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn header_name_follows_http_rules() {
        assert_eq!(header_name("Authorization"), Ok(()));
        assert_eq!(header_name("x-api-key"), Ok(()));
        for invalid in ["", "Bad Header", "Bad:Header", "Bad\nHeader"] {
            assert_eq!(
                header_name(invalid),
                Err(ValidationError::HeaderName {
                    field: HEADERS.to_string(),
                    name: invalid.to_string()
                }),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn header_name_refuses_what_the_connection_sets_itself_in_any_case() {
        for name in [
            "Host",
            "HOST",
            "content-length",
            "Content-Type",
            "Transfer-Encoding",
            "Connection",
            "Keep-Alive",
            "Proxy-Authorization",
            "Proxy-Connection",
            "TE",
            "Trailer",
            "Upgrade",
            "Accept",
            "Mcp-Session-Id",
            "MCP-Protocol-Version",
            "Last-Event-Id",
            "Mcp-Method",
            "Mcp-Name",
            "Mcp-Param-Region",
        ] {
            assert_eq!(
                header_name(name),
                Err(ValidationError::ReservedHeader {
                    field: HEADERS.to_string(),
                    name: name.to_string()
                }),
                "{name}"
            );
        }
        for name in [
            "Authorization",
            "Cookie",
            "User-Agent",
            "Accept-Language",
            "Mcp-Tenant",
        ] {
            assert_eq!(header_name(name), Ok(()), "{name}");
        }
        assert_eq!(
            header_name("Host").unwrap_err().to_string(),
            "`headers` name `Host` is set by the connection itself and cannot be configured"
        );
    }

    #[test]
    fn a_metadata_header_is_reserved_whatever_its_case() {
        for name in [
            "Metadata-Flavor",
            "METADATA",
            "X-aws-ec2-metadata-token",
            "X-Aws-Ec2-Metadata-Token-Ttl-Seconds",
            "X-Google-Metadata-Request",
            "x-aliyun-ecs-metadata-token",
            "X-Aliyun-Ecs-Metadata-Token-TTL-Seconds",
        ] {
            assert_eq!(
                header_name(name),
                Err(ValidationError::ReservedHeader {
                    field: HEADERS.to_string(),
                    name: name.to_string()
                }),
                "{name}"
            );
        }
        for name in ["X-Metadata-Note", "Authorization"] {
            assert_eq!(header_name(name), Ok(()), "{name}");
        }
    }

    #[test]
    fn every_reserved_header_is_spelled_the_way_a_parsed_name_is() {
        for name in RESERVED.into_iter().chain([RESERVED_PREFIX]) {
            let header = HeaderName::from_bytes(name.as_bytes()).expect("a header name");
            assert_eq!(header.as_str(), name);
        }
        assert!(RESERVED.is_sorted(), "{RESERVED:?}");
    }

    #[test]
    fn headers_refuses_two_names_that_differ_only_by_case() {
        assert_eq!(headers(["Authorization", "X-Api-Key"]), Ok(()));
        assert_eq!(headers([]), Ok(()));
        assert_eq!(
            headers(["Authorization", "X-Api-Key", "authorization"]),
            Err(ValidationError::DuplicateHeader {
                field: HEADERS.to_string(),
                name: "authorization".to_string()
            })
        );
        assert_eq!(
            headers(["A", "a"]).unwrap_err().to_string(),
            "`headers` names `a` twice: header names ignore case"
        );
    }

    #[test]
    fn a_header_value_holds_no_line_break_or_control_character() {
        for value in ["Bearer token", "tab\tseparated", "", "caf\u{e9}"] {
            assert_eq!(header_value(value), Ok(()), "{value:?}");
            assert_eq!(literal(HEADERS, value), Ok(()), "{value:?}");
        }
        for value in [
            "a\r\nInjected: 1",
            "a\nb",
            "a\rb",
            "bell\u{7}",
            "delete\u{7f}",
        ] {
            let refused = Err(ValidationError::HeaderValue {
                field: HEADERS.to_string(),
            });
            assert_eq!(header_value(value), refused, "{value:?}");
            assert_eq!(literal(HEADERS, value), refused, "{value:?}");
        }
        assert_eq!(literal(ENVIRONMENT, "-----BEGIN KEY-----\nabc\n"), Ok(()));
        assert_eq!(literal("arguments", "first line\nsecond line"), Ok(()));
        let error = header_value("secret\nvalue").unwrap_err().to_string();
        assert_eq!(
            error,
            "`headers` values must not contain line breaks or other control characters"
        );
    }

    #[test]
    fn tools_refuses_the_first_unsafe_entry() {
        assert_eq!(tools(&tool_list(&["ok", "also-ok_2"])), Ok(()));
        assert_eq!(tools(&[]), Ok(()));
        assert_eq!(
            tools(&tool_list(&["ok", "bad name"])),
            Err(ValidationError::Tool {
                field: TOOLS.to_string(),
                name: "bad name".to_string()
            })
        );
        assert_eq!(
            tools(&tool_list(&[""])),
            Err(ValidationError::Tool {
                field: TOOLS.to_string(),
                name: String::new()
            })
        );
        assert_eq!(
            tools(&tool_list(&["a,b"])),
            Err(ValidationError::Tool {
                field: TOOLS.to_string(),
                name: "a,b".to_string()
            })
        );
    }

    #[test]
    fn target_matches_each_transport_with_its_field() {
        assert_eq!(target(Transport::Stdio, Some("npx"), None), Ok(()));
        assert_eq!(
            target(Transport::Http, None, Some("https://example.com/mcp")),
            Ok(())
        );
        assert_eq!(
            target(Transport::Stdio, None, None),
            Err(ValidationError::Missing {
                field: COMMAND.to_string(),
                transport: Transport::Stdio
            })
        );
        assert_eq!(
            target(Transport::Stdio, Some("  "), None),
            Err(ValidationError::Missing {
                field: COMMAND.to_string(),
                transport: Transport::Stdio
            })
        );
        assert_eq!(
            target(
                Transport::Stdio,
                Some("npx"),
                Some("https://example.com/mcp")
            ),
            Err(ValidationError::Unexpected {
                field: URL.to_string(),
                transport: Transport::Stdio
            })
        );
        assert_eq!(
            target(Transport::Http, Some("npx"), None),
            Err(ValidationError::Missing {
                field: URL.to_string(),
                transport: Transport::Http
            })
        );
        assert_eq!(
            target(
                Transport::Http,
                Some("npx"),
                Some("https://example.com/mcp")
            ),
            Err(ValidationError::Unexpected {
                field: COMMAND.to_string(),
                transport: Transport::Http
            })
        );
        assert_eq!(
            target(Transport::Http, Some("npx"), Some("u"))
                .unwrap_err()
                .to_string(),
            "`command` is not allowed for `http` servers"
        );
    }

    #[test]
    fn target_refuses_a_command_with_surrounding_whitespace() {
        for command in [" npx", "npx ", "\tnpx", "npx\n", " npx "] {
            assert_eq!(
                target(Transport::Stdio, Some(command), None),
                Err(ValidationError::Whitespace {
                    field: COMMAND.to_string()
                }),
                "{command:?}"
            );
        }
        assert_eq!(
            target(Transport::Stdio, Some("/opt/my tools/server"), None),
            Ok(())
        );
        assert_eq!(
            target(Transport::Stdio, Some(" npx"), None)
                .unwrap_err()
                .to_string(),
            "`command` must not start or end with whitespace"
        );
    }

    #[test]
    fn target_refuses_sse() {
        for (command, url) in [
            (None, Some("https://example.com/sse")),
            (Some("npx"), None),
            (None, None),
        ] {
            assert_eq!(
                target(Transport::Sse, command, url),
                Err(ValidationError::Unsupported {
                    field: TRANSPORT.to_string()
                })
            );
        }
        assert_eq!(
            target(Transport::Sse, None, None).unwrap_err().to_string(),
            "SSE servers are not supported; use streamable HTTP (`http`)"
        );
    }
}
