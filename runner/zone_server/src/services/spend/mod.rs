//! Spend through the instance's LiteLLM, attributed to the workspace and member it was spent for
//! by a virtual key Zone derives for each, and ingested from LiteLLM's spend logs.

pub mod attribution;
pub mod client;
pub mod coverage;
pub mod error;
pub mod ingest;
pub mod key;
pub mod log;
pub mod notice;
pub mod page;
pub mod registry;
pub mod standing;
pub mod subject;
pub mod sweep;
pub mod unlicensed;
pub mod upkeep;

#[cfg(test)]
mod tests;

use abnegate_secret::SecretValue;

pub use client::Client;
pub use error::Error;
pub use key::Key;
pub use standing::Standing;
pub use subject::Subject;
pub use unlicensed::Unlicensed;

use crate::state::AppState;

/// The instance's LiteLLM under its master key, none when the instance runs without one.
pub fn client(state: &AppState) -> Option<Client> {
    let config = state.config();
    if config.litellm_host.is_empty() || config.litellm_key.is_empty() {
        return None;
    }
    Some(Client::new(
        config.litellm_host.clone(),
        SecretValue::new(config.litellm_key.clone()),
    ))
}
