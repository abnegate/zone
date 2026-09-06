use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::llm::{LlmError, Message};

/// An immutable replay message plus request-local eligibility flags.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub message: Message,
    pub preserve: bool,
    pub consumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub entries: Vec<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub content: String,
    pub coverage: Coverage,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStatus {
    Ready,
    Compacting,
    Compacted,
    Unavailable,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    Runtime,
    Configured,
    Provider,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBreakdown {
    pub instructions: u64,
    pub conversation: u64,
    pub tools: u64,
    pub results: u64,
    pub summary: u64,
    pub attachments: Option<u64>,
    pub overhead: u64,
}

impl ContextBreakdown {
    pub fn total(&self) -> u64 {
        [
            self.instructions,
            self.conversation,
            self.tools,
            self.results,
            self.summary,
            self.attachments.unwrap_or_default(),
            self.overhead,
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsage {
    pub model: String,
    pub used: u64,
    pub limit: Option<u64>,
    pub reserved: u32,
    pub threshold: Option<u64>,
    pub remaining: Option<u64>,
    pub estimated: bool,
    pub incomplete: bool,
    pub source: ContextSource,
    pub status: ContextStatus,
    pub breakdown: ContextBreakdown,
    pub revision: u64,
    pub compacted_messages: usize,
    pub updated_at: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub limit: Option<u64>,
    pub reserved: u32,
    pub source: ContextSource,
}

impl Policy {
    /// Preserve 20% of available input capacity for estimation uncertainty.
    pub fn threshold(&self) -> Option<u64> {
        self.input_limit()
            .map(|limit| limit.saturating_sub(limit / 5))
    }

    pub fn input_limit(&self) -> Option<u64> {
        self.limit
            .map(|limit| limit.saturating_sub(u64::from(self.reserved)))
    }
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub messages: Vec<Message>,
    pub usage: ContextUsage,
    pub summary: Option<Summary>,
}

#[derive(Debug, Error)]
pub enum ContextError {
    #[error("Context capacity exceeded: {used} estimated input tokens; budget {budget}. {reason}")]
    Capacity {
        used: u64,
        budget: u64,
        reason: String,
    },
    #[error("Conversation checkpoint integrity error: {0}")]
    Integrity(String),
    #[error("Conversation summary failed: {0}")]
    Summary(String),
    #[error("Conversation summary transport failed: {0}")]
    Transport(#[from] LlmError),
}
