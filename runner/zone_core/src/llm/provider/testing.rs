//! A provider whose answers the tests decide.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use super::completion::{Completion, CompletionProvider, CompletionRequest, ProviderKind};
use super::error::ProviderError;
use crate::llm::Message;

/// What a [`StubProvider`] does when asked.
#[derive(Debug, Clone)]
pub enum Behaviour {
    Answer(String),
    /// Fails in a way a chain is expected to move past.
    Fail(String),
    /// Fails in a way a chain must not move past.
    Reject(String),
}

#[derive(Debug)]
pub struct StubProvider {
    name: String,
    behaviour: Behaviour,
    calls: AtomicUsize,
}

impl StubProvider {
    pub fn new(name: impl Into<String>, behaviour: Behaviour) -> Self {
        Self {
            name: name.into(),
            behaviour,
            calls: AtomicUsize::new(0),
        }
    }

    pub fn answering(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(name, Behaviour::Answer(text.into()))
    }

    pub fn failing(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(name, Behaviour::Fail(message.into()))
    }

    pub fn rejecting(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(name, Behaviour::Reject(message.into()))
    }

    pub fn shared(self) -> Arc<dyn CompletionProvider> {
        Arc::new(self)
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl CompletionProvider for StubProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Http
    }

    async fn complete(&self, _request: CompletionRequest<'_>) -> Result<Completion, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        match &self.behaviour {
            Behaviour::Answer(text) => Ok(Completion {
                provider: self.name.clone(),
                message: Message::assistant(text.clone()),
                usage: None,
                finish_reason: Some("stop".to_string()),
            }),
            Behaviour::Fail(message) => Err(ProviderError::Agent {
                provider: self.name.clone(),
                message: message.clone(),
            }),
            Behaviour::Reject(message) => Err(ProviderError::Http {
                provider: self.name.clone(),
                source: crate::llm::LlmError::Api {
                    status: 400,
                    message: message.clone(),
                },
            }),
        }
    }
}
