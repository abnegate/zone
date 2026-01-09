//! Context assembly and prompt injection
//!
//! Provides `ContextBuilder` for assembling relevant context from gathered content
//! and injecting it into LLM prompts.

pub mod injection;
mod service;

pub use service::{ContextService, GatheringResult, SearchResultWithAnalysis};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::content::{ContentCategory, ContentItem};
use crate::embeddings::SearchResult;

/// Configuration for context building
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum tokens for the assembled context
    pub token_budget: usize,
    /// Minimum relevance threshold (0.0-1.0)
    pub relevance_threshold: f32,
    /// Whether to include metadata summaries for excluded items
    pub include_metadata_summaries: bool,
    /// Categories to prioritize (in order)
    pub priority_categories: Vec<ContentCategory>,
    /// Specific source IDs to include (None = all)
    pub source_ids: Option<Vec<Uuid>>,
    /// Maximum items to include
    pub max_items: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            token_budget: 50_000, // Leave room for prompt and response
            relevance_threshold: 0.5,
            include_metadata_summaries: true,
            priority_categories: vec![
                ContentCategory::File,
                ContentCategory::Document,
                ContentCategory::Communication,
            ],
            source_ids: None,
            max_items: 50,
        }
    }
}

/// Assembled context ready for prompt injection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembledContext {
    /// The formatted context text
    pub text: String,
    /// Token count of the context
    pub token_count: usize,
    /// Items included in context
    pub included_items: Vec<ContextIncludedItem>,
    /// Items excluded (and why)
    pub excluded_items: Vec<ContextExcludedItem>,
    /// Statistics about context assembly
    pub stats: ContextStats,
    /// When this context was assembled
    pub assembled_at: DateTime<Utc>,
}

impl AssembledContext {
    /// Create an empty context
    pub fn empty() -> Self {
        Self {
            text: String::new(),
            token_count: 0,
            included_items: Vec::new(),
            excluded_items: Vec::new(),
            stats: ContextStats::default(),
            assembled_at: Utc::now(),
        }
    }

    /// Check if context is empty
    pub fn is_empty(&self) -> bool {
        self.included_items.is_empty()
    }
}

/// An item included in the assembled context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextIncludedItem {
    /// Content item ID
    pub content_item_id: Uuid,
    /// Source ID
    pub source_id: Uuid,
    /// Item title
    pub title: String,
    /// Item URI
    pub uri: String,
    /// Relevance score
    pub relevance_score: f32,
    /// Token contribution to context
    pub token_contribution: usize,
    /// Chunk IDs included (if chunked)
    pub chunk_ids: Vec<Uuid>,
}

/// An item excluded from the assembled context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextExcludedItem {
    /// Content item ID
    pub content_item_id: Uuid,
    /// Item title
    pub title: String,
    /// Reason for exclusion
    pub reason: ExclusionReason,
}

/// Reasons for excluding content from context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Below relevance threshold
    BelowRelevanceThreshold { score: f32, threshold: f32 },
    /// Token budget would be exceeded
    TokenBudgetExceeded {
        tokens_needed: usize,
        budget_remaining: usize,
    },
    /// Filtered by category
    FilteredByCategory { category: String },
    /// Filtered by source
    FilteredBySource { source_id: Uuid },
    /// Low quality score
    LowQuality { score: f32 },
    /// Maximum items reached
    MaxItemsReached,
    /// Duplicate of included content
    Duplicate { duplicate_of: Uuid },
}

/// Statistics about context assembly
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextStats {
    /// Total candidate items considered
    pub total_candidates: usize,
    /// Items included
    pub included_count: usize,
    /// Items excluded
    pub excluded_count: usize,
    /// Total tokens in context
    pub total_tokens: usize,
    /// Token budget utilization (0.0-1.0)
    pub budget_utilization: f32,
    /// Assembly time in milliseconds
    pub assembly_time_ms: u64,
}

/// Builder for assembling context from search results and content
pub struct ContextBuilder {
    config: ContextConfig,
}

impl ContextBuilder {
    /// Create a new context builder with config
    pub fn new(config: ContextConfig) -> Self {
        Self { config }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(ContextConfig::default())
    }

    /// Get the config
    pub fn config(&self) -> &ContextConfig {
        &self.config
    }

    /// Build context from search results
    ///
    /// This is the main entry point for context assembly.
    pub fn build_from_results(
        &self,
        results: &[SearchResult],
        _content_lookup: &dyn Fn(Uuid) -> Option<ContentItem>,
    ) -> AssembledContext {
        let start = std::time::Instant::now();
        let mut context = AssembledContext::empty();
        let mut used_tokens = 0;

        // Filter and sort by relevance
        let mut sorted_results: Vec<_> = results
            .iter()
            .filter(|r| r.similarity >= self.config.relevance_threshold)
            .collect();
        sorted_results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

        for result in sorted_results {
            // Check max items
            if context.included_items.len() >= self.config.max_items {
                context.excluded_items.push(ContextExcludedItem {
                    content_item_id: result.content_item_id,
                    title: result.item_title.clone(),
                    reason: ExclusionReason::MaxItemsReached,
                });
                continue;
            }

            // Estimate tokens for this result
            let tokens = crate::content::estimate_tokens(&result.chunk_text);

            // Check budget
            if used_tokens + tokens > self.config.token_budget {
                context.excluded_items.push(ContextExcludedItem {
                    content_item_id: result.content_item_id,
                    title: result.item_title.clone(),
                    reason: ExclusionReason::TokenBudgetExceeded {
                        tokens_needed: tokens,
                        budget_remaining: self.config.token_budget.saturating_sub(used_tokens),
                    },
                });
                continue;
            }

            // Add to context
            context.included_items.push(ContextIncludedItem {
                content_item_id: result.content_item_id,
                source_id: result.source_id,
                title: result.item_title.clone(),
                uri: result.item_uri.clone(),
                relevance_score: result.similarity,
                token_contribution: tokens,
                chunk_ids: vec![result.chunk_id],
            });

            used_tokens += tokens;
        }

        // Format the context text
        context.text = self.format_context(&context.included_items, results);
        context.token_count = used_tokens;

        // Update stats
        context.stats = ContextStats {
            total_candidates: results.len(),
            included_count: context.included_items.len(),
            excluded_count: context.excluded_items.len(),
            total_tokens: used_tokens,
            budget_utilization: used_tokens as f32 / self.config.token_budget as f32,
            assembly_time_ms: start.elapsed().as_millis() as u64,
        };

        context
    }

    /// Format context items into text
    fn format_context(&self, items: &[ContextIncludedItem], results: &[SearchResult]) -> String {
        let mut text = String::from("## Relevant Context\n\n");

        for item in items {
            // Find the corresponding result
            let result = results
                .iter()
                .find(|r| r.content_item_id == item.content_item_id);

            text.push_str(&format!("### {}\n", item.title));
            text.push_str(&format!("*Source: {}*\n\n", item.uri));

            if let Some(r) = result {
                text.push_str(&r.chunk_text);
                text.push_str("\n\n");
            }

            text.push_str("---\n\n");
        }

        text
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::default_config()
    }
}

/// Inject context into a system prompt
pub fn inject_context(system_prompt: &str, context: &AssembledContext) -> String {
    if context.is_empty() {
        return system_prompt.to_string();
    }

    format!("{}\n\n{}", system_prompt, context.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_config_default() {
        let config = ContextConfig::default();
        assert_eq!(config.token_budget, 50_000);
        assert_eq!(config.relevance_threshold, 0.5);
        assert_eq!(config.max_items, 50);
    }

    #[test]
    fn test_assembled_context_empty() {
        let context = AssembledContext::empty();
        assert!(context.is_empty());
        assert!(context.text.is_empty());
        assert_eq!(context.token_count, 0);
    }

    #[test]
    fn test_context_builder_new() {
        let config = ContextConfig {
            token_budget: 10_000,
            ..Default::default()
        };
        let builder = ContextBuilder::new(config);
        assert_eq!(builder.config().token_budget, 10_000);
    }

    #[test]
    fn test_inject_context_empty() {
        let prompt = "You are an assistant.";
        let context = AssembledContext::empty();
        let result = inject_context(prompt, &context);
        assert_eq!(result, prompt);
    }

    #[test]
    fn test_inject_context_with_content() {
        let prompt = "You are an assistant.";
        let mut context = AssembledContext::empty();
        context.text = "## Relevant Context\n\nSome context here.".to_string();
        context.included_items.push(ContextIncludedItem {
            content_item_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            title: "Test".to_string(),
            uri: "test.rs".to_string(),
            relevance_score: 0.9,
            token_contribution: 100,
            chunk_ids: vec![],
        });

        let result = inject_context(prompt, &context);
        assert!(result.contains(prompt));
        assert!(result.contains("Relevant Context"));
    }
}
