//! Agent run execution
//!
//! Handles running the AI agent locally.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use abnegate_agent::Agent;
use abnegate_agent::AgentCallback;
use abnegate_agent::AgentConfig;
use abnegate_agent::AgentPhase;
use abnegate_agent::AgentState;
use abnegate_agent::FileSessionStore;
use abnegate_agent::Session;
use abnegate_agent::SessionStore;
use abnegate_agent::ToolRegistry;
use abnegate_agent::session::SessionError;
use abnegate_agent::tool::Preview;
use abnegate_agent::tool::Tier;
use abnegate_agent::tool::ToolResult;
use abnegate_llm::LlmConfig;
use abnegate_llm::ToolCall;
use async_trait::async_trait;
use console::style;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use thiserror::Error;
use zone_core::commands;
use zone_core::commands::Shell;
use zone_core::llm;
use zone_core::llm::LlmClient;
use zone_core::local;
use zone_core::servers;

use crate::auth::AuthManager;
use crate::config::Config;
use crate::explanation::Explanation;

#[cfg(test)]
mod turns;

const MAXIMUM_TOKENS: u32 = 4096;

const TEMPERATURE: f32 = 0.7;

/// Run error
#[derive(Debug, Error)]
pub enum RunError {
    #[error("Not logged in. Run 'zone login <host>' first.")]
    NotLoggedIn,
    #[error("Configuration error: {}", Explanation(.0))]
    Config(#[from] abnegate_config::Error),
    #[error("No sessions found")]
    NoSessions,
    #[error("Invalid session ID: {0}")]
    InvalidSession(String),
    #[error("Prompt error: {0}")]
    Prompt(#[from] dialoguer::Error),
    #[error(
        "No LLM endpoint is configured. Set llm_base_url in {path} to an OpenAI-compatible base \
         URL (LiteLLM: http://localhost:4000, Ollama: http://127.0.0.1:11434/v1) and llm_api_key \
         if it needs one."
    )]
    LlmNotConfigured { path: String },
    #[error("Agent error: {0}")]
    Agent(#[from] abnegate_agent::RunError),
    #[error("Session error: {0}")]
    Session(#[from] SessionError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// CLI callback for agent progress
struct CliCallback {
    spinner: ProgressBar,
    verbose: bool,
}

impl CliCallback {
    fn new(verbose: bool) -> Self {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));

        Self { spinner, verbose }
    }

    fn finish(&self) {
        self.spinner.finish_and_clear();
    }
}

#[async_trait]
impl AgentCallback for CliCallback {
    fn on_phase_change(&self, phase: AgentPhase, message: Option<&str>) {
        let label = match phase {
            AgentPhase::Thinking => "Thinking...".to_string(),
            AgentPhase::Acting => "Executing tools...".to_string(),
            AgentPhase::Observing => "Processing results...".to_string(),
            AgentPhase::Responding => "Generating response...".to_string(),
            AgentPhase::Complete => "Complete".to_string(),
            AgentPhase::Error => "Error".to_string(),
            _ => phase.to_string(),
        };

        if let Some(detail) = message {
            self.spinner.set_message(format!("{} ({})", label, detail));
        } else {
            self.spinner.set_message(label);
        }
    }

    fn on_tool_call(&self, tool_name: &str, arguments: &str) {
        if self.verbose {
            self.spinner.suspend(|| {
                println!(
                    "{} {} {}",
                    style("→").cyan(),
                    style(tool_name).yellow().bold(),
                    style(truncate(arguments, 80)).dim()
                );
            });
        } else {
            self.spinner
                .set_message(format!("Running {}...", tool_name));
        }
    }

    fn on_tool_result(&self, tool_name: &str, result: &ToolResult) {
        if self.verbose {
            self.spinner.suspend(|| {
                let status = if result.success {
                    style("✓").green()
                } else {
                    style("✗").red()
                };
                let output = result
                    .output
                    .as_deref()
                    .or(result.error.as_deref())
                    .unwrap_or("");
                println!(
                    "  {} {} {}",
                    status,
                    style(tool_name).dim(),
                    style(truncate(output, 60)).dim()
                );
            });
        }
    }

    fn on_response(&self, response: &str) {
        self.finish();
        println!("\n{}\n", response);
    }

    async fn approve(&self, _call: &ToolCall, _tier: Tier, _preview: &Preview) -> bool {
        true
    }
}

/// Truncate a string to a maximum length
fn truncate(text: &str, maximum_length: usize) -> String {
    let text = text.replace('\n', " ");
    if text.len() > maximum_length {
        format!("{}...", &text[..maximum_length])
    } else {
        text
    }
}

/// The completions endpoint a local run talks to. The Zone server has no
/// completions route of its own, so this comes from the CLI's config rather
/// than from the host the CLI is logged in to.
fn llm_config(config: &Config) -> Result<LlmConfig, RunError> {
    let base_url = config
        .llm_base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| RunError::LlmNotConfigured {
            path: Config::path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "~/.zone/config.toml".to_string()),
        })?;
    Ok(LlmConfig::new(
        base_url.trim_end_matches('/'),
        config.model.clone(),
        config.llm_api_key.clone().unwrap_or_default(),
    )
    .with_temperature(TEMPERATURE)
    .with_maximum_tokens(MAXIMUM_TOKENS)
    .with_timeout(llm::TIMEOUT)
    .with_read_timeout(llm::READ_TIMEOUT))
}

/// The tools a local run offers before any MCP server's are added
fn registry() -> ToolRegistry {
    commands::registry(Shell::Excluded)
}

/// The tools a local run offers, and how many MCP servers contributed to them
async fn tools() -> (ToolRegistry, usize) {
    let mut tools = registry();
    let connected = servers::attach(&mut tools).await;
    (tools, connected)
}

/// The agent a local run drives in `working_directory`, given `tools` from
/// `connected` MCP servers
fn agent(
    llm: LlmClient,
    tools: ToolRegistry,
    connected: usize,
    maximum_iterations: usize,
    working_directory: &Path,
) -> Agent {
    let model = llm.config().default_model.clone();
    let guidance = servers::prompt(connected, &tools.names());
    let summarizer = Arc::new(llm.summarizer());
    let config = AgentConfig::default()
        .with_maximum_iterations(maximum_iterations)
        .with_maximum_tokens(MAXIMUM_TOKENS);
    Agent::new(
        Arc::new(llm),
        model,
        tools,
        config,
        local::context(working_directory),
    )
    .with_summarizer(summarizer)
    .with_guidance(guidance)
}

/// Save a finished first turn on `prompt`, run in `working_directory`
async fn save(
    store: &dyn SessionStore,
    state: AgentState,
    prompt: &str,
    working_directory: &Path,
) -> Result<Session, SessionError> {
    let session = Session::new(
        state,
        create_session_title(prompt),
        Some(working_directory.display().to_string()),
    );
    store.save(&session).await?;
    Ok(session)
}

/// The directory a resumed session works in: its own, or the current one
fn working_directory(session: &Session) -> PathBuf {
    session
        .project_directory
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap())
}

/// Continue `session` with `prompt`, and save it
async fn proceed(
    agent: &Agent,
    store: &dyn SessionStore,
    session: &mut Session,
    prompt: &str,
    callback: &dyn AgentCallback,
) -> Result<(), RunError> {
    agent
        .continue_run(&mut session.state, prompt, callback)
        .await?;
    session.updated_at = chrono::Utc::now();
    store.save(session).await?;
    Ok(())
}

/// Run the agent with a prompt
pub async fn run(prompt: &str, workspace: Option<&str>, verbose: bool) -> Result<(), RunError> {
    let config = Config::load()?;

    let auth = AuthManager::new().map_err(|_| RunError::NotLoggedIn)?;
    if !auth.is_logged_in() {
        return Err(RunError::NotLoggedIn);
    }

    let llm_config = llm_config(&config)?;
    let metadata = auth.get_metadata().map_err(|_| RunError::NotLoggedIn)?;
    auth.get_access_token()
        .await
        .map_err(|_| RunError::NotLoggedIn)?;
    println!(
        "{} {} {}",
        style("Signed in as").dim(),
        style(metadata.email.as_deref().unwrap_or_default()).cyan(),
        style(format!("({})", metadata.host)).dim()
    );

    let cwd = if let Some(workspace) = workspace {
        PathBuf::from(workspace)
    } else {
        std::env::current_dir()?
    };

    let (tools, connected) = tools().await;
    let agent = agent(
        LlmClient::new(llm_config),
        tools,
        connected,
        config.max_iterations as usize,
        &cwd,
    );

    let callback = CliCallback::new(verbose);

    println!(
        "{} {}",
        style("Working in:").dim(),
        style(cwd.display()).cyan()
    );
    println!();

    let state = agent.run(prompt, &callback).await?;

    let session_store = FileSessionStore::new(Config::ensure_sessions_directory()?);
    let session = save(&session_store, state, prompt, &cwd).await?;

    println!(
        "{} {} {}",
        style("Session saved:").dim(),
        style(session.id.to_string().split('-').next().unwrap()).yellow(),
        style("(use 'zone resume' to continue)").dim()
    );

    Ok(())
}

/// Resume a session
pub async fn resume(session_id: Option<&str>, last: bool, verbose: bool) -> Result<(), RunError> {
    let config = Config::load()?;
    let session_store = FileSessionStore::new(Config::ensure_sessions_directory()?);

    let mut session = if last {
        session_store
            .most_recent()
            .await?
            .ok_or(RunError::NoSessions)?
    } else if let Some(id) = session_id {
        let uuid =
            uuid::Uuid::parse_str(id).map_err(|_| RunError::InvalidSession(id.to_string()))?;
        session_store.load(uuid).await?
    } else {
        let summaries = session_store.list().await?;
        if summaries.is_empty() {
            return Err(RunError::NoSessions);
        }

        println!("{}", style("Recent sessions:").bold());
        for (i, s) in summaries.iter().take(10).enumerate() {
            let status = if s.finished {
                style("✓").green()
            } else {
                style("→").yellow()
            };
            let short_id = s.id.to_string().split('-').next().unwrap().to_string();
            let age = chrono::Utc::now().signed_duration_since(s.updated_at);
            let age_str = format_duration(age);

            println!(
                "  {} {} {} {} {}",
                style(format!("[{}]", i + 1)).dim(),
                status,
                style(&short_id).cyan(),
                style(&s.title).white(),
                style(age_str).dim()
            );
        }

        let selection: usize = dialoguer::Input::new()
            .with_prompt("Select session (1-10)")
            .validate_with(|input: &usize| {
                if *input >= 1 && *input <= summaries.len() {
                    Ok(())
                } else {
                    Err("Invalid selection")
                }
            })
            .interact()?;

        session_store.load(summaries[selection - 1].id).await?
    };

    println!(
        "{} {} {}",
        style("Resuming session:").dim(),
        style(session.id.to_string().split('-').next().unwrap()).yellow(),
        style(&session.title).white()
    );
    println!();

    let auth = AuthManager::new().map_err(|_| RunError::NotLoggedIn)?;
    if !auth.is_logged_in() {
        return Err(RunError::NotLoggedIn);
    }

    let llm_config = llm_config(&config)?;
    let metadata = auth.get_metadata().map_err(|_| RunError::NotLoggedIn)?;
    auth.get_access_token()
        .await
        .map_err(|_| RunError::NotLoggedIn)?;
    println!(
        "{} {} {}",
        style("Signed in as").dim(),
        style(metadata.email.as_deref().unwrap_or_default()).cyan(),
        style(format!("({})", metadata.host)).dim()
    );

    let cwd = working_directory(&session);
    let (tools, connected) = tools().await;
    let agent = agent(
        LlmClient::new(llm_config),
        tools,
        connected,
        config.max_iterations as usize,
        &cwd,
    );

    let prompt: String = dialoguer::Input::new()
        .with_prompt("Continue with")
        .interact_text()?;

    let callback = CliCallback::new(verbose);

    proceed(&agent, &session_store, &mut session, &prompt, &callback).await
}

/// List recent sessions
pub async fn list_sessions(limit: usize) -> Result<(), RunError> {
    let session_store = FileSessionStore::new(Config::ensure_sessions_directory()?);
    let summaries = session_store.list().await?;

    if summaries.is_empty() {
        println!("{}", style("No sessions found").dim());
        return Ok(());
    }

    println!("{}", style("Recent sessions:").bold());
    println!();

    for s in summaries.iter().take(limit) {
        let status = if s.finished {
            style("✓").green()
        } else {
            style("→").yellow()
        };
        let short_id = s.id.to_string().split('-').next().unwrap().to_string();
        let age = chrono::Utc::now().signed_duration_since(s.updated_at);
        let age_str = format_duration(age);

        println!(
            "  {} {} {} {}",
            status,
            style(&short_id).cyan(),
            style(&s.title).white(),
            style(format!("({})", age_str)).dim()
        );

        if let Some(dir) = &s.project_directory {
            println!("    {}", style(dir).dim());
        }
    }

    Ok(())
}

/// Format a duration as a human-readable string
fn format_duration(duration: chrono::Duration) -> String {
    let seconds = duration.num_seconds().abs();
    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

/// Create a session title from the prompt
fn create_session_title(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().take(6).collect();
    if words.len() == 6 {
        format!("{}...", words.join(" "))
    } else {
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        assert_eq!(truncate("hello world this is a test", 10), "hello worl...");
    }

    #[test]
    fn test_truncate_with_newlines() {
        assert_eq!(truncate("hello\nworld", 20), "hello world");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn test_format_duration_just_now() {
        let duration = chrono::Duration::seconds(30);
        assert_eq!(format_duration(duration), "just now");
    }

    #[test]
    fn test_format_duration_minutes() {
        let duration = chrono::Duration::minutes(5);
        assert_eq!(format_duration(duration), "5m ago");
    }

    #[test]
    fn test_format_duration_hours() {
        let duration = chrono::Duration::hours(2);
        assert_eq!(format_duration(duration), "2h ago");
    }

    #[test]
    fn test_format_duration_days() {
        let duration = chrono::Duration::days(3);
        assert_eq!(format_duration(duration), "3d ago");
    }

    #[test]
    fn test_format_duration_negative() {
        // Should use absolute value
        let duration = chrono::Duration::hours(-2);
        assert_eq!(format_duration(duration), "2h ago");
    }

    #[test]
    fn test_create_session_title_short() {
        assert_eq!(create_session_title("Fix the bug"), "Fix the bug");
    }

    #[test]
    fn test_create_session_title_long() {
        let prompt = "Add new feature to handle user authentication and authorization";
        let title = create_session_title(prompt);
        assert_eq!(title, "Add new feature to handle user...");
    }

    #[test]
    fn test_create_session_title_empty() {
        assert_eq!(create_session_title(""), "");
    }

    #[test]
    fn test_create_session_title_exactly_six_words() {
        assert_eq!(
            create_session_title("one two three four five six"),
            "one two three four five six..."
        );
    }

    #[test]
    fn test_create_session_title_five_words() {
        assert_eq!(
            create_session_title("one two three four five"),
            "one two three four five"
        );
    }

    #[test]
    fn test_run_error_display() {
        let error = RunError::NotLoggedIn;
        assert!(error.to_string().contains("Not logged in"));

        let error = RunError::InvalidSession("not-a-uuid".to_string());
        assert_eq!(error.to_string(), "Invalid session ID: not-a-uuid");

        assert_eq!(RunError::NoSessions.to_string(), "No sessions found");
    }

    #[test]
    fn test_run_error_from_config_error() {
        let error = RunError::from(abnegate_config::Error::NoHomeDirectory);
        assert!(matches!(error, RunError::Config(_)));
        assert_eq!(
            error.to_string(),
            "Configuration error: Home directory not found"
        );
    }

    #[test]
    fn a_configuration_error_never_quotes_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "llm_api_key = \"sk-secret\n").unwrap();

        let error = RunError::from(
            abnegate_config::Loader::at(&path)
                .load::<Config>()
                .unwrap_err(),
        );

        let message = error.to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(!message.contains("sk-secret"), "{message}");
    }

    #[test]
    fn a_configuration_error_names_its_cause() {
        let error = RunError::from(abnegate_config::Error::write(
            "/zone/sessions",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        ));

        assert_eq!(
            error.to_string(),
            "Configuration error: Failed to write '/zone/sessions': denied"
        );
    }

    /// A run without an endpoint says which key to set and where, rather than
    /// pointing at a route the server does not have or blaming the login.
    #[test]
    fn a_run_names_the_missing_llm_endpoint() {
        let config = Config {
            llm_base_url: None,
            ..Config::default()
        };
        let error = llm_config(&config).unwrap_err();
        assert!(matches!(error, RunError::LlmNotConfigured { .. }));
        let message = error.to_string();
        assert!(message.contains("llm_base_url"), "{message}");
        assert!(message.contains("config.toml"), "{message}");
        assert!(!message.contains("Not logged in"), "{message}");

        let blank = Config {
            llm_base_url: Some("   ".to_string()),
            ..Config::default()
        };
        assert!(matches!(
            llm_config(&blank),
            Err(RunError::LlmNotConfigured { .. })
        ));
    }

    #[test]
    fn a_configured_endpoint_is_used_as_given() {
        let config = Config {
            llm_base_url: Some("http://127.0.0.1:11434/v1/".to_string()),
            llm_api_key: Some("ollama".to_string()),
            model: "qwen3:8b".to_string(),
            ..Config::default()
        };
        let llm = llm_config(&config).unwrap();
        assert_eq!(llm.base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(llm.api_key.expose(), "ollama");
        assert_eq!(llm.default_model, "qwen3:8b");
        assert_eq!(llm.timeout, zone_core::llm::TIMEOUT);
        assert_eq!(llm.read_timeout, zone_core::llm::READ_TIMEOUT);
    }
}
