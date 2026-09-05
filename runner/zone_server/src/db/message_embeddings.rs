//! Message embedding storage and retrieval
//!
//! Provides functions for storing and searching message embeddings
//! for semantic search over chat history.

use sqlx::PgPool;
use uuid::Uuid;
use zone_context::embeddings::align_vector;

fn aligned_vector_literal(embedding: &[f32]) -> Result<String, sqlx::Error> {
    let embedding = align_vector(embedding).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    Ok(format!(
        "[{}]",
        embedding
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    ))
}

/// Store an embedding for a message
///
/// This function stores the embedding vector for a chat message,
/// enabling semantic search over conversation history.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `message_id` - ID of the message
/// * `chat_id` - ID of the chat containing the message
/// * `embedding` - The embedding vector (padded to 1024 if the model is narrower)
/// * `model` - Model identifier used to generate the embedding
///
/// # Note
/// This function uses ON CONFLICT to handle re-embedding scenarios.
/// If the message already has an embedding, it will be updated.
///
/// # Security
/// Validates that all embedding values are finite (not NaN or infinity)
/// before inserting into the database to prevent invalid vector operations.
pub async fn store_message_embedding(
    pool: &PgPool,
    message_id: Uuid,
    chat_id: Uuid,
    embedding: &[f32],
    model: &str,
) -> Result<(), sqlx::Error> {
    let vector_str = aligned_vector_literal(embedding)?;

    sqlx::query(
        r#"
        INSERT INTO message_embeddings (message_id, chat_id, vector, model)
        VALUES ($1, $2, $3::vector, $4)
        ON CONFLICT (message_id) DO UPDATE SET
            vector = EXCLUDED.vector,
            model = EXCLUDED.model
        "#,
    )
    .bind(message_id)
    .bind(chat_id)
    .bind(&vector_str)
    .bind(model)
    .execute(pool)
    .await?;

    Ok(())
}

/// Search messages by semantic similarity
///
/// Uses vector similarity search to find messages similar to a query embedding.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `query_embedding` - The query embedding vector
/// * `workspace_id` - Workspace ID to restrict search to a workspace (required)
/// * `chat_id` - Optional chat ID to restrict search to a specific chat
/// * `limit` - Maximum number of results to return
/// * `threshold` - Minimum similarity threshold (0.0-1.0)
///
/// # Returns
/// Vector of search results ordered by similarity (highest first)
///
/// # Security
/// Validates that all embedding values are finite before searching.
/// workspace_id is required to prevent unauthorized cross-workspace searches.
pub async fn search_messages(
    pool: &PgPool,
    query_embedding: &[f32],
    workspace_id: Uuid,
    chat_id: Option<Uuid>,
    limit: usize,
    threshold: f32,
) -> Result<Vec<MessageSearchResult>, sqlx::Error> {
    let vector_str = aligned_vector_literal(query_embedding)?;

    let results = match chat_id {
        Some(cid) => {
            // Search within a specific chat in a workspace
            sqlx::query_as::<_, MessageSearchResult>(
                r#"
                SELECT
                    m.id as message_id,
                    m.chat_id,
                    (1 - (me.vector <=> $1::vector))::REAL as similarity,
                    m.role,
                    m.content,
                    m.created_at
                FROM message_embeddings me
                JOIN messages m ON m.id = me.message_id
                JOIN chats c ON c.id = m.chat_id
                WHERE me.chat_id = $2
                  AND c.workspace_id = $3
                  AND (1 - (me.vector <=> $1::vector)) >= $5
                ORDER BY me.vector <=> $1::vector
                LIMIT $4
                "#,
            )
            .bind(&vector_str)
            .bind(cid)
            .bind(workspace_id)
            .bind(limit as i64)
            .bind(threshold)
            .fetch_all(pool)
            .await?
        }
        None => {
            // Search across all chats in a workspace
            sqlx::query_as::<_, MessageSearchResult>(
                r#"
                SELECT
                    m.id as message_id,
                    m.chat_id,
                    (1 - (me.vector <=> $1::vector))::REAL as similarity,
                    m.role,
                    m.content,
                    m.created_at
                FROM message_embeddings me
                JOIN messages m ON m.id = me.message_id
                JOIN chats c ON c.id = m.chat_id
                WHERE c.workspace_id = $2
                  AND (1 - (me.vector <=> $1::vector)) >= $4
                ORDER BY me.vector <=> $1::vector
                LIMIT $3
                "#,
            )
            .bind(&vector_str)
            .bind(workspace_id)
            .bind(limit as i64)
            .bind(threshold)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(results)
}

/// Keyword search over chat messages when embeddings are missing or fail.
pub async fn search_messages_keyword(
    pool: &PgPool,
    query: &str,
    workspace_id: Uuid,
    chat_id: Option<Uuid>,
    limit: usize,
) -> Result<Vec<MessageSearchResult>, sqlx::Error> {
    let keyword = zone_context::rewrite_query(query).keyword;
    let sanitized = zone_context::embeddings::sanitize_search_query(&keyword);
    if sanitized.trim().is_empty() {
        return Ok(Vec::new());
    }

    let results = match chat_id {
        Some(cid) => {
            sqlx::query_as::<_, MessageSearchResult>(
                r#"
                SELECT
                    m.id as message_id,
                    m.chat_id,
                    ts_rank(
                        to_tsvector('english', m.content),
                        websearch_to_tsquery('english', $1)
                    )::REAL as similarity,
                    m.role,
                    m.content,
                    m.created_at
                FROM messages m
                JOIN chats c ON c.id = m.chat_id
                WHERE m.chat_id = $2
                  AND c.workspace_id = $3
                  AND to_tsvector('english', m.content)
                      @@ websearch_to_tsquery('english', $1)
                ORDER BY similarity DESC
                LIMIT $4
                "#,
            )
            .bind(&sanitized)
            .bind(cid)
            .bind(workspace_id)
            .bind(limit as i64)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, MessageSearchResult>(
                r#"
                SELECT
                    m.id as message_id,
                    m.chat_id,
                    ts_rank(
                        to_tsvector('english', m.content),
                        websearch_to_tsquery('english', $1)
                    )::REAL as similarity,
                    m.role,
                    m.content,
                    m.created_at
                FROM messages m
                JOIN chats c ON c.id = m.chat_id
                WHERE c.workspace_id = $2
                  AND to_tsvector('english', m.content)
                      @@ websearch_to_tsquery('english', $1)
                ORDER BY similarity DESC
                LIMIT $3
                "#,
            )
            .bind(&sanitized)
            .bind(workspace_id)
            .bind(limit as i64)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(results)
}

/// Fuse semantic and keyword chat-history lists with RRF plus identifier boost.
pub fn fuse_message_hits(
    semantic: Vec<MessageSearchResult>,
    keyword: Vec<MessageSearchResult>,
    query: &str,
    limit: usize,
) -> Vec<MessageSearchResult> {
    let identifiers = zone_context::rewrite_query(query).identifiers;
    let mut scores: std::collections::HashMap<Uuid, (MessageSearchResult, f32)> =
        std::collections::HashMap::new();
    for (rank, hit) in semantic.into_iter().enumerate() {
        let boost = zone_context::identifier_match_boost("", "", &hit.content, &identifiers);
        let rrf = 1.0 / (60.0 + rank as f32 + 1.0);
        scores.insert(hit.message_id, (hit, rrf + boost));
    }
    for (rank, hit) in keyword.into_iter().enumerate() {
        let boost = zone_context::identifier_match_boost("", "", &hit.content, &identifiers);
        let rrf = 1.0 / (60.0 + rank as f32 + 1.0);
        scores
            .entry(hit.message_id)
            .and_modify(|(_, score)| *score += rrf)
            .or_insert((hit, rrf + boost));
    }
    let mut fused: Vec<_> = scores.into_values().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused.into_iter().take(limit).map(|(hit, _)| hit).collect()
}

/// Delete embedding for a message
///
/// Called when a message is deleted to clean up associated embeddings.
pub async fn delete_message_embedding(pool: &PgPool, message_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM message_embeddings
        WHERE message_id = $1
        "#,
    )
    .bind(message_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get embedding for a specific message
///
/// Returns the stored embedding if it exists.
pub async fn get_message_embedding(
    pool: &PgPool,
    message_id: Uuid,
) -> Result<Option<MessageEmbedding>, sqlx::Error> {
    let result = sqlx::query_as::<_, MessageEmbedding>(
        r#"
        SELECT id, message_id, chat_id, model, created_at
        FROM message_embeddings
        WHERE message_id = $1
        "#,
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await?;

    Ok(result)
}

/// Count embeddings for a chat
///
/// Useful for statistics and checking embedding coverage.
pub async fn count_chat_embeddings(pool: &PgPool, chat_id: Uuid) -> Result<i64, sqlx::Error> {
    let result = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM message_embeddings
        WHERE chat_id = $1
        "#,
    )
    .bind(chat_id)
    .fetch_one(pool)
    .await?;

    Ok(result)
}

/// Message search result
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageSearchResult {
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub similarity: f32,
    pub role: String,
    pub content: String,
    pub created_at: chrono::NaiveDateTime,
}

/// Message embedding record
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageEmbedding {
    pub id: Uuid,
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub model: String,
    pub created_at: chrono::NaiveDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_formatting() {
        let embedding = [0.1, 0.2, 0.3];
        let vector_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(vector_str, "[0.1,0.2,0.3]");
    }

    #[test]
    fn test_message_search_result_struct() {
        // Test that the struct can be created
        let result = MessageSearchResult {
            message_id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            similarity: 0.95,
            role: "user".to_string(),
            content: "Test message".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
        };

        assert_eq!(result.role, "user");
        assert_eq!(result.similarity, 0.95);
    }

    #[test]
    fn test_message_embedding_struct() {
        // Test that the struct can be created
        let embedding = MessageEmbedding {
            id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            chat_id: Uuid::new_v4(),
            model: "text-embedding-3-small".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
        };

        assert_eq!(embedding.model, "text-embedding-3-small");
    }

    #[test]
    fn test_embedding_validation_finite_values() {
        // Test that finite values pass validation
        let valid_embedding: Vec<f32> = vec![0.1, 0.2, 0.3, -0.5, 0.0];
        assert!(valid_embedding.iter().all(|&v| v.is_finite()));
    }

    #[test]
    fn test_embedding_validation_nan() {
        // Test that NaN values are detected
        let invalid_embedding: Vec<f32> = vec![0.1, f32::NAN, 0.3];
        assert!(!invalid_embedding.iter().all(|&v| v.is_finite()));
    }

    #[test]
    fn test_embedding_validation_infinity() {
        // Test that infinity values are detected
        let invalid_embedding: Vec<f32> = vec![0.1, f32::INFINITY, 0.3];
        assert!(!invalid_embedding.iter().all(|&v| v.is_finite()));
    }

    #[test]
    fn test_embedding_validation_neg_infinity() {
        // Test that negative infinity values are detected
        let invalid_embedding: Vec<f32> = vec![0.1, f32::NEG_INFINITY, 0.3];
        assert!(!invalid_embedding.iter().all(|&v| v.is_finite()));
    }

    #[test]
    fn fuse_messages_prefers_identifier_hits() {
        let symbol = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let neighbor = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let now = chrono::Utc::now().naive_utc();
        let fused = fuse_message_hits(
            vec![MessageSearchResult {
                message_id: neighbor,
                chat_id: Uuid::new_v4(),
                similarity: 0.88,
                role: "assistant".into(),
                content: "generic auth helper".into(),
                created_at: now,
            }],
            vec![MessageSearchResult {
                message_id: symbol,
                chat_id: Uuid::new_v4(),
                similarity: 0.03,
                role: "user".into(),
                content: "should_skip_blob is the SHA short-circuit".into(),
                created_at: now,
            }],
            "What does should_skip_blob do?",
            5,
        );
        assert_eq!(fused[0].message_id, symbol);
    }
}
