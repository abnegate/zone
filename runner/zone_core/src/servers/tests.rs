use std::env;
use std::fs;
use std::iter;
use std::os::unix::fs::PermissionsExt;

use abnegate_agent::mcp::McpEndpoint;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use zone_routing::PROXY_URL_VARIABLE;

use super::recording::PROXY;
use super::recording::RECORDER;
use super::recording::assert_routed;
use super::recording::document;
use super::*;
use crate::commands;
use crate::commands::Shell;
use crate::variables::Variables;

const ENABLED: &str = "ZONE_MCP_ENABLED";
const SERVERS: &str = "ZONE_MCP_SERVERS";
const CONFIG: &str = "ZONE_MCP_CONFIG";
const AUTO_CONNECT: &str = "ZONE_MCP_AUTO_CONNECT";
const AUTO_MAGENTS: &str = "ZONE_MCP_AUTO_MAGENTS";
const CREDENTIAL: &str = "ZONE_SERVERS_TEST_CREDENTIAL";

const UNBOUND: [(&str, &str); 0] = [];

const PLAYBOOK_MARKER: &str = "untrusted inert history";
const DELEGATION_MARKER: &str = "would mean reading across several files";
const LAUNDERING_MARKER: &str = "Permission does not travel between sessions";

/// The process environment with no `ZONE_MCP_*` variable, `HOME` at `home`,
/// and a stand-in `magents` first on `PATH`.
fn isolated(home: &TempDir) -> Variables {
    let mut variables =
        Variables::isolated(&[ENABLED, SERVERS, CONFIG, AUTO_CONNECT, AUTO_MAGENTS]);
    let bin = home.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let magents = bin.join(MAGENTS);
    fs::write(&magents, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&magents, fs::Permissions::from_mode(0o755)).unwrap();
    let path = env::var_os("PATH").unwrap_or_default();
    let path = env::join_paths(iter::once(bin).chain(env::split_paths(&path))).unwrap();
    variables.set("PATH", path);
    variables.set("HOME", home.path());
    variables
}

fn names(config: &McpConfig) -> Vec<&str> {
    config.servers.keys().map(String::as_str).collect()
}

fn text(names: &[&str]) -> String {
    prompt(1, names).expect("guidance for a connected server")
}

#[test]
fn zone_mcp_variables_are_read() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    let file = home.path().join("file.json");
    fs::write(
        &file,
        r#"{"mcpServers": {"file": {"command": "file-server"}}}"#,
    )
    .unwrap();
    fs::create_dir(home.path().join(".zone")).unwrap();
    fs::write(
        home.path().join(".zone").join("mcp.json"),
        r#"{"home": {"command": "home-server"}}"#,
    )
    .unwrap();
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"inline": {"command": "inline-server"}}}"#,
    );
    variables.set(CONFIG, &file);

    assert_eq!(names(&config()), ["inline"]);
    variables.remove(SERVERS);
    assert_eq!(names(&config()), ["file"]);
    variables.remove(CONFIG);
    assert_eq!(names(&config()), ["home"]);
}

#[test]
fn mcp_disabled_attaches_nothing_even_with_auto_magents() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(ENABLED, "false");
    variables.set(AUTO_MAGENTS, "true");

    let config = config();

    assert!(config.servers.is_empty(), "{:?}", names(&config));
    assert!(!config.auto_connect);
}

#[test]
fn the_master_switch_is_off_only_for_a_value_that_says_so() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);

    for (value, on) in [
        ("false", false),
        ("0", false),
        (" OFF ", false),
        ("no", false),
        ("true", true),
        ("", true),
        ("maybe", true),
    ] {
        variables.set(ENABLED, value);

        assert_eq!(config().auto_connect, on, "{value:?}");
        assert_eq!(config().servers.is_empty(), !on, "{value:?}");
    }
}

#[test]
fn auto_connect_off_attaches_nothing_even_with_auto_magents() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(AUTO_CONNECT, "false");
    variables.set(AUTO_MAGENTS, "true");

    let config = config();

    assert!(config.servers.is_empty(), "{:?}", names(&config));
    assert!(!config.auto_connect);
}

#[test]
fn auto_magents_off_stops_the_fallback() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(AUTO_MAGENTS, "off");
    variables.set(AUTO_CONNECT, "true");

    let config = config();

    assert!(config.servers.is_empty(), "{:?}", names(&config));
    assert!(!config.auto_connect);
}

#[test]
fn the_fallback_is_magents_mcp() {
    let home = TempDir::new().unwrap();
    let _variables = isolated(&home);

    let config = config();

    assert_eq!(names(&config), [MAGENTS]);
    let magents = &config.servers[MAGENTS];
    assert_eq!(magents.command.as_deref(), Some(MAGENTS));
    assert_eq!(magents.arguments, ["mcp"]);
    assert!(!magents.disabled);
}

#[test]
fn configured_servers_and_the_fallback_inherit_the_environment() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {
            "docs": {"command": "docs-server", "env": {"DOCS_TOKEN": "x"}},
            "notes": {"command": "notes-server", "args": ["mcp"]}
        }}"#,
    );

    let configured = config();
    assert_eq!(names(&configured), ["docs", "notes"]);
    assert!(
        configured
            .servers
            .values()
            .all(|server| server.inherit_environment)
    );
    assert!(
        configured.servers["docs"]
            .environment
            .contains_key("DOCS_TOKEN")
    );

    variables.remove(SERVERS);
    let fallback = config();
    assert_eq!(names(&fallback), [MAGENTS]);
    assert!(fallback.servers[MAGENTS].inherit_environment);
}

#[test]
fn a_url_server_stays_configured() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"remote": {"url": "http://localhost:3000/mcp"}}}"#,
    );

    let config = config();

    assert_eq!(names(&config), ["remote"]);
    let remote = &config.servers["remote"];
    assert_eq!(remote.url.as_deref(), Some("http://localhost:3000/mcp"));
    assert_eq!(remote.command, None);
    assert!(!remote.inherit_environment);
    assert!(remote.valid());
}

#[test]
fn a_url_only_config_counts_as_configured() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"remote": {"type": "http", "url": "http://localhost:3000/mcp"}}}"#,
    );

    let config = config();

    assert!(config.auto_connect);
    assert_eq!(names(&config), ["remote"]);
}

#[test]
fn an_sse_only_config_counts_as_configured() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"remote": {"type": "sse", "url": "http://localhost:3000/sse"}}}"#,
    );

    let config = config();

    assert!(config.auto_connect);
    assert_eq!(names(&config), ["remote"]);
    assert_eq!(config.servers["remote"].transport, Some(McpTransport::Sse));
}

#[test]
fn a_server_the_hub_cannot_attach_stays_configured_and_unbound() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"remote": {"type": "ws", "url": "ws://localhost:3000/mcp"}}}"#,
    );

    let config = config();

    assert_eq!(names(&config), ["remote"]);
    let remote = &config.servers["remote"];
    assert!(!remote.valid());
    assert_eq!(bound(remote), UNBOUND);
}

#[test]
fn a_disabled_url_server_is_dropped() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);

    for (document, expected) in [
        (
            r#"{"mcpServers": {"remote": {"url": "http://localhost:3000/mcp", "disabled": true}}}"#,
            MAGENTS,
        ),
        (
            r#"{"mcpServers": {"remote": {"url": "http://localhost:3000/mcp", "disabled": true}, "docs": {"command": "docs-server"}}}"#,
            "docs",
        ),
    ] {
        variables.set(SERVERS, document);

        assert_eq!(names(&config()), [expected], "{document}");
    }
}

#[test]
fn the_environment_feeds_an_instance_url_server() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        json!({"mcpServers": {"remote": {
            "url": "http://127.0.0.1:3000/mcp",
            "headers": {"Authorization": format!("Bearer ${{{CREDENTIAL}}}")}
        }}})
        .to_string(),
    );
    variables.set(CREDENTIAL, "token-9f3a7b");

    let set = config();
    variables.remove(CREDENTIAL);
    let unset = config();

    let remote = &set.servers["remote"];
    assert_eq!(remote.url.as_deref(), Some("http://127.0.0.1:3000/mcp"));
    assert_eq!(
        remote.headers["Authorization"].expose(),
        format!("Bearer ${{{CREDENTIAL}}}")
    );
    let endpoint = remote.endpoint().expect("the environment binds the token");
    assert_eq!(endpoint.url, "http://127.0.0.1:3000/mcp");
    assert_eq!(sent(&endpoint), [("Authorization", "Bearer token-9f3a7b")]);
    assert_eq!(
        bound(remote),
        [
            (CREDENTIAL, "token-9f3a7b"),
            ("header credential Authorization", "token-9f3a7b"),
            ("header value Authorization", "Bearer token-9f3a7b"),
            ("path segment 0", "mcp"),
            ("url path", "/mcp"),
        ]
    );
    assert_eq!(names(&unset), ["remote"]);
    assert_eq!(unset.servers["remote"].endpoint(), None);
    assert_eq!(bound(&unset.servers["remote"]), UNBOUND);
}

#[test]
fn a_command_server_is_never_bound() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    let argument = format!("--token=${{{CREDENTIAL}}}");
    variables.set(
        SERVERS,
        json!({"mcpServers": {"docs": {
            "command": "docs-server",
            "args": [argument],
            "url": format!("http://localhost:3000/${{{CREDENTIAL}}}")
        }}})
        .to_string(),
    );
    variables.set(CREDENTIAL, "token-9f3a7b");

    let config = config();

    let docs = &config.servers["docs"];
    assert_eq!(docs.arguments, [argument]);
    assert_eq!(bound(docs), UNBOUND);
}

#[test]
fn references_in_a_url_and_headers_are_bound_from_the_lookup() {
    let written = McpServer::remote("https://${HOST:-127.0.0.1}/mcp/${PATH_KEY}")
        .with_header("Authorization", "Bearer ${TOKEN}");
    let resolved = written
        .endpoint_with(&lookup)
        .expect("the lookup or a default answers every reference");
    let mut server = written.clone();

    assert!(bind(&mut server, &lookup));

    assert_eq!(server.url, written.url);
    assert_eq!(server.headers, written.headers);
    assert_eq!(
        bound(&server),
        [
            ("PATH_KEY", "path-key-51c2"),
            ("TOKEN", "token-9f3a7b"),
            ("header credential Authorization", "token-9f3a7b"),
            ("header value Authorization", "Bearer token-9f3a7b"),
            ("path segment 0", "mcp"),
            ("path segment 1", "path-key-51c2"),
            ("url path", "/mcp/path-key-51c2"),
        ]
    );
    assert_eq!(resolved.url, "https://127.0.0.1/mcp/path-key-51c2");
    assert_eq!(sent(&resolved), [("Authorization", "Bearer token-9f3a7b")]);
    assert_eq!(server.endpoint(), Some(resolved));
}

#[test]
fn an_unset_reference_binds_nothing() {
    let mut server = McpServer::remote("https://example.com/mcp/${PATH_KEY}")
        .with_header("Authorization", "Bearer ${MISSING}");

    assert!(!bind(&mut server, &lookup));

    assert_eq!(server.endpoint(), None);
    assert_eq!(bound(&server), UNBOUND);
}

#[test]
fn a_literal_header_value_is_masked() {
    let mut server =
        McpServer::remote("https://example.com/mcp").with_header("X-Key", "literal-key-7d21");

    mask(&mut server);

    assert_eq!(
        bound(&server),
        [
            ("header value X-Key", "literal-key-7d21"),
            ("path segment 0", "mcp"),
            ("url path", "/mcp"),
        ]
    );
    assert!(server.secrets.keys().all(|key| spaced(key)));
}

#[test]
fn a_two_part_header_value_also_masks_its_credential() {
    let mut server = McpServer::remote("https://example.com")
        .with_header("Authorization", "Bearer credential-4e8b")
        .with_header("X-Padded", " padded-0c6d ");

    mask(&mut server);

    assert_eq!(
        bound(&server),
        [
            ("header credential Authorization", "credential-4e8b"),
            ("header value Authorization", "Bearer credential-4e8b"),
            ("header value X-Padded", " padded-0c6d "),
            ("url path", "/"),
        ]
    );
}

#[test]
fn a_urls_path_and_query_are_masked() {
    let mut queried = McpServer::remote(
        "https://example.com/mcp/segment-a1b2c3/v1/?key=value-d4e5f6&empty=&mode=fast",
    );
    let mut plain = McpServer::remote("https://example.com/mcp");

    mask(&mut queried);
    mask(&mut plain);

    assert_eq!(
        bound(&queried),
        [
            ("path segment 0", "mcp"),
            ("path segment 1", "segment-a1b2c3"),
            ("path segment 2", "v1"),
            ("query value 0", "value-d4e5f6"),
            ("query value 2", "fast"),
            ("url path", "/mcp/segment-a1b2c3/v1/"),
            ("url query", "key=value-d4e5f6&empty=&mode=fast"),
        ]
    );
    assert_eq!(
        bound(&plain),
        [("path segment 0", "mcp"), ("url path", "/mcp")]
    );
}

#[test]
fn an_encoded_piece_of_a_url_is_masked_as_the_server_reads_it() {
    let mut server = McpServer::remote(
        "https://example.com/open%20sesame%2F9c1e/100%/%E2%9C%93%zz?note=a%20b+c%26d",
    );

    mask(&mut server);

    assert_eq!(
        bound(&server),
        [
            ("path segment 0", "open sesame/9c1e"),
            ("path segment 1", "100%"),
            ("path segment 2", "\u{2713}%zz"),
            ("query value 0", "a b c&d"),
            ("url path", "/open%20sesame%2F9c1e/100%/%E2%9C%93%zz"),
            ("url query", "note=a%20b+c%26d"),
        ]
    );
}

#[test]
fn the_user_info_a_url_carries_is_masked() {
    let mut credentialed =
        McpServer::remote("https://operator-5b2e:hunter2%40home%2Foffice@example.com/mcp");
    let mut named = McpServer::remote("https://operator-5b2e@example.com/mcp");
    let mut keyed = McpServer::remote("https://:password-3b9d@example.com/mcp");
    let resolved = credentialed.endpoint();

    mask(&mut credentialed);
    mask(&mut named);
    mask(&mut keyed);
    let once = credentialed.secrets.clone();
    mask(&mut credentialed);

    assert_eq!(
        bound(&credentialed),
        [
            ("path segment 0", "mcp"),
            ("url password", "hunter2@home/office"),
            ("url path", "/mcp"),
            ("url username", "operator-5b2e"),
        ]
    );
    assert_eq!(
        bound(&named),
        [
            ("path segment 0", "mcp"),
            ("url path", "/mcp"),
            ("url username", "operator-5b2e"),
        ]
    );
    assert_eq!(
        bound(&keyed),
        [
            ("path segment 0", "mcp"),
            ("url password", "password-3b9d"),
            ("url path", "/mcp"),
        ]
    );
    assert_eq!(credentialed.secrets, once);
    assert_eq!(credentialed.endpoint(), resolved);
    assert!(credentialed.secrets.keys().all(|key| spaced(key)));
}

#[test]
fn a_header_named_like_its_own_reference_resolves_unchanged() {
    let written =
        McpServer::remote("https://example.com/mcp").with_header("TOKEN", "Bearer ${TOKEN}");
    let resolved = written
        .endpoint_with(&lookup)
        .expect("the lookup answers the reference");
    let mut server = written.clone();

    assert!(bind(&mut server, &lookup));

    assert_eq!(
        bound(&server),
        [
            ("TOKEN", "token-9f3a7b"),
            ("header credential TOKEN", "token-9f3a7b"),
            ("header value TOKEN", "Bearer token-9f3a7b"),
            ("path segment 0", "mcp"),
            ("url path", "/mcp"),
        ]
    );
    assert!(
        server
            .secrets
            .keys()
            .filter(|key| *key != "TOKEN")
            .all(|key| spaced(key))
    );
    assert_eq!(sent(&resolved), [("TOKEN", "Bearer token-9f3a7b")]);
    assert_eq!(server.endpoint(), Some(resolved));
}

#[test]
fn masking_is_idempotent() {
    let mut server = McpServer::remote("https://example.com/mcp/${PATH_KEY}?key=value-d4e5f6")
        .with_header("Authorization", "Bearer ${TOKEN}")
        .with_header("X-Key", "literal-key-7d21");
    assert!(bind(&mut server, &lookup));
    let once = server.secrets.clone();
    let resolved = server.endpoint();

    mask(&mut server);

    assert_eq!(server.secrets, once);
    assert_eq!(server.endpoint(), resolved);
    assert_eq!(once.len(), 10);
}

#[test]
fn a_server_with_no_endpoint_is_left_unmasked() {
    let mut command = McpServer::command("docs-server", ["--token=token-9f3a7b"]);
    let mut unbound = McpServer::remote("https://example.com/mcp/${PATH_KEY}")
        .with_header("X-Key", "literal-key-7d21");

    mask(&mut command);
    mask(&mut unbound);

    assert_eq!(bound(&command), UNBOUND);
    assert_eq!(bound(&unbound), UNBOUND);
}

fn lookup(variable: &str) -> Option<String> {
    match variable {
        "PATH_KEY" => Some("path-key-51c2".to_string()),
        "TOKEN" => Some("token-9f3a7b".to_string()),
        _ => None,
    }
}

fn bound(server: &McpServer) -> Vec<(&str, &str)> {
    server
        .secrets
        .iter()
        .map(|(key, value)| (key.as_str(), value.expose()))
        .collect()
}

fn sent(endpoint: &McpEndpoint) -> Vec<(&str, &str)> {
    endpoint
        .headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.expose()))
        .collect()
}

fn spaced(key: &str) -> bool {
    key.contains(' ')
}

#[test]
fn a_config_whose_only_server_is_disabled_gets_the_magents_fallback() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"docs": {"command": "docs-server", "disabled": true}}}"#,
    );

    let config = config();

    assert_eq!(names(&config), [MAGENTS]);
    assert!(!config.servers[MAGENTS].disabled);
}

#[test]
fn a_disabled_magents_server_stays_detached() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);

    for document in [
        r#"{"mcpServers": {"magents": {"command": "magents", "args": ["mcp"], "disabled": true}}}"#,
        r#"{"mcpServers": {"magents": {"url": "http://localhost:3000/mcp", "disabled": true}}}"#,
        r#"{"mcpServers": {"magents": {"command": "magents", "disabled": true}, "docs": {"command": "docs-server", "disabled": true}}}"#,
        r#"{"mcpServers": {"magents": {"command": "magents", "disabled": true}, "docs": {"command": ""}}}"#,
    ] {
        variables.set(SERVERS, document);

        let config = config();

        assert!(
            config.servers.is_empty(),
            "{document}: {:?}",
            names(&config)
        );
    }
}

#[test]
fn a_disabled_magents_server_leaves_the_other_servers_attached() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"magents": {"command": "magents", "disabled": true}, "docs": {"command": "docs-server"}}}"#,
    );

    assert_eq!(names(&config()), ["docs"]);
}

#[test]
fn a_server_with_a_command_and_a_url_launches_over_stdio() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"both": {"command": "docs-server", "url": "http://localhost:3000/mcp"}}}"#,
    );

    let config = config();

    assert_eq!(names(&config), ["both"]);
    assert_launched_over_stdio(&config.servers["both"]);
}

#[test]
fn a_server_with_a_command_and_any_type_launches_over_stdio() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);

    for transport in ["http", "sse", "ws"] {
        variables.set(
            SERVERS,
            json!({"mcpServers": {"typed": {"command": "docs-server", "type": transport}}})
                .to_string(),
        );

        let config = config();

        assert_eq!(names(&config), ["typed"], "{transport}");
        assert_launched_over_stdio(&config.servers["typed"]);
    }
}

#[test]
fn a_blank_command_discards_the_whole_document_for_the_magents_fallback() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);

    for document in [
        r#"{"mcpServers": {"blank": {"command": "  "}}}"#,
        r#"{"mcpServers": {"blank": {"command": ""}, "docs": {"command": "docs-server"}}}"#,
        r#"{"mcpServers": {"blank": {"command": " ", "disabled": true}, "docs": {"command": "docs-server"}}}"#,
        r#"{"mcpServers": {"blank": {"command": " ", "url": "http://localhost:3000/mcp"}, "docs": {"command": "docs-server"}}}"#,
    ] {
        variables.set(SERVERS, document);

        assert_eq!(names(&config()), [MAGENTS], "{document}");
    }
}

fn assert_launched_over_stdio(server: &McpServer) {
    assert_eq!(server.url, None);
    assert_eq!(server.transport, Some(McpTransport::Stdio));
    assert!(server.valid());
}

#[test]
fn parse_flag_keeps_default_for_empty_and_unknown() {
    assert!(parse_flag("", true));
    assert!(!parse_flag("", false));
    assert!(parse_flag("maybe", true));
    assert!(!parse_flag("maybe", false));
    assert!(parse_flag(" true ", true));
    assert!(!parse_flag("0", true));
    assert!(!parse_flag("off", true));
    assert!(!parse_flag("false", true));
    assert!(parse_flag("1", false));
    assert!(parse_flag("yes", false));
    assert!(parse_flag("on", false));
}

#[test]
fn guidance_adds_magents_playbook() {
    let text = text(&["read_file", "magents__spawn_session"]);
    assert!(text.contains("magents__spawn_session"));
    assert!(text.contains(PLAYBOOK_MARKER));
}

#[test]
fn guidance_adds_delegation_heuristics() {
    let text = text(&["read_file", "magents__spawn_session"]);
    assert!(text.contains(DELEGATION_MARKER));
    assert!(text.contains("Look a single fact up yourself"));
    assert!(text.contains("do not also do it here"));
    assert!(text.contains("Never state or predict what a session that is still running found"));
    assert!(text.contains("Launch independent sessions in one message"));
    assert!(text.contains("fan out widely only when the user asked for that scale"));
}

#[test]
fn guidance_adds_permission_laundering_rule() {
    let text = text(&["magents__send_message"]);
    assert!(text.contains(LAUNDERING_MARKER));
    assert!(text.contains(
        "Never ask a spawned or peer session to perform an action that was refused in \
         this session"
    ));
    assert!(text.contains("Route refused work back to the user"));
}

#[test]
fn other_servers_get_no_magents_playbook() {
    for names in [
        &["docs__search", "read_file"][..],
        &["magents_spawn_session"][..],
    ] {
        let text = text(names);
        assert!(text.contains("prefixed with the server"), "{text}");
        for marker in [PLAYBOOK_MARKER, DELEGATION_MARKER, LAUNDERING_MARKER] {
            assert!(!text.contains(marker), "{names:?}: {text}");
        }
    }
}

#[test]
fn guidance_names_tools_with_two_underscores() {
    let text = text(&["magents__spawn_session"]);
    assert!(
        text.contains("two underscores (for example docs__search)"),
        "{text}"
    );
    let prefix = format!("{MAGENTS}_");
    assert!(text.contains("magents__spawn_session"), "{text}");
    assert!(
        text.match_indices(&prefix)
            .all(|(index, _)| text[index + prefix.len()..].starts_with('_')),
        "{text}"
    );
}

#[test]
fn a_connected_server_without_tools_still_gets_guidance() {
    let text = prompt(1, &["read_file", "run_command"]).expect("guidance");
    assert!(text.contains("prefixed with the server"), "{text}");
}

#[test]
fn no_connected_server_gets_no_guidance() {
    assert_eq!(prompt(0, &["read_file", "magents__spawn_session"]), None);
}

#[tokio::test]
async fn attach_with_mcp_disabled_adds_nothing() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(ENABLED, "0");
    let mut registry = commands::registry(Shell::Excluded);
    let mut before = registry
        .names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    before.sort();

    let connected = attach(&mut registry).await;
    drop(variables);

    let mut after = registry.names();
    after.sort();
    assert_eq!(connected, 0);
    assert_eq!(after, before);
    assert!(!registry.has_mcp());
}

#[tokio::test]
async fn attached_servers_route_through_zones_proxy_over_their_own() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    let recorded = home.path().join("environment");
    variables.set(SERVERS, document(&recorded));
    variables.set(PROXY_URL_VARIABLE, PROXY);
    let mut registry = commands::registry(Shell::Excluded);

    let connected = attach(&mut registry).await;
    drop(variables);

    assert_eq!(connected, 0, "the recorder exits without answering");
    assert_routed(&recorded);
}

#[tokio::test]
async fn a_server_with_a_command_and_a_url_or_a_type_is_launched_as_a_command() {
    for (field, value) in [("url", "http://localhost:3000/mcp"), ("type", "http")] {
        let home = TempDir::new().unwrap();
        let mut variables = isolated(&home);
        let recorded = home.path().join("environment");
        let mut configured: Value = serde_json::from_str(&document(&recorded)).unwrap();
        configured["mcpServers"][RECORDER][field] = json!(value);
        variables.set(SERVERS, configured.to_string());
        variables.set(PROXY_URL_VARIABLE, PROXY);
        let mut registry = commands::registry(Shell::Excluded);

        attach(&mut registry).await;
        drop(variables);

        assert!(recorded.is_file(), "{field}: the recorder was not started");
        assert_routed(&recorded);
    }
}
