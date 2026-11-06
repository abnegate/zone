/// A provider key column in the AI settings tables.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AiKey {
    LitellmKey,
    OpenaiApiKey,
    AnthropicApiKey,
    RunpodApiKey,
    BedrockAccessKey,
    BedrockSecretKey,
}

impl AiKey {
    /// The column the key is stored in.
    pub fn column(self) -> &'static str {
        match self {
            Self::LitellmKey => "litellm_key",
            Self::OpenaiApiKey => "openai_api_key",
            Self::AnthropicApiKey => "anthropic_api_key",
            Self::RunpodApiKey => "runpod_api_key",
            Self::BedrockAccessKey => "bedrock_access_key",
            Self::BedrockSecretKey => "bedrock_secret_key",
        }
    }
}
