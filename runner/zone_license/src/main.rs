use std::error::Error;
use std::io;
use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::EnvFilter;
use zone_enterprise::Keyring;
use zone_license::{Cli, Command, keygen, sign};

const DEFAULT_FILTER: &str = "info";

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER)),
        )
        .init();
    let outcome: Result<(), Box<dyn Error>> = match Cli::parse().command {
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
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zone-license: {error}");
            ExitCode::FAILURE
        }
    }
}
