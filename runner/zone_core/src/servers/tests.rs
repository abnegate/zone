use std::env;
use std::fs;
use std::iter;
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;
use zone_routing::PROXY_URL_VARIABLE;

use super::recording::PROXY;
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
fn a_url_only_config_still_gets_the_magents_fallback() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);
    variables.set(
        SERVERS,
        r#"{"mcpServers": {"remote": {"url": "http://localhost:3000/mcp"}}}"#,
    );

    assert_eq!(names(&config()), [MAGENTS]);
}

#[test]
fn a_config_whose_only_server_is_disabled_gets_the_magents_fallback() {
    let home = TempDir::new().unwrap();
    let mut variables = isolated(&home);

    for document in [
        r#"{"mcpServers": {"docs": {"command": "docs-server", "disabled": true}}}"#,
        r#"{"mcpServers": {"magents": {"command": "magents", "args": ["mcp"], "disabled": true}}}"#,
    ] {
        variables.set(SERVERS, document);

        let config = config();

        assert_eq!(names(&config), [MAGENTS], "{document}");
        assert!(!config.servers[MAGENTS].disabled, "{document}");
    }
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
