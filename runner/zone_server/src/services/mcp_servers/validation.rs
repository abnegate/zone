use abnegate_agent::mcp::McpServer;
use abnegate_http::metadata;
use reqwest::Url;
use reqwest::header::HeaderName;

use crate::services::mcp_servers::status;
use crate::services::mcp_servers::transport::Transport;

pub const NAME_LIMIT: usize = 32;

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
    #[error("`{field}` must include a host")]
    Host { field: String },
    #[error("`{field}` must not include a username or password; put credentials in headers")]
    Userinfo { field: String },
    #[error("`{field}` must not include a fragment")]
    Fragment { field: String },
    #[error("`{field}` must not point at a link-local or cloud metadata address")]
    Metadata { field: String },
    #[error("`{field}` must not contain `${{`: Zone would expand it from its own environment")]
    Placeholder { field: String },
    #[error(
        "`{field}` name `{name}` must start with a letter or `_` and hold only letters, digits and `_`"
    )]
    EnvironmentName { field: String, name: String },
    #[error("`{field}` name `{name}` is not a valid HTTP header name")]
    HeaderName { field: String, name: String },
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
    if !SCHEMES.contains(&url.scheme()) {
        return Err(ValidationError::Scheme { field: field() });
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(ValidationError::Host { field: field() });
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
    HeaderName::from_bytes(name.as_bytes())
        .map(|_| ())
        .map_err(|_| ValidationError::HeaderName {
            field: HEADERS.to_string(),
            name: name.to_string(),
        })
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
    Ok(())
}

fn nameable_tool(tool: &str) -> bool {
    McpServer::default().with_tools([tool]).nameable(PROBE)
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
