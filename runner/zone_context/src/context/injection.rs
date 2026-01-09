//! Context injection for agent prompts
//!
//! Provides utilities for injecting assembled context into LLM prompts
//! with various formatting options optimized for agent consumption.

use crate::context::AssembledContext;

/// Inject assembled context into a system prompt
///
/// Formats the context for optimal LLM consumption with clear section headers.
pub fn inject_context(system_prompt: &str, context: &AssembledContext) -> String {
    if context.is_empty() {
        return system_prompt.to_string();
    }

    format!(
        "{}\n\n## Relevant Context\n\n{}\n\n---\n\n*{} items included, {} tokens*",
        system_prompt, context.text, context.stats.included_count, context.stats.total_tokens
    )
}

/// Create a context-aware system prompt extension
///
/// Generates a structured section describing the available context,
/// including metadata about sources and relevance scores.
pub fn create_context_section(context: &AssembledContext) -> String {
    if context.is_empty() {
        return String::new();
    }

    let mut section = String::from("## Available Context\n\n");
    section.push_str("The following context has been gathered from connected sources:\n\n");

    for item in &context.included_items {
        section.push_str(&format!(
            "### {} (relevance: {:.0}%)\n",
            item.title,
            item.relevance_score * 100.0
        ));
        section.push_str(&format!("Source: {}\n\n", item.uri));
    }

    section
}

/// Create a compact context summary
///
/// Useful for logging or displaying context information without the full text.
pub fn create_context_summary(context: &AssembledContext) -> String {
    if context.is_empty() {
        return "No context available".to_string();
    }

    format!(
        "Context: {} items, {} tokens, {:.1}% budget used",
        context.stats.included_count,
        context.stats.total_tokens,
        context.stats.budget_utilization * 100.0
    )
}

/// Format context for inclusion in a user message
///
/// Wraps context in a user-friendly format suitable for injection into
/// conversation messages rather than system prompts.
pub fn format_for_user_message(context: &AssembledContext) -> String {
    if context.is_empty() {
        return String::new();
    }

    let mut message = String::from("I've gathered some relevant information:\n\n");

    for (idx, item) in context.included_items.iter().enumerate() {
        message.push_str(&format!(
            "{}. **{}** ({}% relevant)\n",
            idx + 1,
            item.title,
            (item.relevance_score * 100.0) as u8
        ));
    }

    message.push_str(&format!(
        "\nTotal: {} items, {} tokens",
        context.stats.included_count, context.stats.total_tokens
    ));

    message
}

/// Create a context injection optimized for code-related tasks
///
/// Emphasizes structure and organization for code context.
pub fn inject_code_context(system_prompt: &str, context: &AssembledContext) -> String {
    if context.is_empty() {
        return system_prompt.to_string();
    }

    let mut code_section = String::from("## Codebase Context\n\n");
    code_section.push_str("The following code files and documentation are relevant:\n\n");

    for item in &context.included_items {
        code_section.push_str(&format!("### `{}`\n", item.uri));
        code_section.push_str(&format!(
            "Relevance: {:.0}%\n\n",
            item.relevance_score * 100.0
        ));
    }

    code_section.push_str(&context.text);
    code_section.push_str(&format!(
        "\n\n---\n*{} files, {} tokens total*\n",
        context.stats.included_count, context.stats.total_tokens
    ));

    format!("{}\n\n{}", system_prompt, code_section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextIncludedItem, ContextStats};
    use uuid::Uuid;

    fn create_test_context() -> AssembledContext {
        let mut context = AssembledContext::empty();
        context.text = "Test content here.".to_string();
        context.included_items.push(ContextIncludedItem {
            content_item_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
            title: "test.rs".to_string(),
            uri: "src/test.rs".to_string(),
            relevance_score: 0.85,
            token_contribution: 100,
            chunk_ids: vec![],
        });
        context.stats = ContextStats {
            total_candidates: 10,
            included_count: 1,
            excluded_count: 9,
            total_tokens: 100,
            budget_utilization: 0.002,
            assembly_time_ms: 50,
        };
        context
    }

    #[test]
    fn test_inject_context_empty() {
        let prompt = "You are a helpful assistant.";
        let context = AssembledContext::empty();
        let result = inject_context(prompt, &context);
        assert_eq!(result, prompt);
    }

    #[test]
    fn test_inject_context_with_content() {
        let prompt = "You are a helpful assistant.";
        let context = create_test_context();
        let result = inject_context(prompt, &context);

        assert!(result.contains(prompt));
        assert!(result.contains("Relevant Context"));
        assert!(result.contains("1 items included"));
        assert!(result.contains("100 tokens"));
    }

    #[test]
    fn test_create_context_section() {
        let context = create_test_context();
        let section = create_context_section(&context);

        assert!(section.contains("Available Context"));
        assert!(section.contains("test.rs"));
        assert!(section.contains("85%"));
        assert!(section.contains("src/test.rs"));
    }

    #[test]
    fn test_create_context_section_empty() {
        let context = AssembledContext::empty();
        let section = create_context_section(&context);
        assert!(section.is_empty());
    }

    #[test]
    fn test_create_context_summary() {
        let context = create_test_context();
        let summary = create_context_summary(&context);

        assert!(summary.contains("1 items"));
        assert!(summary.contains("100 tokens"));
        assert!(summary.contains("0.2% budget used"));
    }

    #[test]
    fn test_create_context_summary_empty() {
        let context = AssembledContext::empty();
        let summary = create_context_summary(&context);
        assert_eq!(summary, "No context available");
    }

    #[test]
    fn test_format_for_user_message() {
        let context = create_test_context();
        let message = format_for_user_message(&context);

        assert!(message.contains("gathered some relevant information"));
        assert!(message.contains("test.rs"));
        assert!(message.contains("85% relevant"));
    }

    #[test]
    fn test_format_for_user_message_empty() {
        let context = AssembledContext::empty();
        let message = format_for_user_message(&context);
        assert!(message.is_empty());
    }

    #[test]
    fn test_inject_code_context() {
        let prompt = "You are a coding assistant.";
        let context = create_test_context();
        let result = inject_code_context(prompt, &context);

        assert!(result.contains(prompt));
        assert!(result.contains("Codebase Context"));
        assert!(result.contains("`src/test.rs`"));
        assert!(result.contains("85%"));
    }

    #[test]
    fn test_inject_code_context_empty() {
        let prompt = "You are a coding assistant.";
        let context = AssembledContext::empty();
        let result = inject_code_context(prompt, &context);
        assert_eq!(result, prompt);
    }
}
