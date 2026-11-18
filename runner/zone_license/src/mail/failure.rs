use std::fmt;

use lettre::transport::smtp::Error;

pub const REJECTED: &str = "rejected";
pub const RESPONSE: &str = "response";
pub const CLIENT: &str = "client";
pub const TLS: &str = "tls";
pub const TIMEOUT: &str = "timeout";
pub const CONNECTION: &str = "connection";

/// What went wrong talking to the relay, without the relay's reply text,
/// which can quote the recipient's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Rejected,
    Response,
    Client,
    Tls,
    Timeout,
    Connection,
}

impl Failure {
    pub fn of(error: &Error) -> Self {
        if error.status().is_some() {
            Self::Rejected
        } else if error.is_response() {
            Self::Response
        } else if error.is_client() {
            Self::Client
        } else if error.is_tls() {
            Self::Tls
        } else if error.is_timeout() {
            Self::Timeout
        } else {
            Self::Connection
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rejected => REJECTED,
            Self::Response => RESPONSE,
            Self::Client => CLIENT,
            Self::Tls => TLS,
            Self::Timeout => TIMEOUT,
            Self::Connection => CONNECTION,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_log_contract() {
        assert_eq!(Failure::Rejected.to_string(), "rejected");
        assert_eq!(Failure::Response.to_string(), "response");
        assert_eq!(Failure::Client.to_string(), "client");
        assert_eq!(Failure::Tls.to_string(), "tls");
        assert_eq!(Failure::Timeout.to_string(), "timeout");
        assert_eq!(Failure::Connection.to_string(), "connection");
    }
}
