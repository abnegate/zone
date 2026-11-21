use serde::Deserialize;
use serde_json::Value;

use super::log::Log;

/// A page of LiteLLM's `/spend/logs/v2`. Its `total` stops counting, so only the number of rows
/// on a page says whether another follows.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Page {
    #[serde(default)]
    data: Vec<Value>,
}

impl Page {
    /// How many rows LiteLLM returned, readable or not.
    pub fn rows(&self) -> usize {
        self.data.len()
    }

    /// The rows, without any that can't be read: one malformed row never holds the rest back.
    pub fn logs(self) -> Vec<Log> {
        self.data
            .into_iter()
            .filter_map(|row| {
                serde_json::from_value(row)
                    .inspect_err(|error| tracing::warn!(%error, "Skipped a spend log row that could not be read"))
                    .ok()
            })
            .collect()
    }
}
