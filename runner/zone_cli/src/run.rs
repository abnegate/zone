//! Agent run execution
//!
//! Handles running the AI agent locally.

use std::path::PathBuf;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use thiserror::Error;

use zone_core::{
    Agent, AgentCallback, AgentConfig, AgentError, AgentPhase, FileSessionStore, LlmClient,
    LlmConfig, Session, SessionStore, ToolContext, ToolResult,
};

use crate::auth::AuthManager;
use crate::config::{Config, ConfigError};

/// Run error
#[derive(Debug, Error)]
pub enum RunError {
    #[error("Not logged in. Run 'zone login <host>' first.")]
    NotLoggedIn,
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Agent error: {0}")]
    Agent(#[from] AgentError),
    #[error("Session error: {0}")]
    Session(#[from] zone_core::session::SessionError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ConfigError> for RunError {
    fn from(e: ConfigError) -> Self {
        RunError::Config(e.to_string())
    }
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

impl AgentCallback for CliCallback {
    fn on_phase_change(&self, phase: AgentPhase, message: Option<&str>) {
        let msg = match phase {
            AgentPhase::Thinking => "Thinking...".to_string(),
            AgentPhase::Acting => "Executing tools...".to_string(),
            AgentPhase::Observing => "Processing results...".to_string(),
            AgentPhase::Responding => "Generating response...".to_string(),
            AgentPhase::Complete => "Complete".to_string(),
            AgentPhase::Error => "Error".to_string(),
        };

        if let Some(detail) = message {
            self.spinner.set_message(format!("{} ({})", msg, detail));
        } else {
            self.spinner.set_message(msg);
        }
    }

    fn on_tool_call(&self, tool_name: &str, args: &str) {
        if self.verbose {
            self.spinner.suspend(|| {
                println!(
                    "{} {} {}",
                    style("→").cyan(),
                    style(tool_name).yellow().bold(),
                    style(truncate(args, 80)).dim()
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
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s
    }
}

/// Run the agent with a prompt
pub async fn run(prompt: &str, workspace: Option<&str>, verbose: bool) -> Result<(), RunError> {
    // Load config
    let config = Config::load().map_err(|e| RunError::Config(e.to_string()))?;

    // Check authentication
    let auth = AuthManager::new();
    if !auth.is_logged_in() {
        return Err(RunError::NotLoggedIn);
    }

    let metadata = auth.get_metadata().map_err(|_| RunError::NotLoggedIn)?;

    // Get LLM API key from server config (or use local LiteLLM)
    // For now, we'll assume a local setup
    let llm_config = LlmConfig {
        base_url: format!("{}/api/llm", metadata.host),
        api_key: auth
            .get_access_token()
            .await
            .map_err(|_| RunError::NotLoggedIn)?,
        default_model: config.model.clone(),
        temperature: 0.7,
        max_tokens: 4096,
    };

    // Set up working directory
    let cwd = if let Some(ws) = workspace {
        PathBuf::from(ws)
    } else {
        std::env::current_dir()?
    };

    // Create agent
    let llm = LlmClient::new(llm_config);
    let tools = zone_core::tools::with_defaults_and_mcp().await;
    let agent_config = AgentConfig {
        max_iterations: config.max_iterations as usize,
        ..Default::default()
    };
    let context = ToolContext {
        cwd: cwd.clone(),
        ..Default::default()
    };

    let agent = Agent::new(llm, tools, agent_config, context);

    // Create callback
    let callback = CliCallback::new(verbose);

    println!(
        "{} {}",
        style("Working in:").dim(),
        style(cwd.display()).cyan()
    );
    println!();

    // Run agent
    let state = agent.run(prompt, &callback).await?;

    // Save session
    let session_store = FileSessionStore::new(Config::ensure_sessions_dir()?);
    let title = create_session_title(prompt);
    let session = Session::new(state, title, Some(cwd.display().to_string()));
    session_store.save(&session).await?;

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
    let config = Config::load().map_err(|e| RunError::Config(e.to_string()))?;
    let session_store = FileSessionStore::new(Config::ensure_sessions_dir()?);

    // Load session
    let session = if last {
        session_store
            .most_recent()
            .await?
            .ok_or_else(|| RunError::Config("No sessions found".to_string()))?
    } else if let Some(id) = session_id {
        let uuid = uuid::Uuid::parse_str(id)
            .map_err(|_| RunError::Config(format!("Invalid session ID: {}", id)))?;
        session_store.load(uuid).await?
    } else {
        // Interactive selection
        let summaries = session_store.list().await?;
        if summaries.is_empty() {
            return Err(RunError::Config("No sessions found".to_string()));
        }

        // Display sessions
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

        // Use dialoguer for selection
        let selection: usize = dialoguer::Input::new()
            .with_prompt("Select session (1-10)")
            .validate_with(|input: &usize| {
                if *input >= 1 && *input <= summaries.len() {
                    Ok(())
                } else {
                    Err("Invalid selection")
                }
            })
            .interact()
            .map_err(|e| RunError::Config(e.to_string()))?;

        session_store.load(summaries[selection - 1].id).await?
    };

    println!(
        "{} {} {}",
        style("Resuming session:").dim(),
        style(session.id.to_string().split('-').next().unwrap()).yellow(),
        style(&session.title).white()
    );
    println!();

    // Set up agent (similar to run)
    let auth = AuthManager::new();
    if !auth.is_logged_in() {
        return Err(RunError::NotLoggedIn);
    }

    let metadata = auth.get_metadata().map_err(|_| RunError::NotLoggedIn)?;

    let llm_config = LlmConfig {
        base_url: format!("{}/api/llm", metadata.host),
        api_key: auth
            .get_access_token()
            .await
            .map_err(|_| RunError::NotLoggedIn)?,
        default_model: config.model.clone(),
        temperature: 0.7,
        max_tokens: 4096,
    };

    let cwd = session
        .project_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let llm = LlmClient::new(llm_config);
    let tools = zone_core::tools::with_defaults_and_mcp().await;
    let agent_config = AgentConfig {
        max_iterations: config.max_iterations as usize,
        ..Default::default()
    };
    let context = ToolContext {
        cwd: cwd.clone(),
        ..Default::default()
    };

    let agent = Agent::new(llm, tools, agent_config, context);

    // Get continuation prompt
    let prompt: String = dialoguer::Input::new()
        .with_prompt("Continue with")
        .interact_text()
        .map_err(|e| RunError::Config(e.to_string()))?;

    let callback = CliCallback::new(verbose);

    // Continue the session
    let mut session = session;
    agent
        .continue_run(&mut session.state, &prompt, &callback)
        .await?;

    // Update timestamp and save
    session.updated_at = chrono::Utc::now();
    session_store.save(&session).await?;

    Ok(())
}

/// List recent sessions
pub async fn list_sessions(limit: usize) -> Result<(), RunError> {
    let session_store = FileSessionStore::new(Config::ensure_sessions_dir()?);
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

        if let Some(dir) = &s.project_dir {
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
        let err = RunError::NotLoggedIn;
        assert!(err.to_string().contains("Not logged in"));

        let err = RunError::Config("test error".to_string());
        assert!(err.to_string().contains("Configuration error"));
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_run_error_from_config_error() {
        let config_err = ConfigError::NoHomeDir;
        let run_err: RunError = config_err.into();
        assert!(matches!(run_err, RunError::Config(_)));
    }
}
