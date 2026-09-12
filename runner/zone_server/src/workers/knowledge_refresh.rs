//! Keeping a workspace's knowledge current.
//!
//! One pass does two things. It takes the entries whose
//! `refresh_interval_minutes` has elapsed and re-fetches each one, and it
//! embeds the entries that have no stored vector at all. The cadence belongs to
//! [`crate::workers::housekeeping`]; what is due, and what refreshing an entry
//! means, belongs here.

use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

use crate::db::{DbResult, knowledge};
use crate::services;
use crate::state::AppState;

/// Maximum concurrent refresh operations
const MAX_CONCURRENT_REFRESHES: usize = 3;

/// Interval between refresh checks (5 minutes)
pub const REFRESH_CHECK_INTERVAL_SECS: u64 = 300;

/// Maximum entries to process per cycle
const MAX_ENTRIES_PER_CYCLE: i64 = 50;

/// Entries one pass will try to embed.
///
/// Smaller than the refresh bite because a backlog here is finite: it is
/// whatever an embedding outage happened to catch, and every pass that answers
/// takes another bite out of it.
pub const MAX_UNINDEXED_PER_CYCLE: i64 = 25;

const _: () = assert!(
    MAX_UNINDEXED_PER_CYCLE > 0 && MAX_UNINDEXED_PER_CYCLE <= MAX_ENTRIES_PER_CYCLE,
    "a recovery pass has to be bounded, and no larger than the refresh bite it shares a \
     cadence and a semaphore with"
);

/// Passes to sit out after the first one that could embed nothing.
const FIRST_BACKOFF_PASSES: u32 = 1;

/// The longest the recovery pass will sit out.
///
/// Sixteen passes is eighty minutes at this cadence. A dead embedding service
/// is asked again that often rather than every five minutes, and the first pass
/// that gets an answer clears the whole thing.
const MAX_BACKOFF_PASSES: u32 = 16;

/// Timeout for HTTP requests
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Maximum content size (1MB)
const MAX_CONTENT_SIZE: usize = 1_048_576;

/// The concurrency one pass shares across the entries it starts.
///
/// Held by the worker rather than created per pass, so a pass that starts while
/// the previous one still has fetches in flight cannot double the load on the
/// sites being refreshed.
pub fn permits() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(MAX_CONCURRENT_REFRESHES))
}

/// What one recovery pass carries over from the last one.
///
/// Held across passes rather than derived from a column: both parts describe
/// this process's dealings with the embedding service, and an entry's own row
/// says nothing about them. The cadence is fixed, so a count of passes is a
/// duration.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Passes still owed before this one may look.
    waiting: u32,
    /// Passes to owe after the next one that embeds nothing.
    next_wait: u32,
    /// How far down the backlog the next pass starts.
    ///
    /// A bounded pass that always started at the oldest row would spend every
    /// pass on the same head, and never reach the rest of the backlog, if that
    /// head is entries the service will not accept. The cursor moves on only
    /// while passes keep failing, and goes back to the oldest row as soon as
    /// one succeeds or the backlog runs out.
    cursor: i64,
}

impl Recovery {
    /// Whether this pass looks, spending one of the passes owed if not.
    fn ready(&mut self) -> bool {
        if self.waiting == 0 {
            return true;
        }

        self.waiting -= 1;
        false
    }

    fn cursor(&self) -> i64 {
        self.cursor
    }

    /// A pass embedded nothing it tried. Wait longer, and start the next pass
    /// past the entries this one could not embed.
    fn embedded_nothing(&mut self, attempted: i64) {
        self.next_wait = self
            .next_wait
            .saturating_mul(2)
            .clamp(FIRST_BACKOFF_PASSES, MAX_BACKOFF_PASSES);
        self.waiting = self.next_wait;
        self.cursor = if attempted < MAX_UNINDEXED_PER_CYCLE {
            0
        } else {
            self.cursor.saturating_add(attempted)
        };
    }

    /// The cursor has run past the end of the backlog. Start over from the
    /// oldest row, still owing whatever the last failure owed.
    fn rewound(&mut self) {
        self.cursor = 0;
    }

    /// The embedding service answered. Nothing is owed and nothing is skipped.
    fn answered(&mut self) {
        *self = Self::default();
    }
}

/// The recovery pass's state, held by the worker for the same reason
/// [`permits`] is.
pub fn recovery() -> Arc<Mutex<Recovery>> {
    Arc::new(Mutex::new(Recovery::default()))
}

/// Bring every entry that has come due, or was never indexed, up to date.
pub async fn run_cycle(
    state: &AppState,
    permits: &Arc<Semaphore>,
    recovery: &Arc<Mutex<Recovery>>,
) -> DbResult<()> {
    refresh_due(state, permits).await?;
    recover_unindexed(state, permits, recovery).await
}

/// Start a refresh for every entry that has come due.
async fn refresh_due(state: &AppState, permits: &Arc<Semaphore>) -> DbResult<()> {
    tracing::debug!("Knowledge refresh worker: checking for entries to refresh");

    let entries =
        knowledge::list_entries_due_for_refresh(state.db(), MAX_ENTRIES_PER_CYCLE).await?;

    if entries.is_empty() {
        tracing::debug!("Knowledge refresh worker: no entries due for refresh");
        return Ok(());
    }

    tracing::info!(
        "Knowledge refresh worker: found {} entries to refresh",
        entries.len()
    );

    // The housekeeping slot for this worker stays taken until the pass
    // returns, which is what stops two passes running at once. Detaching the
    // work here returns before any of it has happened, so the slot frees on a
    // pass that has not started fetching and the next tick re-reads the same
    // rows -- `last_refreshed` only moves when a refresh finishes. Fifty
    // entries through three permits outlast the five-minute cadence easily,
    // and each pass then re-fetches URLs the previous one is still fetching.
    let mut refreshes = JoinSet::new();

    for entry in entries {
        let state = state.clone();
        let permits = Arc::clone(permits);

        refreshes.spawn(async move {
            let Ok(_permit) = permits.acquire().await else {
                tracing::error!("Failed to acquire refresh semaphore for entry {}", entry.id);
                return;
            };

            refresh_entry(&state, entry).await;
        });
    }

    while let Some(refresh) = refreshes.join_next().await {
        if let Err(error) = refresh {
            tracing::error!("A knowledge refresh did not finish: {error}");
        }
    }

    Ok(())
}

/// Embed the active entries that have no stored vector.
///
/// An entry whose embedding fails at creation is still stored, because failing
/// the write would lose the text the user had just given us. Nothing then ever
/// looked at it again: the refresh pass above only revisits entries that have a
/// `source_url`, the manual `/refresh` route refuses an entry that has none,
/// and there is no update route. One transient embedding outage therefore took
/// an entry out of semantic search permanently, and silently, because keyword
/// search kept finding it. This is the pass that puts it back.
///
/// Every candidate is tried, rather than stopping at the first failure, so one
/// entry the service will never accept cannot hold up the whole pass; and a
/// failed pass moves the cursor on, so a head of such entries longer than one
/// pass cannot starve the rest of the backlog either.
///
/// One pass, so a caller can drive the recovery on its own rather than through
/// the whole cycle.
pub async fn recover_unindexed(
    state: &AppState,
    permits: &Arc<Semaphore>,
    recovery: &Arc<Mutex<Recovery>>,
) -> DbResult<()> {
    if state.embedding_service().is_none() {
        tracing::debug!("Knowledge recovery: no embedding service, nothing to index");
        return Ok(());
    }

    let cursor = {
        let mut recovery = recovery.lock().await;
        if !recovery.ready() {
            tracing::debug!("Knowledge recovery: waiting out a pass that could embed nothing");
            return Ok(());
        }
        recovery.cursor()
    };

    let entries =
        knowledge::list_entries_missing_embeddings(state.db(), MAX_UNINDEXED_PER_CYCLE, cursor)
            .await?;

    if entries.is_empty() {
        let mut recovery = recovery.lock().await;
        if cursor == 0 {
            recovery.answered();
            tracing::debug!("Knowledge recovery: every active entry has a stored vector");
        } else {
            recovery.rewound();
            tracing::debug!("Knowledge recovery: reached the end of the backlog; starting over");
        }
        return Ok(());
    }

    tracing::info!(
        "Knowledge recovery: embedding {} entries with no stored vector, from offset {}",
        entries.len(),
        cursor
    );

    let mut indexing = JoinSet::new();

    for entry in entries {
        let state = state.clone();
        let permits = Arc::clone(permits);

        indexing.spawn(async move {
            let Ok(_permit) = permits.acquire().await else {
                tracing::error!("Failed to acquire refresh semaphore for entry {}", entry.id);
                return false;
            };

            match services::knowledge::index(&state, entry.id, entry.workspace_id, &entry.content)
                .await
            {
                Ok(()) => {
                    // The id and nothing else: a title is workspace content,
                    // and a server log is not where it belongs.
                    tracing::info!(
                        "Indexed knowledge entry {}, which had no stored vector",
                        entry.id
                    );
                    true
                }
                Err(error) => {
                    tracing::warn!("Failed to index knowledge entry {}: {error}", entry.id);
                    false
                }
            }
        });
    }

    let mut indexed = 0_i64;
    let mut attempted = 0_i64;

    while let Some(outcome) = indexing.join_next().await {
        attempted += 1;
        match outcome {
            Ok(true) => indexed += 1,
            Ok(false) => {}
            Err(error) => tracing::error!("A knowledge indexing task did not finish: {error}"),
        }
    }

    let mut recovery = recovery.lock().await;

    if indexed == 0 {
        recovery.embedded_nothing(attempted);
        tracing::warn!(
            "Knowledge recovery: none of {} entries could be embedded; waiting {} pass(es), \
             resuming at offset {}",
            attempted,
            recovery.waiting,
            recovery.cursor
        );
    } else {
        recovery.answered();
        tracing::info!(
            "Knowledge recovery: indexed {} of {} entries",
            indexed,
            attempted
        );
    }

    Ok(())
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

    // New content wants a new vector. A failure here leaves the entry indexed
    // against its previous text, which the recovery pass cannot see -- it looks
    // for entries with no vector at all -- so it is the next refresh that
    // corrects it.
    match services::knowledge::index(state, entry.id, entry.workspace_id, &content).await {
        Ok(()) => tracing::info!("Updated embedding for entry {}", entry.id),
        Err(services::knowledge::Unindexed::NoService) => {}
        Err(error) => {
            tracing::warn!("Failed to update embedding for entry {}: {error}", entry.id)
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
    let client = crate::utils::url::public_client(Duration::from_secs(HTTP_TIMEOUT_SECS))?;

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

    // Get content type before consuming the response
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let body =
        String::from_utf8_lossy(&crate::utils::url::read_capped(response, MAX_CONTENT_SIZE).await?)
            .into_owned();

    let text = if is_html(content_type.as_deref(), &body) {
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

/// Whether the body should have its markup stripped before being stored.
///
/// A missing header is not evidence the body is plain text. Without the sniff,
/// a server that omits Content-Type -- or sends one with a byte that is not
/// ASCII, which cannot be read as a string -- gets its markup stored verbatim
/// as the entry's text, embedded, and handed to the agent as knowledge. Media
/// types are case-insensitive, so `TEXT/HTML` has to match too.
fn is_html(content_type: Option<&str>, body: &str) -> bool {
    match content_type {
        Some(declared) => {
            let declared = declared.to_ascii_lowercase();
            declared.contains("text/html") || declared.contains("application/xhtml")
        }
        None => {
            let start = body.trim_start().to_ascii_lowercase();
            start.starts_with("<!doctype html") || start.starts_with("<html")
        }
    }
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
/// Text of an element and its descendants, walked iteratively.
///
/// Recursion here descends once per level of nesting, and the fetch cap admits
/// a megabyte of markup — enough for hundreds of thousands of nested divs. A
/// Rust stack overflow is a SIGSEGV rather than an unwinding panic, so it takes
/// the process with it and the worker restarts onto the same row, which is a
/// crash loop driven by a workspace-supplied URL.
fn extract_text_from_element(element: &scraper::ElementRef) -> String {
    const SKIPPED: [&str; 7] = [
        "script", "style", "nav", "header", "footer", "aside", "noscript",
    ];

    let mut text = String::new();
    let mut pending: Vec<_> = element.children().rev().collect();

    while let Some(node) = pending.pop() {
        if let Some(element_ref) = scraper::ElementRef::wrap(node) {
            if SKIPPED.contains(&element_ref.value().name()) {
                continue;
            }
            pending.extend(element_ref.children().rev());
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

    /// Passes the recovery sits out before it looks again, given a run of
    /// full passes that could embed nothing.
    fn passes_sat_out(failures: usize) -> Vec<u32> {
        let mut recovery = Recovery::default();
        let mut waited = Vec::new();

        for _ in 0..failures {
            assert!(
                recovery.ready(),
                "a pass that has paid its wait has to be allowed to look"
            );
            recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE);

            let mut passes = 0;
            while !recovery.ready() {
                passes += 1;
                assert!(passes <= MAX_BACKOFF_PASSES, "backoff never let a pass run");
            }
            waited.push(passes);
        }

        waited
    }

    #[test]
    fn a_dead_embedding_service_is_asked_less_and_less_often() {
        assert_eq!(
            passes_sat_out(8),
            vec![1, 2, 4, 8, 16, 16, 16, 16],
            "a pass that embeds nothing has to wait longer than the one before it, up to a \
             cap -- otherwise an embedding service that is down is asked again every five \
             minutes for as long as it stays down, once per unindexed entry per pass"
        );
    }

    #[test]
    fn the_first_pass_after_an_answer_owes_nothing() {
        let mut recovery = Recovery::default();

        assert!(
            recovery.ready(),
            "nothing is owed before anything has failed"
        );
        recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE);
        recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE);
        assert!(
            !recovery.ready(),
            "two failures in a row owe more than one pass"
        );

        recovery.answered();

        assert_eq!(
            recovery,
            Recovery::default(),
            "an embedding service that has answered is not still being backed off from, and \
             starts again at the oldest entry it could not reach"
        );
        assert!(recovery.ready());
    }

    /// Spend whatever passes the recovery owes, then the offset the pass that
    /// does look will read from.
    fn offset_of_next_look(recovery: &mut Recovery) -> i64 {
        let mut spent = 0;
        while !recovery.ready() {
            spent += 1;
            assert!(
                spent <= MAX_BACKOFF_PASSES,
                "the recovery never let a pass look"
            );
        }

        recovery.cursor()
    }

    #[test]
    fn a_head_of_entries_that_never_embed_does_not_starve_the_backlog() {
        let mut recovery = Recovery::default();
        let mut looked_at = Vec::new();

        // Four full passes, none of which embeds anything: a cursor that did
        // not move would spend all four on the same rows, and the entries
        // behind them would never be attempted at all.
        for _ in 0..4 {
            looked_at.push(offset_of_next_look(&mut recovery));
            recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE);
        }

        assert_eq!(
            looked_at,
            vec![
                0,
                MAX_UNINDEXED_PER_CYCLE,
                2 * MAX_UNINDEXED_PER_CYCLE,
                3 * MAX_UNINDEXED_PER_CYCLE
            ],
            "each failed pass has to start where the last one stopped"
        );

        // A short bite is the end of the backlog, so the next pass starts over
        // rather than reading past it forever.
        recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE - 1);
        assert_eq!(recovery.cursor(), 0);

        // And one entry embedded means the head is worth reading again: what
        // succeeded is no longer a candidate.
        recovery.cursor = 3 * MAX_UNINDEXED_PER_CYCLE;
        recovery.answered();
        assert_eq!(recovery.cursor(), 0);
    }

    #[test]
    fn running_off_the_end_of_the_backlog_keeps_the_wait_it_owed() {
        let mut recovery = Recovery::default();
        recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE);
        recovery.embedded_nothing(MAX_UNINDEXED_PER_CYCLE);
        let owed = recovery.next_wait;

        recovery.rewound();

        assert_eq!(recovery.cursor(), 0);
        assert_eq!(
            recovery.next_wait, owed,
            "reaching the end of the backlog is not the embedding service answering, so the \
             wait a dead service earned has to survive the rewind"
        );
    }

    #[test]
    fn a_page_is_stripped_of_markup_even_when_the_server_will_not_say_it_is_html() {
        assert!(is_html(Some("text/html; charset=utf-8"), ""));

        // Media types are case-insensitive, and this one used to fall through
        // to the raw-body branch.
        assert!(is_html(Some("TEXT/HTML"), ""));
        assert!(is_html(Some("application/xhtml+xml"), ""));

        // No header at all, or one carrying a byte that cannot be read as a
        // string: the body is the only evidence left.
        assert!(is_html(None, "<!DOCTYPE html><html><body>hi</body></html>"));
        assert!(is_html(None, "\n  <html><body>hi</body></html>"));

        assert!(!is_html(
            Some("text/plain"),
            "<html>not actually served as html</html>"
        ));
        assert!(!is_html(
            None,
            "# A markdown document\n\nwith a <span> in it"
        ));
    }
}
