use std::error::Error;
use std::io;
use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;
use zone_enterprise::Keyring;
use zone_license::{Cli, Command, keygen, serve, sign};

const DEFAULT_FILTER: &str = "info";

fn main() -> ExitCode {
    let command = Cli::parse().command;
    subscribe(&command);
    let outcome: Result<(), Box<dyn Error>> = match command {
        Command::Keygen(arguments) => keygen::run(
            &arguments,
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )
        .map_err(Into::into),
        Command::Sign(arguments) => {
            sign::run(arguments, &Keyring::production(), &mut io::stdout().lock())
                .map_err(Into::into)
        }
        Command::Registry(command) => command
            .run(&Keyring::production(), &mut io::stdout().lock())
            .map_err(Into::into),
        Command::Serve => serve::run(&Keyring::production()).map_err(Into::into),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zone-license: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `serve` logs JSON lines for a log collector; the one-shot commands log
/// plain text for a person at a terminal.
fn subscribe(command: &Command) {
    let builder = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER)),
        );
    match command {
        Command::Serve => builder.json().init(),
        Command::Keygen(_) | Command::Sign(_) | Command::Registry(_) => builder.init(),
    }
}
