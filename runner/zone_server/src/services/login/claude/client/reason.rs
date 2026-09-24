//! A refusal's `error`: an OAuth error code, or an API error with a message.

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Reason {
    Code(String),
    Detail { message: String },
}

impl Reason {
    pub(super) fn text(self) -> String {
        match self {
            Self::Code(code) => code,
            Self::Detail { message } => message,
        }
    }
}
