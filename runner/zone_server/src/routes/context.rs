//! Context gathering and search routes

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{
        StatusCode,
        header::{HeaderMap, HeaderName},
    },
    response::IntoResponse,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::ErrorResponse;
use crate::auth::AuthUser;
use crate::db::workspace_members;
use crate::state::AppState;
use sqlx::PgPool;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GatherRequest {
    source_ids: Vec<Uuid>,
    workspace_id: Uuid,
    #[serde(default)]
    force_refresh: bool,
}

#[derive(Debug, Serialize)]
pub struct GatherResponse {
    gathering_id: Uuid,
    sources_queued: usize,
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
    workspace_id: Uuid,
    #[serde(default = "default_limit")]
    limit: usize,
    source_ids: Option<String>, // comma-separated
    categories: Option<String>, // comma-separated
    threshold: Option<f32>,
    // Hybrid search parameters
    #[serde(default = "default_search_mode")]
    mode: String, // "hybrid", "semantic", "keyword"
    semantic_weight: Option<f32>, // 0.0-1.0, default 0.7 for hybrid mode
    rrf_k: Option<f32>,           // RRF constant, default 60
    min_keyword_score: Option<f32>, // Minimum keyword score, default 0.0
    min_semantic_score: Option<f32>, // Minimum semantic score, default 0.5
}

fn default_search_mode() -> String {
    "hybrid".to_string()
}

const MAX_SEARCH_LIMIT: usize = 100;
const MAX_QUERY_LENGTH: usize = 1000;
const MIN_QUERY_LENGTH: usize = 1;
const MAX_SOURCE_IDS: usize = 50;
const SNIPPET_MAX_LENGTH: usize = 200;

// Knowledge validation constants
const MAX_TITLE_LENGTH: usize = 256;
const MAX_CONTENT_LENGTH: usize = 1_000_000; // 1MB
const MAX_TAGS_COUNT: usize = 20;
const MAX_TAG_LENGTH: usize = 64;

fn default_limit() -> usize {
    10
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    results: Vec<SearchResultItem>,
    query: String,
    total_results: usize,
    search_time_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    chunk_id: Uuid,
    content_item_id: Uuid,
    source_id: Uuid,
    similarity: f32,
    title: String,
    uri: String,
    snippet: String,
}

#[derive(Debug, Deserialize)]
pub struct ListKnowledgeQuery {
    workspace_id: Uuid,
    category: Option<String>,
    #[serde(default = "default_knowledge_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_knowledge_limit() -> usize {
    50
}

const MAX_KNOWLEDGE_LIMIT: usize = 100;

#[derive(Debug, Deserialize)]
pub struct CreateKnowledgeRequest {
    workspace_id: Uuid,
    title: String,
    /// Content text (optional if source_url is provided)
    content: Option<String>,
    category: Option<String>,
    tags: Option<Vec<String>>,
    /// Optional URL to fetch content from
    source_url: Option<String>,
    /// Auto-refresh interval in minutes (only for URL-based entries)
    refresh_interval_minutes: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeResponse {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    content: String,
    category: Option<String>,
    tags: Vec<String>,
    token_count: usize,
    is_active: bool,
    /// Source URL if this knowledge was fetched from a web link
    #[serde(skip_serializing_if = "Option::is_none")]
    source_url: Option<String>,
    /// When the URL content was last fetched
    #[serde(skip_serializing_if = "Option::is_none")]
    last_fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Auto-refresh interval in minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_interval_minutes: Option<i32>,
    /// Last fetch error if any
    #[serde(skip_serializing_if = "Option::is_none")]
    last_fetch_error: Option<String>,
}

/// Lightweight knowledge entry for list responses (without full content)
#[derive(Debug, Serialize)]
pub struct KnowledgeListItem {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    category: Option<String>,
    tags: Vec<String>,
    token_count: usize,
    is_active: bool,
    /// Source URL if this knowledge was fetched from a web link
    #[serde(skip_serializing_if = "Option::is_none")]
    source_url: Option<String>,
    /// When the URL content was last fetched
    #[serde(skip_serializing_if = "Option::is_none")]
    last_fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Auto-refresh interval in minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_interval_minutes: Option<i32>,
    /// Last fetch error if any
    #[serde(skip_serializing_if = "Option::is_none")]
    last_fetch_error: Option<String>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Verify that a user has access to a workspace
async fn verify_workspace_access(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, sqlx::Error> {
    workspace_members::is_member(pool, user_id, workspace_id).await
}

/// Verify that source IDs belong to a workspace
async fn verify_source_ownership(
    pool: &PgPool,
    workspace_id: Uuid,
    source_ids: &[Uuid],
) -> Result<bool, sqlx::Error> {
    workspace_members::verify_sources_in_workspace(pool, workspace_id, source_ids).await
}

// ============================================================================
// Route Handlers
// ============================================================================

/// POST /api/context/gather
/// Trigger context gathering for specified sources
///
/// For now, returns a queued status. Actual gathering will happen via WebSocket.
///
/// Rate limited: 10 requests per minute per user
pub async fn gather(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<GatherRequest>,
) -> impl IntoResponse {
    // Extract user ID from auth claims
    let user_id = match auth.0.user_id() {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Failed to parse user ID from auth claims");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Authentication error")),
            )
                .into_response();
        }
    };

    // Check rate limit
    let rate_limiter = state.rate_limiter();
    let (allowed, remaining, reset_at) = rate_limiter.check_rate_limit(user_id);

    // Create rate limit headers
    let mut headers = HeaderMap::new();
    let rate_limit_max = rate_limiter.config().max_requests;
    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        rate_limit_max.to_string().parse().unwrap(),
    );
    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        remaining.to_string().parse().unwrap(),
    );
    // Calculate seconds until reset (reset_at is in the future)
    let now = std::time::Instant::now();
    let reset_seconds = if reset_at > now {
        (reset_at - now).as_secs()
    } else {
        0
    };
    headers.insert(
        HeaderName::from_static("x-ratelimit-reset"),
        reset_seconds.to_string().parse().unwrap(),
    );

    if !allowed {
        tracing::warn!("Rate limit exceeded for user_id: {}", user_id);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            Json(ErrorResponse::new(
                "Rate limit exceeded. Please try again later.",
            )),
        )
            .into_response();
    }

    // Verify workspace access
    let db = state.db();
    match verify_workspace_access(db, user_id, req.workspace_id).await {
        Ok(has_access) if has_access => {}
        Ok(_) => {
            tracing::warn!(
                "User {} attempted to access workspace {}",
                user_id,
                req.workspace_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Access denied")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    // Verify source ownership
    // Note: This will require workspace_id on sources table to work properly
    match verify_source_ownership(db, req.workspace_id, &req.source_ids).await {
        Ok(owns_all) if owns_all => {}
        Ok(_) => {
            tracing::warn!(
                "User {} attempted to access sources not in workspace {}",
                user_id,
                req.workspace_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Access denied")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking source ownership: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    // Create gathering record in database
    use crate::db::context_gatherings;
    let gathering_id =
        match context_gatherings::create_gathering(db, user_id, req.workspace_id, &req.source_ids)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to create gathering record: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Failed to create gathering")),
                )
                    .into_response();
            }
        };

    let sources_queued = req.source_ids.len();

    tracing::info!(
        "Context gathering queued: id={}, user_id={}, workspace_id={}, sources={}, force_refresh={}",
        gathering_id,
        user_id,
        req.workspace_id,
        sources_queued,
        req.force_refresh
    );

    // Spawn background task to execute gathering with panic handling
    let state_clone = state.clone();
    let source_ids = req.source_ids.clone();
    let workspace_id = req.workspace_id;
    let force_refresh = req.force_refresh;
    tokio::spawn(async move {
        use crate::workers::gathering;

        // Wrap in panic handler to prevent panics from killing the task
        let result = std::panic::AssertUnwindSafe(gathering::execute_gathering(
            &state_clone,
            gathering_id,
            workspace_id,
            source_ids,
            force_refresh,
        ))
        .catch_unwind()
        .await;

        if result.is_err() {
            tracing::error!("CRITICAL: Gathering {} panicked", gathering_id);
            // Update gathering status to failed on panic
            let _ = context_gatherings::update_gathering_status(
                state_clone.db(),
                gathering_id,
                "failed",
                Some("Internal panic during gathering"),
            )
            .await;
        }
    });

    let response = GatherResponse {
        gathering_id,
        sources_queued,
        message: format!(
            "Context gathering queued for {} source(s). Use WebSocket to track progress.",
            sources_queued
        ),
    };

    (StatusCode::ACCEPTED, headers, Json(response)).into_response()
}

/// GET /api/context/search
/// Semantic search across gathered content
///
/// Performs similarity search using embeddings.
pub async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    // Extract user ID for authorization
    let user_id = match auth.0.user_id() {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Failed to parse user ID from auth claims");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Authentication error")),
            )
                .into_response();
        }
    };

    // Verify workspace access
    let db = state.db();
    match verify_workspace_access(db, user_id, query.workspace_id).await {
        Ok(has_access) if has_access => {}
        Ok(_) => {
            tracing::warn!(
                "User {} attempted to search in workspace {}",
                user_id,
                query.workspace_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Access denied")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    // Trim query and validate length
    let trimmed_query = query.q.trim();
    if trimmed_query.is_empty() || trimmed_query.len() > MAX_QUERY_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Query cannot be empty or whitespace only, and must not exceed {} characters",
                MAX_QUERY_LENGTH
            ))),
        )
            .into_response();
    }

    // Clamp limit to maximum
    let limit = query.limit.min(MAX_SEARCH_LIMIT);

    // Clamp threshold to valid range [0.0, 1.0]
    let threshold = query.threshold.map(|t| t.clamp(0.0, 1.0));

    tracing::info!(
        "Context search: user_id={}, workspace_id={}, query_length={}, limit={}, threshold={:?}",
        user_id,
        query.workspace_id,
        trimmed_query.len(),
        limit,
        threshold
    );

    // Parse optional filters
    let source_ids: Option<Vec<Uuid>> = query.source_ids.as_ref().and_then(|s| {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() > MAX_SOURCE_IDS {
            return None; // Will trigger "Invalid source_ids format" error
        }
        parts
            .into_iter()
            .map(|id| id.trim().parse::<Uuid>().ok())
            .collect::<Option<Vec<_>>>()
    });

    if query.source_ids.is_some() && source_ids.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Invalid source_ids format or exceeds maximum of {} IDs",
                MAX_SOURCE_IDS
            ))),
        )
            .into_response();
    }

    // Reject categories filter as it's not implemented yet
    if query.categories.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "Category filtering is not yet implemented",
            )),
        )
            .into_response();
    }

    // Verify user has access to requested source_ids
    if let Some(ref ids) = source_ids {
        match verify_source_ownership(db, query.workspace_id, ids).await {
            Ok(owns_all) if owns_all => {}
            Ok(_) => {
                tracing::warn!(
                    "User {} attempted to search sources not in workspace {}",
                    user_id,
                    query.workspace_id
                );
                return (
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse::new("Access denied to specified sources")),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("Database error checking source ownership: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    // Get context service
    let context_service = match state.context_service() {
        Some(svc) => svc,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse::new("Search service not available")),
            )
                .into_response();
        }
    };

    // Build search filters
    let filters = zone_context::embeddings::SearchFilters {
        source_ids: source_ids.clone(),
        workspace_id: Some(query.workspace_id),
        categories: None,
        min_quality: threshold,
        since: None,
    };

    // Determine search mode
    let search_mode = query.mode.to_lowercase();

    // Perform search based on mode
    let results_vec = match search_mode.as_str() {
        "hybrid" => {
            // Build hybrid search config
            let hybrid_config = zone_context::HybridSearchConfig {
                semantic_weight: query.semantic_weight.unwrap_or(0.7).clamp(0.0, 1.0),
                rrf_k: query.rrf_k.unwrap_or(60.0),
                min_keyword_score: query.min_keyword_score.unwrap_or(0.0).clamp(0.0, 1.0),
                min_semantic_score: query.min_semantic_score.unwrap_or(0.5).clamp(0.0, 1.0),
            };

            tracing::info!(
                "Hybrid search: semantic_weight={}, rrf_k={}, min_keyword={}, min_semantic={}",
                hybrid_config.semantic_weight,
                hybrid_config.rrf_k,
                hybrid_config.min_keyword_score,
                hybrid_config.min_semantic_score
            );

            match context_service
                .search_hybrid(trimmed_query, limit, Some(filters), Some(hybrid_config))
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Hybrid search failed: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Search failed")),
                    )
                        .into_response();
                }
            }
        }
        "keyword" => {
            let min_score = query.min_keyword_score.unwrap_or(0.0).clamp(0.0, 1.0);
            tracing::info!("Keyword-only search: min_score={}", min_score);

            match context_service
                .search_keyword_only(trimmed_query, limit, Some(filters), min_score)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Keyword search failed: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Search failed")),
                    )
                        .into_response();
                }
            }
        }
        "semantic" => {
            let min_similarity = query.min_semantic_score.unwrap_or(0.5).clamp(0.0, 1.0);
            tracing::info!("Semantic-only search: min_similarity={}", min_similarity);

            match context_service
                .search_semantic_only(trimmed_query, limit, Some(filters), min_similarity)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Semantic search failed: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new("Search failed")),
                    )
                        .into_response();
                }
            }
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Invalid search mode. Use 'hybrid', 'semantic', or 'keyword'",
                )),
            )
                .into_response();
        }
    };

    // Map to response
    let result_items: Vec<SearchResultItem> = results_vec
        .into_iter()
        .map(|r| SearchResultItem {
            chunk_id: r.chunk_id,
            content_item_id: r.content_item_id,
            source_id: r.source_id,
            similarity: r.similarity,
            title: r.item_title,
            uri: r.item_uri,
            snippet: truncate_snippet(&r.chunk_text, SNIPPET_MAX_LENGTH),
        })
        .collect();

    let total_results = result_items.len();
    let search_time_ms = start.elapsed().as_millis() as u64;

    let response = SearchResponse {
        results: result_items,
        query: trimmed_query.to_string(),
        total_results,
        search_time_ms,
    };

    Json(response).into_response()
}

/// Helper function to truncate snippet at UTF-8 character boundaries
fn truncate_snippet(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let boundary = text
            .char_indices()
            .take_while(|(i, _)| *i < max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max_len);
        format!("{}...", &text[..boundary])
    }
}

/// GET /api/knowledge
/// List knowledge base entries
///
/// Returns user-added knowledge for a workspace.
pub async fn list_knowledge(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListKnowledgeQuery>,
) -> impl IntoResponse {
    // Extract user ID for authorization
    let user_id = match auth.0.user_id() {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Failed to parse user ID from auth claims");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Authentication error")),
            )
                .into_response();
        }
    };

    // Verify workspace read access
    let db = state.db();
    match workspace_members::can_read(db, query.workspace_id, user_id).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(
                "User {} attempted to read knowledge in workspace {}",
                user_id,
                query.workspace_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Access denied")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking workspace read access: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    // Clamp limit to maximum
    let limit = query.limit.min(MAX_KNOWLEDGE_LIMIT);

    tracing::info!(
        "List knowledge: user_id={}, workspace_id={}, category={:?}, limit={}, offset={}",
        user_id,
        query.workspace_id,
        query.category,
        limit,
        query.offset
    );

    // Query database for knowledge entries
    use crate::db::knowledge;
    let entries = match knowledge::list_knowledge(
        db,
        query.workspace_id,
        query.category.as_deref(),
        limit as i64,
        query.offset as i64,
    )
    .await
    {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!("Failed to list knowledge entries: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to list knowledge entries")),
            )
                .into_response();
        }
    };

    // Map to response (using KnowledgeListItem which excludes full content)
    let knowledge: Vec<KnowledgeListItem> = entries
        .into_iter()
        .map(|entry| KnowledgeListItem {
            id: entry.id,
            workspace_id: entry.workspace_id,
            title: entry.title,
            category: entry.category,
            tags: entry.tags,
            token_count: entry.token_count as usize,
            is_active: entry.is_active,
            source_url: entry.source_url,
            last_fetched_at: entry
                .last_fetched_at
                .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc)),
            refresh_interval_minutes: entry.refresh_interval_minutes,
            last_fetch_error: entry.last_fetch_error,
        })
        .collect();

    Json(knowledge).into_response()
}

/// Maximum URL length
const MAX_URL_LENGTH: usize = 2048;

/// POST /api/knowledge
/// Create knowledge base entry
///
/// Adds user-defined knowledge to the workspace context.
/// Supports both direct content and web URL-based entries.
pub async fn create_knowledge(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateKnowledgeRequest>,
) -> impl IntoResponse {
    // Extract user ID for authorization
    let user_id = match auth.0.user_id() {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Failed to parse user ID from auth claims");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Authentication error")),
            )
                .into_response();
        }
    };

    // Validate input
    if req.title.is_empty() || req.title.len() > MAX_TITLE_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Title must be between 1 and {} characters",
                MAX_TITLE_LENGTH
            ))),
        )
            .into_response();
    }

    // Must have either content or source_url
    let has_content = req.content.as_ref().is_some_and(|c| !c.is_empty());
    let has_url = req.source_url.as_ref().is_some_and(|u| !u.is_empty());

    if !has_content && !has_url {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "Either content or source_url is required",
            )),
        )
            .into_response();
    }

    // Validate content if provided
    if let Some(ref content) = req.content
        && content.len() > MAX_CONTENT_LENGTH
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Content must not exceed {} characters",
                MAX_CONTENT_LENGTH
            ))),
        )
            .into_response();
    }

    // Validate URL if provided
    if let Some(ref url) = req.source_url {
        if url.len() > MAX_URL_LENGTH {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(format!(
                    "URL must not exceed {} characters",
                    MAX_URL_LENGTH
                ))),
            )
                .into_response();
        }
        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "URL must start with http:// or https://",
                )),
            )
                .into_response();
        }
    }

    // Validate refresh interval
    if let Some(interval) = req.refresh_interval_minutes {
        if interval < 0 {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("Refresh interval cannot be negative")),
            )
                .into_response();
        }
        // Max 30 days
        if interval > 43200 {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "Refresh interval cannot exceed 30 days (43200 minutes)",
                )),
            )
                .into_response();
        }
    }

    if let Some(ref tags) = req.tags {
        if tags.len() > MAX_TAGS_COUNT {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(format!(
                    "Maximum {} tags allowed",
                    MAX_TAGS_COUNT
                ))),
            )
                .into_response();
        }

        for tag in tags {
            if tag.is_empty() || tag.len() > MAX_TAG_LENGTH {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(format!(
                        "Each tag must be between 1 and {} characters",
                        MAX_TAG_LENGTH
                    ))),
                )
                    .into_response();
            }
        }
    }

    // Verify workspace write access
    let db = state.db();
    match workspace_members::can_write(db, req.workspace_id, user_id).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(
                "User {} attempted to create knowledge in workspace {}",
                user_id,
                req.workspace_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Access denied")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking workspace write access: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    use crate::db::knowledge;

    // Check for duplicate URL in workspace
    if let Some(ref url) = req.source_url {
        match knowledge::url_exists_in_workspace(db, req.workspace_id, url).await {
            Ok(Some(_existing_id)) => {
                return (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse::new(
                        "This URL already exists in the knowledge base",
                    )),
                )
                    .into_response();
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!("Database error checking URL existence: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    tracing::info!(
        "Create knowledge: user_id={}, workspace_id={}, title='{}', has_url={}",
        user_id,
        req.workspace_id,
        req.title,
        has_url
    );

    // Determine content: fetch from URL or use provided content
    let (final_content, content_hash, is_url_based) = if let Some(ref url) = req.source_url {
        // Fetch content from URL
        match fetch_web_content(url).await {
            Ok((content, hash)) => (content, Some(hash), true),
            Err(e) => {
                tracing::warn!("Failed to fetch URL content: {}", e);
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(format!("Failed to fetch URL: {}", e))),
                )
                    .into_response();
            }
        }
    } else {
        // Use provided content
        let content = req.content.clone().unwrap_or_default();
        (content, None, false)
    };

    // Validate fetched content length
    if final_content.len() > MAX_CONTENT_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Fetched content exceeds maximum {} characters",
                MAX_CONTENT_LENGTH
            ))),
        )
            .into_response();
    }

    // Calculate token count with safe conversion
    let token_count: i32 = zone_context::content::estimate_tokens(&final_content)
        .try_into()
        .unwrap_or(i32::MAX);

    // Insert into database
    let entry_id = if is_url_based {
        match knowledge::create_knowledge_with_url(
            db,
            req.workspace_id,
            &req.title,
            &final_content,
            req.source_url.as_ref().unwrap(),
            req.category.as_deref(),
            &req.tags.clone().unwrap_or_default(),
            token_count,
            content_hash.as_ref().unwrap(),
            req.refresh_interval_minutes,
            user_id,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to create knowledge entry: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Failed to create knowledge entry")),
                )
                    .into_response();
            }
        }
    } else {
        match knowledge::create_knowledge(
            db,
            req.workspace_id,
            &req.title,
            &final_content,
            req.category.as_deref(),
            &req.tags.clone().unwrap_or_default(),
            token_count,
            user_id,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to create knowledge entry: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Failed to create knowledge entry")),
                )
                    .into_response();
            }
        }
    };

    // Generate and store embedding (if service available)
    if let Some(embedding_service) = state.embedding_service() {
        match embedding_service.embed(&final_content).await {
            Ok(embedding) => {
                let model = embedding_service.model();
                if let Err(e) = knowledge::store_knowledge_embedding(
                    db,
                    entry_id,
                    req.workspace_id,
                    &embedding,
                    model,
                )
                .await
                {
                    tracing::warn!("Failed to store knowledge embedding: {}", e);
                    // Don't fail the request - embedding is optional enhancement
                }
            }
            Err(e) => {
                tracing::warn!("Failed to generate knowledge embedding: {}", e);
                // Don't fail the request - embedding is optional enhancement
            }
        }
    }

    let response = KnowledgeResponse {
        id: entry_id,
        workspace_id: req.workspace_id,
        title: req.title,
        content: final_content,
        category: req.category,
        tags: req.tags.unwrap_or_default(),
        token_count: token_count as usize,
        is_active: true,
        source_url: req.source_url.clone(),
        last_fetched_at: if is_url_based {
            Some(chrono::Utc::now())
        } else {
            None
        },
        refresh_interval_minutes: req.refresh_interval_minutes,
        last_fetch_error: None,
    };

    (StatusCode::CREATED, Json(response)).into_response()
}

/// Fetch content from a web URL and extract text
///
/// Returns the extracted text content and its SHA-256 hash.
async fn fetch_web_content(url: &str) -> Result<(String, String), String> {
    use sha2::{Digest, Sha256};

    // Timeout for fetching
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .header("User-Agent", "Zone/1.0 (Knowledge Fetcher)")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
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

    // Extract text based on content type
    let text = if content_type.contains("text/html") {
        extract_text_from_html(&body)
    } else {
        // Plain text or other - use as-is
        body
    };

    // Calculate content hash
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hash = hex::encode(hasher.finalize());

    Ok((text, hash))
}

/// Extract text content from HTML, removing scripts, styles, and navigation
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

    // Last resort: all text
    clean_text(&document.root_element().text().collect::<String>())
}

/// Extract text from an HTML element, skipping script/style/nav elements
fn extract_text_from_element(element: &scraper::ElementRef) -> String {
    let mut text = String::new();

    for node in element.children() {
        if let Some(element_ref) = scraper::ElementRef::wrap(node) {
            let tag = element_ref.value().name();
            // Skip non-content elements
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

/// Clean extracted text (normalize whitespace, etc.)
fn clean_text(text: &str) -> String {
    // Replace multiple whitespace with single space
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

/// DELETE /api/knowledge/:id
/// Delete knowledge base entry
///
/// Soft-deletes a knowledge entry (sets is_active = false).
pub async fn delete_knowledge(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Extract user ID for authorization
    let user_id = match auth.0.user_id() {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Failed to parse user ID from auth claims");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Authentication error")),
            )
                .into_response();
        }
    };

    let db = state.db();

    // Fetch the knowledge entry to get its workspace_id
    use crate::db::knowledge;
    let knowledge_entry = match knowledge::get_knowledge(db, id).await {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            tracing::warn!("Knowledge entry not found: id={}", id);
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(e) => {
            tracing::error!("Database error fetching knowledge: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    };

    // Verify user has write access to the knowledge entry's workspace
    match workspace_members::can_write(db, knowledge_entry.workspace_id, user_id).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(
                "User {} attempted to delete knowledge in workspace {}",
                user_id,
                knowledge_entry.workspace_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::new("Access denied")),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Database error checking workspace write access: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response();
        }
    }

    // Soft delete the knowledge entry
    match knowledge::delete_knowledge(db, id).await {
        Ok(deleted) if deleted => {
            tracing::info!("Deleted knowledge: user_id={}, id={}", user_id, id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(_) => {
            // Entry was already deleted or didn't exist
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            tracing::error!("Database error deleting knowledge: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}
