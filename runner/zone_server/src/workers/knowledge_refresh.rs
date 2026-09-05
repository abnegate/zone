//! Background worker for refreshing web-linked knowledge entries
//!
//! Periodically checks for knowledge entries that have source URLs and
//! are due for refresh based on their refresh_interval_minutes setting.

use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::db::knowledge;
use crate::state::AppState;

/// Maximum concurrent refresh operations
const MAX_CONCURRENT_REFRESHES: usize = 3;

/// Interval between refresh checks (5 minutes)
const REFRESH_CHECK_INTERVAL_SECS: u64 = 300;

/// Maximum entries to process per cycle
const MAX_ENTRIES_PER_CYCLE: i64 = 50;

/// Timeout for HTTP requests
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Maximum content size (1MB)
const MAX_CONTENT_SIZE: usize = 1_048_576;

/// Start the knowledge refresh worker
///
/// This spawns a background task that periodically checks for knowledge
/// entries that need refreshing and updates their content.
pub fn start_refresh_worker(state: AppState) {
    tokio::spawn(async move {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REFRESHES));

        loop {
            // Sleep before checking (allows server startup to complete)
            tokio::time::sleep(Duration::from_secs(REFRESH_CHECK_INTERVAL_SECS)).await;

            tracing::debug!("Knowledge refresh worker: checking for entries to refresh");

            // Find entries due for refresh
            let entries =
                match knowledge::list_entries_due_for_refresh(state.db(), MAX_ENTRIES_PER_CYCLE)
                    .await
                {
                    Ok(entries) => entries,
                    Err(e) => {
                        tracing::error!("Failed to list entries for refresh: {}", e);
                        continue;
                    }
                };

            if entries.is_empty() {
                tracing::debug!("Knowledge refresh worker: no entries due for refresh");
                continue;
            }

            tracing::info!(
                "Knowledge refresh worker: found {} entries to refresh",
                entries.len()
            );

            // Process entries concurrently with semaphore limiting
            for entry in entries {
                let state_clone = state.clone();
                let semaphore_clone = semaphore.clone();

                tokio::spawn(async move {
                    let _permit = match semaphore_clone.acquire().await {
                        Ok(p) => p,
                        Err(_) => {
                            tracing::error!(
                                "Failed to acquire refresh semaphore for entry {}",
                                entry.id
                            );
                            return;
                        }
                    };

                    refresh_entry(&state_clone, entry).await;
                });
            }
        }
    });
}

/// Refresh a single knowledge entry
async fn refresh_entry(state: &AppState, entry: knowledge::KnowledgeRefreshDue) {
    tracing::info!(
        "Refreshing knowledge entry: id={}, title='{}', url='{}'",
        entry.id,
        entry.title,
        entry.source_url
    );

    // Fetch content from URL
    let (content, new_hash) = match fetch_web_content(&entry.source_url).await {
        Ok((content, hash)) => (content, hash),
        Err(e) => {
            tracing::warn!("Failed to fetch URL for entry {}: {}", entry.id, e);
            // Record the error
            if let Err(db_err) = knowledge::record_fetch_error(state.db(), entry.id, &e).await {
                tracing::error!(
                    "Failed to record fetch error for entry {}: {}",
                    entry.id,
                    db_err
                );
            }
            return;
        }
    };

    // Check if content changed (compare hashes)
    let content_changed = entry.content_hash.as_ref() != Some(&new_hash);

    if !content_changed {
        tracing::debug!(
            "Content unchanged for entry {} (hash: {})",
            entry.id,
            new_hash
        );
        // Still update last_fetched_at but don't regenerate embeddings
        // We do this by updating with the same content
        if let Err(e) = knowledge::update_knowledge_content(
            state.db(),
            entry.id,
            &content,
            zone_context::content::estimate_tokens(&content) as i32,
            &new_hash,
        )
        .await
        {
            tracing::error!("Failed to update timestamp for entry {}: {}", entry.id, e);
        }
        return;
    }

    tracing::info!(
        "Content changed for entry {} (old: {:?}, new: {})",
        entry.id,
        entry.content_hash,
        new_hash
    );

    // Calculate token count
    let token_count = zone_context::content::estimate_tokens(&content) as i32;

    // Update content in database
    if let Err(e) =
        knowledge::update_knowledge_content(state.db(), entry.id, &content, token_count, &new_hash)
            .await
    {
        tracing::error!("Failed to update content for entry {}: {}", entry.id, e);
        return;
    }

    // Regenerate embedding if service available
    if let Some(embedding_service) = state.embedding_service() {
        match embedding_service.embed(&content).await {
            Ok(embedding) => {
                let model = embedding_service.model();
                if let Err(e) = knowledge::store_knowledge_embedding(
                    state.db(),
                    entry.id,
                    entry.workspace_id,
                    &embedding,
                    model,
                )
                .await
                {
                    tracing::warn!("Failed to update embedding for entry {}: {}", entry.id, e);
                } else {
                    tracing::info!("Updated embedding for entry {}", entry.id);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to generate embedding for entry {}: {}", entry.id, e);
            }
        }
    }

    tracing::info!(
        "Successfully refreshed entry {}: {} tokens",
        entry.id,
        token_count
    );
}

/// Fetch content from a web URL and extract text
///
/// Returns the extracted text content and its SHA-256 hash.
async fn fetch_web_content(url: &str) -> Result<(String, String), String> {
    let url = crate::utils::url::validate_public_url(url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .header("User-Agent", "Zone/1.0 (Knowledge Refresh Worker)")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    crate::utils::url::validate_public_url(response.url().as_str())?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    // Check content length
    if let Some(len) = response.content_length()
        && len as usize > MAX_CONTENT_SIZE
    {
        return Err(format!(
            "Content too large: {} bytes (max: {})",
            len, MAX_CONTENT_SIZE
        ));
    }

    // Get content type before consuming the response
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Check actual size
    if body.len() > MAX_CONTENT_SIZE {
        return Err(format!(
            "Content too large: {} bytes (max: {})",
            body.len(),
            MAX_CONTENT_SIZE
        ));
    }

    // Extract text based on content type
    let text = if content_type.contains("text/html") {
        extract_text_from_html(&body)
    } else {
        body
    };

    // Calculate content hash
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hash = hex::encode(hasher.finalize());

    Ok((text, hash))
}

/// Extract text content from HTML
fn extract_text_from_html(html: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Try to find main content areas
    let main_selectors = [
        "article",
        "main",
        "[role=\"main\"]",
        ".content",
        ".post-content",
        ".article-content",
        "#content",
    ];

    for selector_str in &main_selectors {
        if let Ok(selector) = Selector::parse(selector_str)
            && let Some(element) = document.select(&selector).next()
        {
            let text = extract_text_from_element(&element);
            if !text.trim().is_empty() {
                return clean_text(&text);
            }
        }
    }

    // Fallback: get body text
    if let Ok(body_selector) = Selector::parse("body")
        && let Some(body) = document.select(&body_selector).next()
    {
        return clean_text(&extract_text_from_element(&body));
    }

    clean_text(&document.root_element().text().collect::<String>())
}

/// Extract text from HTML element, skipping non-content elements
fn extract_text_from_element(element: &scraper::ElementRef) -> String {
    let mut text = String::new();

    for node in element.children() {
        if let Some(element_ref) = scraper::ElementRef::wrap(node) {
            let tag = element_ref.value().name();
            if matches!(
                tag,
                "script" | "style" | "nav" | "header" | "footer" | "aside" | "noscript"
            ) {
                continue;
            }
            text.push_str(&extract_text_from_element(&element_ref));
        } else if let Some(text_node) = node.value().as_text() {
            text.push_str(text_node);
        }
    }

    text
}

/// Clean extracted text (normalize whitespace)
fn clean_text(text: &str) -> String {
    let mut result = String::new();
    let mut last_was_whitespace = false;

    for c in text.chars() {
        if c.is_whitespace() {
            if !last_was_whitespace {
                result.push(' ');
                last_was_whitespace = true;
            }
        } else {
            result.push(c);
            last_was_whitespace = false;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_text() {
        assert_eq!(clean_text("  hello   world  "), "hello world");
        assert_eq!(clean_text("\n\nhello\n\nworld\n\n"), "hello world");
        assert_eq!(clean_text("hello\t\tworld"), "hello world");
    }

    #[test]
    fn test_extract_text_from_html_basic() {
        let html = r#"
            <html>
            <body>
                <article>
                    <h1>Title</h1>
                    <p>Content here</p>
                </article>
            </body>
            </html>
        "#;

        let text = extract_text_from_html(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Content here"));
    }

    #[test]
    fn test_extract_text_skips_scripts() {
        let html = r#"
            <html>
            <body>
                <script>alert('test');</script>
                <p>Visible content</p>
                <style>.hidden { display: none; }</style>
            </body>
            </html>
        "#;

        let text = extract_text_from_html(html);
        assert!(!text.contains("alert"));
        assert!(!text.contains("hidden"));
        assert!(text.contains("Visible content"));
    }

    #[test]
    fn test_extract_text_skips_navigation() {
        let html = r#"
            <html>
            <body>
                <nav>Navigation links</nav>
                <main>
                    <p>Main content</p>
                </main>
                <footer>Footer info</footer>
            </body>
            </html>
        "#;

        let text = extract_text_from_html(html);
        assert!(text.contains("Main content"));
        assert!(!text.contains("Navigation links"));
        assert!(!text.contains("Footer info"));
    }
}
