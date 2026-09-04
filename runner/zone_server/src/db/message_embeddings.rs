//! Message embedding storage and retrieval
//!
//! Provides functions for storing and searching message embeddings
//! for semantic search over chat history.

use sqlx::PgPool;
use uuid::Uuid;

/// Store an embedding for a message
///
/// This function stores the embedding vector for a chat message,
/// enabling semantic search over conversation history.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `message_id` - ID of the message
/// * `chat_id` - ID of the chat containing the message
/// * `embedding` - The embedding vector (should be 1536 dimensions)
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
    // Validate that all embedding values are finite
    if !embedding.iter().all(|&v| v.is_finite()) {
        return Err(sqlx::Error::Protocol(
            "Embedding contains non-finite values (NaN or infinity)".to_string(),
        ));
    }

    // Convert embedding to pgvector format
    let vector_str = format!(
        "[{}]",
        embedding
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

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
    // Validate that all embedding values are finite
    if !query_embedding.iter().all(|&v| v.is_finite()) {
        return Err(sqlx::Error::Protocol(
            "Query embedding contains non-finite values (NaN or infinity)".to_string(),
        ));
    }

    // Convert embedding to pgvector format
    let vector_str = format!(
        "[{}]",
        query_embedding
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

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
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Message embedding record
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageEmbedding {
    pub id: Uuid,
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub model: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
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
            created_at: chrono::Utc::now(),
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
            created_at: chrono::Utc::now(),
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
}
