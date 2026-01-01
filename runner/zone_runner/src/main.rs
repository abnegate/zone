//! Zone Runner CLI
//!
//! A tool runner for executing commands in workspaces with streaming output.
//!
//! # Usage
//!
//! ```bash
//! # Daemon mode (for integration with Gleam backend)
//! zone-runner serve --stdio
//!
//! # One-shot mode (for standalone usage)
//! zone-runner exec --workspace /path/to/workspace --cmd echo -- hello world
//! ```

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use tracing_subscriber::{fmt, EnvFilter};

mod exec;
mod serve;

#[derive(Parser)]
#[command(name = "zone-runner")]
#[command(author, version, about = "Zone tool runner for command execution")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    /// Output logs as JSON
    #[arg(long, global = true)]
    log_json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run in daemon mode, communicating via stdio
    Serve {
        /// Use stdio for communication (required)
        #[arg(long, required = true)]
        stdio: bool,
    },

    /// Execute a single command (for standalone usage)
    Exec {
        /// Workspace directory path
        #[arg(long)]
        workspace: PathBuf,

        /// Command to execute
        #[arg(long)]
        cmd: String,

        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,

        /// Arguments to pass to the command
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // Set up tracing - log to stderr (stdout is for protocol)
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&cli.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if cli.log_json {
        fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .json()
            .init();
    } else {
        fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }

    match cli.command {
        Commands::Serve { stdio } => {
            if !stdio {
                eprintln!("Error: --stdio is required for serve mode");
                return ExitCode::FAILURE;
            }

            tracing::info!("Starting zone-runner in daemon mode");

            match serve::run_daemon().await {
                Ok(_) => {
                    tracing::info!("Daemon exited cleanly");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    tracing::error!("Daemon error: {}", e);
                    ExitCode::FAILURE
                }
            }
        }

        Commands::Exec {
            workspace,
            cmd,
            timeout,
            args,
        } => {
            tracing::info!(
                "Executing command: {} {:?} in {}",
                cmd,
                args,
                workspace.display()
            );

            match exec::run_once(workspace, cmd, args, timeout).await {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!("Execution error: {}", e);
                    ExitCode::FAILURE
                }
            }
        }
    }
}
