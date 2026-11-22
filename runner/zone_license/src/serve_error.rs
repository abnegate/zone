use std::io;
use std::net::SocketAddr;

use crate::config_error::ConfigError;
use crate::mail::MailError;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("the mail relay could not be set up: {0}")]
    Mail(#[from] MailError),
    #[error("the Polar API client could not be set up: {0}")]
    Polar(#[from] reqwest::Error),
    #[error("the bundle storage client could not be set up: {0}")]
    Storage(reqwest::Error),
    #[error("could not start the async runtime: {0}")]
    Runtime(io::Error),
    #[error("could not listen for shutdown signals: {0}")]
    Signal(io::Error),
    #[error("could not listen on {address}: {source}")]
    Bind {
        address: SocketAddr,
        source: io::Error,
    },
    #[error("the server stopped: {0}")]
    Serve(io::Error),
}
