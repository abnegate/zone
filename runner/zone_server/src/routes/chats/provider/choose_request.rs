use serde::Deserialize;

/// The provider, and optionally the model, one chat runs on. No provider
/// returns the chat to its workspace's.
#[derive(Debug, Deserialize)]
pub struct ChooseRequest {
    pub provider: Option<String>,
    pub model: Option<String>,
}
