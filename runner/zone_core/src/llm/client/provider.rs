//! Zone's client as a [`CompletionProvider`], for code written against any
//! provider, such as the crate's context compaction.

use abnegate_llm::Completion;
use abnegate_llm::CompletionProvider;
use abnegate_llm::CompletionRequest;
use abnegate_llm::ProviderError;
use abnegate_llm::ProviderKind;
use async_trait::async_trait;

use super::LlmBackend;
use super::LlmClient;

#[cfg(test)]
mod tests;

#[async_trait]
impl CompletionProvider for LlmClient {
    fn name(&self) -> &str {
        match self.backend() {
            LlmBackend::Http => ProviderKind::Http.as_str(),
            LlmBackend::Cli { agent, .. } => agent.as_str(),
        }
    }

    fn kind(&self) -> ProviderKind {
        match self.backend() {
            LlmBackend::Http => ProviderKind::Http,
            LlmBackend::Cli { .. } => ProviderKind::Cli,
        }
    }

    async fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, ProviderError> {
        let tuned;
        let client = match request.temperature {
            Some(temperature) => {
                tuned = self.clone().with_temperature(temperature);
                &tuned
            }
            None => self,
        };
        let response = client
            .chat_with_options(
                request.model,
                request.messages,
                request.tools,
                request.options,
            )
            .await
            .map_err(|source| ProviderError::http(self.name(), source))?;

        let usage = response.usage;
        let choice =
            response.choices.into_iter().next().ok_or_else(|| {
                ProviderError::agent(self.name(), "the provider returned no choices")
            })?;

        Ok(Completion::new(self.name(), choice.message)
            .with_usage(usage)
            .with_finish_reason(choice.finish_reason))
    }
}
