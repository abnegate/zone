//! Whose sign-in an organization's coding agent runs under.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// One an admin signed the organization in to from Zone.
    Zone,
    /// The server user's own sign-in on the host, which an organization without one falls back
    /// to when host login is on.
    Host,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_source_travels_in_lowercase() {
        for (source, spelled) in [(Source::Zone, "zone"), (Source::Host, "host")] {
            assert_eq!(
                serde_json::to_value(source).expect("serialise"),
                json!(spelled)
            );
            assert_eq!(
                serde_json::from_value::<Source>(json!(spelled)).expect("deserialise"),
                source
            );
        }
    }
}
