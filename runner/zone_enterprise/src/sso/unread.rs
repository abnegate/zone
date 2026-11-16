use std::fmt;

use crate::sso::body::LIMIT;

#[derive(Debug)]
pub(crate) enum Unread {
    Transport(reqwest::Error),
    Oversized,
}

impl fmt::Display for Unread {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "{error}"),
            Self::Oversized => write!(formatter, "the response is larger than {LIMIT} bytes"),
        }
    }
}
