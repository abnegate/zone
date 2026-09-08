use serde::{Deserialize, Serialize};

/// The part a nominated file plays in the source it was found in.
///
/// A closed set: an unrecognised role rejects the whole marker rather than
/// widening what the model can name.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Configuration,
    Documentation,
    Entrypoint,
    Fixture,
    Implementation,
    Manifest,
    Migration,
    Schema,
    Test,
    Workflow,
}
