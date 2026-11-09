//! Zone CLI - AI coding agent for your terminal
//!
//! This is the main entry point for the Zone CLI, which provides:
//! - Authentication via hosted manager
//! - Local agent execution
//! - Session management and resume
//! - Development environment setup

mod auth;
mod config;
mod explanation;
mod lighthouse;
mod run;
mod setup;

use clap::Parser;
use clap::Subcommand;
use console::style;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::auth::AuthManager;

const VERBOSE_FILTER: &str = "zone_cli=debug,zone_core=debug,abnegate_agent=debug";
const QUIET_FILTER: &str = "zone_cli=info";

#[derive(Parser)]
#[command(name = "zone")]
#[command(about = "Zone - AI coding agent for your terminal")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with a Zone server
    Login {
        /// The Zone server URL (e.g., https://zone.example.com)
        host: String,
    },

    /// Log out and clear credentials
    Logout,

    /// Run an agent task
    Run {
        /// The task prompt
        prompt: String,

        /// Working directory (default: current directory)
        #[arg(short, long)]
        workspace: Option<String>,
    },

    /// Resume a previous session
    Resume {
        /// Session ID to resume (omit for interactive selection)
        session_id: Option<String>,

        /// Resume the most recent session
        #[arg(long)]
        last: bool,
    },

    /// List recent sessions
    Sessions {
        /// Maximum number of sessions to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Edit configuration
    Config,

    /// Setup development environment
    Setup {
        /// Project root directory (default: current directory)
        #[arg(short, long)]
        path: Option<String>,

        /// Run full setup non-interactively
        #[arg(long)]
        full: bool,

        /// Only check prerequisites
        #[arg(long)]
        check: bool,

        /// Only generate secrets and create .env file
        #[arg(long)]
        secrets: bool,

        /// Only validate configuration
        #[arg(long)]
        validate: bool,
    },

    /// Run Lighthouse performance audits on frontends
    Lighthouse {
        /// Target frontend: manager or all (default: all)
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Project root directory (default: current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_filter(cli.verbose).into()),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    match cli.command {
        Commands::Login { host } => {
            let auth = AuthManager::new()?;

            // Prompt for credentials
            let email: String = dialoguer::Input::new()
                .with_prompt("Email")
                .interact_text()?;

            let password = dialoguer::Password::new()
                .with_prompt("Password")
                .interact()?;

            println!("{}", style("Logging in...").dim());

            match auth.login(&host, &email, &password).await {
                Ok(metadata) => {
                    println!(
                        "{} Logged in as {} to {}",
                        style("✓").green(),
                        style(metadata.email.as_deref().unwrap_or_default()).cyan(),
                        style(&metadata.host).cyan()
                    );
                }
                Err(e) => {
                    eprintln!("{} {}", style("✗").red(), e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Logout => {
            let auth = AuthManager::new()?;
            auth.logout()?;
            println!("{} Logged out", style("✓").green());
        }
        Commands::Run { prompt, workspace } => {
            if let Err(e) = run::run(&prompt, workspace.as_deref(), cli.verbose).await {
                eprintln!("{} {}", style("Error:").red(), e);
                std::process::exit(1);
            }
        }
        Commands::Resume { session_id, last } => {
            if let Err(e) = run::resume(session_id.as_deref(), last, cli.verbose).await {
                eprintln!("{} {}", style("Error:").red(), e);
                std::process::exit(1);
            }
        }
        Commands::Sessions { limit } => {
            if let Err(e) = run::list_sessions(limit).await {
                eprintln!("{} {}", style("Error:").red(), e);
                std::process::exit(1);
            }
        }
        Commands::Config => {
            let config = config::Config::load()?;
            let path = config::Config::path()?;

            // Open in editor
            let editor = &config.editor;
            let status = std::process::Command::new(editor).arg(&path).status()?;

            if !status.success() {
                eprintln!("{} Editor exited with error", style("✗").red());
                std::process::exit(1);
            }
        }
        Commands::Setup {
            path,
            full,
            check,
            secrets,
            validate,
        } => {
            let project_root = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap());

            let setup = setup::Setup::new(project_root);

            // Handle specific flags
            if check {
                setup.check_prerequisites()?;
            } else if secrets {
                setup.setup_env_file()?;
            } else if validate {
                setup.validate_config()?;
            } else if full {
                setup.run_full()?;
            } else {
                // Interactive mode
                setup.run_interactive().await?;
            }
        }
        Commands::Lighthouse { target, path } => {
            let project_root = path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap());

            let result = match target.to_lowercase().as_str() {
                "manager" => lighthouse::run_lighthouse(
                    &project_root,
                    lighthouse::Frontend::Manager,
                    cli.verbose,
                ),
                "all" => lighthouse::run_all(&project_root, cli.verbose),
                _ => {
                    eprintln!(
                        "{} Invalid target: {}. Use 'manager' or 'all'",
                        style("✗").red(),
                        target
                    );
                    std::process::exit(1);
                }
            };

            if let Err(e) = result {
                eprintln!("{} {}", style("Error:").red(), e);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// The log directives used when `RUST_LOG` is unset.
fn log_filter(verbose: bool) -> &'static str {
    if verbose {
        VERBOSE_FILTER
    } else {
        QUIET_FILTER
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;

    use super::log_filter;

    #[test]
    fn verbose_logging_keeps_the_agent_crates_debug_logs() {
        let subscriber = tracing_subscriber::registry().with(EnvFilter::new(log_filter(true)));

        tracing::subscriber::with_default(subscriber, || {
            assert!(tracing::enabled!(target: "abnegate_agent::mcp::hub", Level::DEBUG));
            assert!(tracing::enabled!(target: "zone_core::servers", Level::DEBUG));
            assert!(tracing::enabled!(target: "zone_cli::run", Level::DEBUG));
        });
    }
}
