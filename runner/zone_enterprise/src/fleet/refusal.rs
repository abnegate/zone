use std::fmt;

use crate::fleet::code::{
    LABELS_MISSING, NO_RUNNER, NO_RUNNER_LEFT, RUNNER_CORDONED, RUNNER_OUTDATED, RUNNER_REVOKED,
    RUNNER_UNREGISTERED,
};
use crate::fleet::labels::Labels;
use crate::fleet::version::Version;

/// Why fleet policy refused work, or refused a change that would leave no runner to take it.
/// The runner refusals read as a chat sees them, since a refused task run is handed to another
/// runner or fails as [`Refusal::NoRunner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Unregistered,
    Cordoned,
    Revoked,
    Outdated {
        version: Version,
        minimum: Version,
    },
    LabelsMissing {
        missing: Labels,
    },
    NoRunner {
        minimum: Option<Version>,
        labels: Labels,
    },
    NoRunnerLeft {
        remaining: i64,
    },
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unregistered => RUNNER_UNREGISTERED,
            Self::Cordoned => RUNNER_CORDONED,
            Self::Revoked => RUNNER_REVOKED,
            Self::Outdated { .. } => RUNNER_OUTDATED,
            Self::LabelsMissing { .. } => LABELS_MISSING,
            Self::NoRunner { .. } => NO_RUNNER,
            Self::NoRunnerLeft { .. } => NO_RUNNER_LEFT,
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unregistered => formatter.write_str(
                "This Zone server isn't taking work as a runner in its fleet right now: it may be restarting, or another server may share its runner name. Reload the page to reach another server.",
            ),
            Self::Cordoned => formatter
                .write_str("This runner is cordoned, so it takes no new unattended work."),
            Self::Revoked => formatter.write_str(
                "This Zone server was revoked from its fleet, so it can't run chats. Reload the page to reach another server.",
            ),
            Self::Outdated { version, minimum } => write!(
                formatter,
                "This Zone server runs Zone {version}, below its fleet's minimum of {minimum}, so it can't run chats. Reload the page to reach another server."
            ),
            Self::LabelsMissing { missing } => write!(
                formatter,
                "This runner lacks labels this work requires: {missing}."
            ),
            Self::NoRunner { minimum, labels } => {
                formatter.write_str(
                    "No runner in this server's fleet can take this run. It needs an active runner",
                )?;
                if let Some(minimum) = minimum {
                    write!(formatter, " on Zone {minimum} or later")?;
                }
                if !labels.is_empty() {
                    write!(formatter, " labelled {labels}")?;
                }
                formatter.write_str(".")
            }
            Self::NoRunnerLeft { .. } => formatter.write_str(
                "This would leave no runner able to take work. Confirm to do it anyway.",
            ),
        }
    }
}

impl std::error::Error for Refusal {}
