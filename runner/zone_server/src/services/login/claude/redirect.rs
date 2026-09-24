//! Where claude.com sends the browser once someone approves a sign-in.

use super::{Flow, REDIRECT_URL};

/// The only host a loopback redirect names, as the claude CLI's own sign-in does.
pub const LOOPBACK_HOST: &str = "localhost";

pub const LOOPBACK_SCHEME: &str = "http";

/// The path of every loopback redirect, and the only one Zone's callback listener serves.
pub const CALLBACK_PATH: &str = "/callback";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redirect {
    /// claude.com's own page, which shows the code to paste.
    Paste,
    /// Zone's callback listener, at `http://localhost:<port>/callback`.
    Loopback(u16),
}

impl Redirect {
    /// The `redirect_uri` the authorize link carries, which the exchange has to send again.
    pub fn uri(self) -> String {
        match self {
            Self::Paste => REDIRECT_URL.to_string(),
            Self::Loopback(port) => {
                format!("{LOOPBACK_SCHEME}://{LOOPBACK_HOST}:{port}{CALLBACK_PATH}")
            }
        }
    }

    pub fn flow(self) -> Flow {
        match self {
            Self::Paste => Flow::Paste,
            Self::Loopback(_) => Flow::Loopback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loopback_redirect_is_the_one_the_claude_cli_sends_to_its_own_listener() {
        assert_eq!(
            Redirect::Loopback(54_545).uri(),
            "http://localhost:54545/callback"
        );
        assert_eq!(Redirect::Loopback(54_545).flow(), Flow::Loopback);
    }

    #[test]
    fn a_paste_redirect_is_claudes_own_page() {
        assert_eq!(Redirect::Paste.uri(), REDIRECT_URL);
        assert_eq!(Redirect::Paste.flow(), Flow::Paste);
    }
}
