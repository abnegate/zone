//! One unit of work, described in terms both consumers can supply.
//!
//! A queued task and a pull request are the same shape to this module: an
//! identity, some prose, the paths and symbols it reaches, and the signals
//! measured about it.

use serde::Serialize;

use super::signals::Signals;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Task,
    PullRequest,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::PullRequest => "pull_request",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    pub identifier: String,
    pub origin: Origin,
    pub title: String,
    pub description: String,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
    pub labels: Vec<String>,
    pub signals: Signals,
}

impl Change {
    pub fn new(identifier: impl Into<String>, origin: Origin) -> Self {
        Self {
            identifier: identifier.into(),
            origin,
            ..Self::default()
        }
    }

    /// Everything a path pattern is matched against: the paths the change
    /// touches, plus the symbols it names when no path is known.
    pub fn surface(&self) -> impl Iterator<Item = &str> {
        self.paths
            .iter()
            .chain(self.symbols.iter())
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Origin};

    #[test]
    fn origin_renders_a_stable_identifier() {
        assert_eq!(Origin::Task.as_str(), "task");
        assert_eq!(Origin::PullRequest.as_str(), "pull_request");
    }

    #[test]
    fn surface_joins_paths_and_symbols() {
        let change = Change {
            paths: vec!["src/api/routes.rs".into()],
            symbols: vec!["charge_card".into()],
            ..Change::new("task-1", Origin::Task)
        };
        let surface: Vec<&str> = change.surface().collect();
        assert_eq!(surface, vec!["src/api/routes.rs", "charge_card"]);
    }

    #[test]
    fn surface_drops_blank_entries() {
        let change = Change {
            paths: vec!["".into(), "   ".into(), "src/lib.rs".into()],
            ..Change::new("task-1", Origin::Task)
        };
        let surface: Vec<&str> = change.surface().collect();
        assert_eq!(surface, vec!["src/lib.rs"]);
    }
}
