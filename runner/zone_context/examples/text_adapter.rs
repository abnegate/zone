//! Example usage of the TextAdapter
//!
//! Run with: cargo run -p zone_context --example text_adapter

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use zone_context::adapters::{NoOpProgress, SourceAdapter, TextAdapter};
use zone_context::{FetchConfig, FetchStrategy};
use zone_core::{Source, SourceCategory, SourceType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("TextAdapter Example\n");

    // Create a text adapter
    let adapter = TextAdapter::new();
    println!("Created TextAdapter with type: {}", adapter.source_type());

    // Create a text source with inline content
    let source = Source {
        id: Uuid::new_v4(),
        name: "Example Text".to_string(),
        source_type: SourceType::Text,
        category: SourceCategory::Text,
        config: json!({
            "content": "This is inline text content that doesn't require an external source. \
                        It's perfect for quick testing, ad-hoc content injection, or \
                        user-provided text snippets.",
            "label": "Example Text Content"
        }),
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Step 1: Verify the source
    println!("\n1. Verifying source configuration...");
    adapter.verify(&source).await?;
    println!("   ✓ Source verified successfully");

    // Step 2: Estimate tokens
    println!("\n2. Estimating token count...");
    let token_count = adapter.estimate_tokens(&source).await?;
    println!("   ✓ Estimated tokens: {}", token_count);

    // Step 3: Fetch content
    println!("\n3. Fetching content...");
    let config = FetchConfig::default();
    let progress = NoOpProgress;
    let result = adapter
        .fetch(&source, &config, FetchStrategy::Full, &progress)
        .await?;

    println!("   ✓ Fetched {} item(s)", result.items.len());
    println!("   ✓ Total tokens: {}", result.stats.total_tokens);

    // Display the fetched content
    if let Some(item) = result.items.first() {
        println!("\n4. Content Details:");
        println!("   Title: {}", item.title);
        println!("   URI: {}", item.uri);
        println!("   Category: {:?}", item.category);
        println!("   Token count: {}", item.token_count);
        println!("   Metadata only: {}", item.metadata_only);

        if let Some(content) = &item.content {
            println!("\n   Content preview:");
            let preview = if content.len() > 100 {
                format!("{}...", &content[..100])
            } else {
                content.clone()
            };
            println!("   {}", preview);
        }
    }

    // Example with metadata-only fetch
    println!("\n5. Fetching metadata only...");
    let metadata_result = adapter
        .fetch(&source, &config, FetchStrategy::MetadataOnly, &progress)
        .await?;

    if let Some(item) = metadata_result.items.first() {
        println!("   ✓ Item fetched with metadata only");
        println!("   Content included: {}", item.content.is_some());
        println!("   Metadata only flag: {}", item.metadata_only);
    }

    println!("\n✓ Example completed successfully!");

    Ok(())
}
