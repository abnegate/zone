//! The MCP servers a connected provider already has configured, for an admin to import.
//!
//! A server a person set up for a coding agent itself never reaches a Zone turn: claude runs with
//! `--strict-mcp-config` and codex is given its servers as `-c` overrides, so each run attaches
//! what Zone configured and nothing else. Importing such a server into the organization layer is
//! what makes its tools available, and this module is the reading half: it parses what each agent
//! keeps in the home Zone would point it at, in the shape the create route takes.
//!
//! Only a provider the organization is connected to is read. Nothing here connects to a server,
//! stores anything, or quotes a value out of a document: a value in the wrong place may still be
//! a secret, and the secrets a server does hold travel no further than the import that stores
//! them.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use abnegate_agent::mcp::McpConfig;
use abnegate_agent::mcp::McpServer;
use abnegate_agent::mcp::McpTransport;
use abnegate_agent_cli::AgentKind;
use abnegate_secret::SecretValue;
use serde_json::Map;
use serde_json::Value;
use uuid::Uuid;

use crate::db::agent_logins;
use crate::services::agent::home;
use crate::services::login::status::AgentStatus;
use crate::services::provider::Provider;
use crate::state::AppState;

use super::transport::Transport;

/// Where Claude Code keeps the servers a person configured for it: in the directory
/// `CLAUDE_CONFIG_DIR` names, or beside `~/.claude` when it names none.
const CLAUDE_DOCUMENT: &str = ".claude.json";
/// Claude Code's settings file, which may carry `mcpServers` as well.
const CLAUDE_SETTINGS: &str = "settings.json";
/// The directory Claude Code keeps its state in with `CLAUDE_CONFIG_DIR` unset.
const CLAUDE_DIRECTORY: &str = ".claude";
/// Codex keeps everything, its servers included, in its home.
const CODEX_DOCUMENT: &str = "config.toml";
/// The directory codex keeps its state in with `CODEX_HOME` unset.
const CODEX_DIRECTORY: &str = ".codex";

const HOME: &str = "HOME";

/// The keys each agent's document holds its servers under, in the order they are looked for.
const CLAUDE_KEYS: [&str; 2] = ["mcpServers", "servers"];
const CODEX_KEYS: [&str; 1] = ["mcp_servers"];

/// Codex turns a server off with `enabled`, Claude Code with `disabled`.
const ENABLED: &str = "enabled";
const DISABLED: &str = "disabled";

/// The keys an import carries into the organization layer. Anything else an entry holds —
/// codex's `bearer_token_env_var`, a `cwd`, a timeout, a Claude Code `timeout` — is reported as
/// not imported, so an admin can see what they will have to set up again themselves.
const CARRIED: [&str; 11] = [
    "command",
    "args",
    "arguments",
    "env",
    "environment",
    "url",
    "headers",
    "tools",
    "type",
    DISABLED,
    ENABLED,
];

const NOT_AN_ENTRY: &str = "the entry is not an object";
const UNREADABLE: &str = "the entry could not be read";
const NO_TARGET: &str = "the entry names neither a command nor a url";

/// One server a provider has configured, with the values an import would store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderServer {
    pub name: String,
    pub transport: Transport,
    pub command: Option<String>,
    pub arguments: Vec<String>,
    pub url: Option<String>,
    pub tools: Option<Vec<String>>,
    /// Kept for the import that stores them; the API answers with their names alone.
    pub environment: BTreeMap<String, SecretValue>,
    /// Kept for the import that stores them; the API answers with their names alone.
    pub headers: BTreeMap<String, SecretValue>,
    /// Whether the provider has the server turned on. An import stores it as it stands.
    pub enabled: bool,
    /// The keys of this entry that an import does not carry.
    pub ignored: Vec<String>,
}

/// What one connected provider has configured, and what could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderServers {
    pub servers: Vec<ProviderServer>,
    /// One sentence per document or entry that could not be read, naming the document by its file
    /// name and the server by the name the document gave it. Nothing else from a document is
    /// quoted.
    pub errors: Vec<String>,
}

/// What every provider `organization` is connected to has configured, in the order
/// [`Provider::ALL`] names them.
///
/// A provider Zone holds no sign-in for is left out: it runs no turn for this organization, and
/// its configuration is not Zone's to read.
pub async fn discovered(
    state: &AppState,
    organization: Uuid,
) -> Result<Vec<(Provider, ProviderServers)>, sqlx::Error> {
    let mut discovered = Vec::new();
    for provider in Provider::ALL {
        if let Some(servers) = of(state, organization, provider).await? {
            discovered.push((provider, servers));
        }
    }
    Ok(discovered)
}

/// What `provider` has configured for `organization`, or `None` when the organization is not
/// connected to it, and for every provider that is not a coding agent: nothing else configures
/// servers of its own.
pub async fn of(
    state: &AppState,
    organization: Uuid,
    provider: Provider,
) -> Result<Option<ProviderServers>, sqlx::Error> {
    let Some(agent) = provider.agent() else {
        return Ok(None);
    };
    if !AgentStatus::signed_in(state, organization, agent).await? {
        return Ok(None);
    }
    Ok(Some(read(&documents(state, organization, agent).await?)))
}

/// Every document the organization's turns of `agent` would read servers from, in the order a
/// name first found in one wins: one per login home Zone keeps for the agent, and, holding no
/// login of it, the host's own configuration when the instance allows the host's sign-in. That
/// is where [`crate::services::backend`] points the agent, so it is where the servers configured
/// for it are.
async fn documents(
    state: &AppState,
    organization: Uuid,
    agent: AgentKind,
) -> Result<Vec<PathBuf>, sqlx::Error> {
    let config = state.config();
    let logins = agent_logins::list_for(state.db(), organization, agent.as_str()).await?;
    if logins.is_empty() {
        return Ok(if config.agents.host_login {
            host_documents(agent)
        } else {
            Vec::new()
        });
    }
    Ok(logins
        .iter()
        .flat_map(|login| {
            in_directory(
                agent,
                &config.agents.login_home(organization, agent, login.id),
            )
        })
        .collect())
}

/// The documents of the host's own configuration of `agent`: those in the directory its
/// configuration variable names, or, with none, those in the directory it falls back to under
/// `HOME`. With neither named there is nowhere to look.
fn host_documents(agent: AgentKind) -> Vec<PathBuf> {
    let named = home::variable(agent)
        .and_then(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    if let Some(directory) = named {
        return in_directory(agent, &directory);
    }
    let Some(home) = std::env::var_os(HOME)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let mut documents = in_directory(agent, &home.join(directory(agent)));
    if agent == AgentKind::Claude {
        // With no `CLAUDE_CONFIG_DIR`, Claude Code keeps a person's own servers in
        // `~/.claude.json`, beside the directory rather than in it.
        documents.push(home.join(CLAUDE_DOCUMENT));
    }
    documents
}

/// The documents `agent` would read servers from in the home `directory`.
fn in_directory(agent: AgentKind, directory: &Path) -> Vec<PathBuf> {
    match agent {
        AgentKind::Claude => vec![
            directory.join(CLAUDE_DOCUMENT),
            directory.join(CLAUDE_SETTINGS),
        ],
        AgentKind::Codex => vec![directory.join(CODEX_DOCUMENT)],
        _ => Vec::new(),
    }
}

/// The directory `agent` keeps its state in when its configuration variable names none.
fn directory(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Codex => CODEX_DIRECTORY,
        _ => CLAUDE_DIRECTORY,
    }
}

/// Every server `documents` configure, the first entry under a name winning, with a sentence for
/// each document and entry that could not be read.
fn read(documents: &[PathBuf]) -> ProviderServers {
    let mut found = ProviderServers::default();
    for document in documents {
        let read = reading(document);
        found.errors.extend(read.errors);
        for server in read.servers {
            if found.servers.iter().any(|held| held.name == server.name) {
                continue;
            }
            found.servers.push(server);
        }
    }
    found
}

/// What one document configures. A document that is not there configures nothing, which is the
/// usual answer: an agent writes one only once it has state to keep.
fn reading(document: &Path) -> ProviderServers {
    let name = file_name(document);
    let text = match std::fs::read_to_string(document) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return ProviderServers::default(),
        Err(error) => return failed(format!("{name} could not be read: {error}")),
    };
    match entries(document, &text) {
        Ok(entries) => servers(&entries),
        Err(message) => failed(format!("{name}: {message}")),
    }
}

/// The entries `text` holds under the key its agent keeps servers at, keyed by server name, or
/// why it holds none that could be read. A document in the agent's own format that configures no
/// servers at all has no entries and is no error.
fn entries(document: &Path, text: &str) -> Result<Map<String, Value>, String> {
    let toml = document
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(std::ffi::OsStr::new("toml")));
    let (parsed, keys): (Value, &[&str]) = if toml {
        let document: toml::Value =
            toml::from_str(text).map_err(|_| "the file is not valid TOML".to_string())?;
        let parsed =
            serde_json::to_value(document).map_err(|_| "the file is not valid TOML".to_string())?;
        (parsed, &CODEX_KEYS)
    } else {
        let parsed: Value =
            serde_json::from_str(text).map_err(|_| "the file is not valid JSON".to_string())?;
        (parsed, &CLAUDE_KEYS)
    };
    for key in keys {
        let Some(servers) = parsed.get(key) else {
            continue;
        };
        return servers
            .as_object()
            .cloned()
            .ok_or_else(|| format!("`{key}` is not a map of servers"));
    }
    Ok(Map::new())
}

/// Each entry as a server, or a sentence naming the entry that could not be read as one.
fn servers(entries: &Map<String, Value>) -> ProviderServers {
    let mut found = ProviderServers::default();
    for (name, entry) in entries {
        match server(name, entry) {
            Ok(server) => found.servers.push(server),
            Err(message) => found.errors.push(format!("{name}: {message}")),
        }
    }
    found
}

/// The server `entry` describes under `name`, or why it describes none Zone could store.
///
/// The entry is read by the same crate that reads an instance document, so every spelling a
/// provider uses for a field is the spelling Zone already accepts, and a field neither reads is
/// reported as not imported rather than guessed at.
fn server(name: &str, entry: &Value) -> Result<ProviderServer, String> {
    let fields = entry.as_object().ok_or(NOT_AN_ENTRY)?;
    let document = Value::Object(Map::from_iter([(
        CLAUDE_KEYS[0].to_string(),
        Value::Object(Map::from_iter([(
            name.to_string(),
            Value::Object(normalized(fields)),
        )])),
    )]));
    let parsed = McpConfig::from_value(&document).map_err(|_| UNREADABLE.to_string())?;
    let server = parsed.servers.get(name).ok_or(UNREADABLE)?;
    Ok(ProviderServer {
        name: name.to_string(),
        transport: transport(server).ok_or(NO_TARGET)?,
        command: server.command.clone(),
        arguments: server.arguments.clone(),
        url: server.url.clone(),
        tools: (!server.tools.is_empty()).then(|| server.tools.clone()),
        environment: server.environment.clone(),
        headers: server.headers.clone(),
        enabled: !server.disabled,
        ignored: ignored(fields),
    })
}

/// How the entry is reached, or `None` when it names nothing to reach. A command and a URL
/// together is read as a command, which the create route then refuses for naming both, rather
/// than quietly dropping one of them.
fn transport(server: &McpServer) -> Option<Transport> {
    if server.command.is_some() {
        return Some(Transport::Stdio);
    }
    if server.transport == Some(McpTransport::Sse) {
        return Some(Transport::Sse);
    }
    server.url.is_some().then_some(Transport::Http)
}

/// `fields` with codex's `enabled = false` written as the `disabled` the reader understands.
/// Nothing else differs between the two agents' shapes.
fn normalized(fields: &Map<String, Value>) -> Map<String, Value> {
    let mut normalized = fields.clone();
    if normalized.get(ENABLED) == Some(&Value::Bool(false)) {
        normalized.insert(DISABLED.to_string(), Value::Bool(true));
    }
    normalized
}

/// The keys of `fields` an import does not carry.
fn ignored(fields: &Map<String, Value>) -> Vec<String> {
    fields
        .keys()
        .filter(|key| !CARRIED.contains(&key.as_str()))
        .cloned()
        .collect()
}

fn failed(message: String) -> ProviderServers {
    ProviderServers {
        servers: Vec::new(),
        errors: vec![message],
    }
}

/// A document's file name, which is what an error names it by: the path it sits at is the
/// operator's business and says where an agent's state lives.
fn file_name(document: &Path) -> String {
    document
        .file_name()
        .unwrap_or(document.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    const CLAUDE: &str = r#"{
        "numStartups": 12,
        "mcpServers": {
            "notes": {
                "command": "notes-server",
                "args": ["mcp"],
                "env": {"NOTES_TOKEN": "notes-secret"},
                "cwd": "/srv/notes",
                "timeout": 30
            },
            "docs": {
                "type": "http",
                "url": "https://mcp.example.com/mcp",
                "headers": {"Authorization": "Bearer docs-secret"}
            },
            "retired": {
                "command": "retired-server",
                "disabled": true
            },
            "broken": {
                "command": 7
            }
        }
    }"#;

    const CODEX: &str = r#"
model = "gpt-5"

[mcp_servers.grafana]
command = "grafana-mcp"
args = ["--transport", "stdio"]
env = { GRAFANA_TOKEN = "grafana-secret" }

[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
bearer_token_env_var = "LINEAR_TOKEN"

[mcp_servers.old]
command = "old-server"
enabled = false
"#;

    fn written(name: &str, text: &str) -> (TempDir, PathBuf) {
        let directory = TempDir::new().expect("a directory");
        let document = directory.path().join(name);
        fs::write(&document, text).expect("the document is written");
        (directory, document)
    }

    fn named<'servers>(servers: &'servers ProviderServers, name: &str) -> &'servers ProviderServer {
        servers
            .servers
            .iter()
            .find(|server| server.name == name)
            .unwrap_or_else(|| panic!("{name} was read"))
    }

    #[test]
    fn a_claude_document_reads_as_the_servers_it_configures() {
        let (_directory, document) = written(CLAUDE_DOCUMENT, CLAUDE);

        let read = reading(&document);

        assert_eq!(
            read.servers
                .iter()
                .map(|server| server.name.as_str())
                .collect::<Vec<&str>>(),
            ["docs", "notes", "retired"]
        );
        let notes = named(&read, "notes");
        assert_eq!(notes.transport, Transport::Stdio);
        assert_eq!(notes.command.as_deref(), Some("notes-server"));
        assert_eq!(notes.arguments, ["mcp"]);
        assert_eq!(
            notes.environment.keys().collect::<Vec<&String>>(),
            ["NOTES_TOKEN"]
        );
        assert!(notes.enabled);
        assert_eq!(notes.ignored, ["cwd", "timeout"]);
    }

    #[test]
    fn a_remote_entry_keeps_its_url_and_the_names_of_its_headers() {
        let (_directory, document) = written(CLAUDE_DOCUMENT, CLAUDE);

        let docs = reading(&document);
        let docs = named(&docs, "docs");

        assert_eq!(docs.transport, Transport::Http);
        assert_eq!(docs.url.as_deref(), Some("https://mcp.example.com/mcp"));
        assert_eq!(docs.command, None);
        assert_eq!(
            docs.headers.keys().collect::<Vec<&String>>(),
            ["Authorization"]
        );
        assert_eq!(docs.headers["Authorization"].expose(), "Bearer docs-secret");
    }

    #[test]
    fn an_entry_a_provider_turned_off_is_read_as_turned_off_by_either_spelling() {
        let (_claude, claude) = written(CLAUDE_DOCUMENT, CLAUDE);
        let (_codex, codex) = written(CODEX_DOCUMENT, CODEX);

        assert!(!named(&reading(&claude), "retired").enabled);
        assert!(!named(&reading(&codex), "old").enabled);
    }

    #[test]
    fn an_entry_that_could_not_be_read_is_named_and_never_quoted() {
        let (_directory, document) = written(CLAUDE_DOCUMENT, CLAUDE);

        let read = reading(&document);

        assert_eq!(read.errors, [format!("broken: {UNREADABLE}")]);
        assert!(!read.errors.iter().any(|error| error.contains('7')));
    }

    #[test]
    fn a_codex_document_reads_as_the_servers_its_tables_configure() {
        let (_directory, document) = written(CODEX_DOCUMENT, CODEX);

        let read = reading(&document);

        assert_eq!(
            read.servers
                .iter()
                .map(|server| server.name.as_str())
                .collect::<Vec<&str>>(),
            ["grafana", "linear", "old"]
        );
        assert!(read.errors.is_empty());
        let grafana = named(&read, "grafana");
        assert_eq!(grafana.transport, Transport::Stdio);
        assert_eq!(grafana.arguments, ["--transport", "stdio"]);
        assert_eq!(
            grafana.environment["GRAFANA_TOKEN"].expose(),
            "grafana-secret"
        );
        assert!(grafana.ignored.is_empty());
    }

    #[test]
    fn a_codex_key_zone_does_not_store_is_reported_as_not_imported() {
        let (_directory, document) = written(CODEX_DOCUMENT, CODEX);

        let linear = reading(&document);
        let linear = named(&linear, "linear");

        assert_eq!(linear.transport, Transport::Http);
        assert_eq!(linear.ignored, ["bearer_token_env_var"]);
        assert!(linear.headers.is_empty());
    }

    #[test]
    fn a_document_that_is_not_there_configures_nothing_and_fails_nothing() {
        let directory = TempDir::new().expect("a directory");

        let read = reading(&directory.path().join(CLAUDE_DOCUMENT));

        assert_eq!(read, ProviderServers::default());
    }

    #[test]
    fn a_document_in_the_wrong_format_is_reported_by_its_file_name() {
        let (_directory, document) = written(CLAUDE_DOCUMENT, "not json at all");

        let read = reading(&document);

        assert_eq!(
            read.errors,
            [format!("{CLAUDE_DOCUMENT}: the file is not valid JSON")]
        );
    }

    #[test]
    fn a_document_with_no_servers_of_its_own_configures_none() {
        let (_claude, claude) = written(CLAUDE_DOCUMENT, r#"{"numStartups": 3}"#);
        let (_codex, codex) = written(CODEX_DOCUMENT, "model = \"gpt-5\"\n");

        for document in [claude, codex] {
            assert_eq!(reading(&document), ProviderServers::default());
        }
    }

    #[test]
    fn the_first_document_to_configure_a_name_is_the_one_read() {
        let directory = TempDir::new().expect("a directory");
        let first = directory.path().join(CLAUDE_DOCUMENT);
        let second = directory.path().join(CLAUDE_SETTINGS);
        fs::write(
            &first,
            r#"{"mcpServers": {"notes": {"command": "first-server"}}}"#,
        )
        .expect("the first document");
        fs::write(
            &second,
            r#"{"mcpServers": {"notes": {"command": "second-server"}, "extra": {"command": "extra-server"}}}"#,
        )
        .expect("the second document");

        let read = read(&[first, second]);

        assert_eq!(
            named(&read, "notes").command.as_deref(),
            Some("first-server")
        );
        assert_eq!(read.servers.len(), 2);
    }

    #[test]
    fn each_agent_is_read_from_the_documents_it_keeps_in_a_home() {
        let home = Path::new("/state/organization");

        assert_eq!(
            in_directory(AgentKind::Claude, home),
            [home.join(CLAUDE_DOCUMENT), home.join(CLAUDE_SETTINGS)]
        );
        assert_eq!(
            in_directory(AgentKind::Codex, home),
            [home.join(CODEX_DOCUMENT)]
        );
    }

    #[test]
    fn an_entry_naming_nothing_to_reach_is_no_server() {
        let entry = serde_json::json!({"type": "http"});

        assert_eq!(server("nowhere", &entry), Err(NO_TARGET.to_string()));
    }

    #[test]
    fn an_entry_that_is_not_an_object_is_no_server() {
        let entry = serde_json::json!("notes-server");

        assert_eq!(server("notes", &entry), Err(NOT_AN_ENTRY.to_string()));
    }

    #[test]
    fn a_server_named_servers_is_read_as_the_server_it_is() {
        let entry = serde_json::json!({"command": "servers-server"});

        let server = server("servers", &entry).expect("a server");

        assert_eq!(server.command.as_deref(), Some("servers-server"));
    }
}
