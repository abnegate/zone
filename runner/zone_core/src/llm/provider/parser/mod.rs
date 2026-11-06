//! Translating each agent's own stream format into [`super::event::AgentEvent`].

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

pub mod claude;
pub mod codex;

/// A field an agent may one day send in another shape, read as absent rather
/// than losing the whole line, and with it a refusal or news of a subagent,
/// to a parse error.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}
