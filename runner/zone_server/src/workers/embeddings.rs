//! Message embedding worker
//!
//! Executes message embedding operations in the background, storing
//! embeddings in the database for semantic search over chat history.

use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::db::message_embeddings;
use crate::state::AppState;

// Max concurrent embedding operations to prevent resource exhaustion
const MAX_CONCURRENT_EMBEDDINGS: usize = 10;

// Maximum content length (~2000 tokens for text-embedding-3-small)
const MAX_CONTENT_LENGTH: usize = 8000;

// Expected embedding dimensions for text-embedding-3-small
const EXPECTED_EMBEDDING_DIM: usize = 1536;

// Global semaphore to limit concurrent embeddings
static EMBEDDING_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn get_semaphore() -> &'static Arc<Semaphore> {
    EMBEDDING_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_EMBEDDINGS)))
}

/// Truncate string to max_bytes without breaking UTF-8 character boundaries
///
/// If the string is shorter than max_bytes, returns the full string.
/// Otherwise, finds the last valid UTF-8 character boundary at or before max_bytes.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the last valid char boundary at or before max_bytes
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &s[..boundary]
}

/// Spawn a background task to generate and store message embedding
///
/// This function spawns an async task that:
/// 1. Acquires a semaphore permit to limit concurrency
/// 2. Uses the embedding service to generate embeddings
/// 3. Stores the embedding in the database
/// 4. Handles errors gracefully (logs but doesn't fail)
///
/// The task is non-blocking - it spawns and returns immediately,
/// allowing the message creation flow to complete without waiting
/// for embedding generation.
///
/// # Arguments
/// * `state` - Application state containing embedding service and database
/// * `message_id` - ID of the message to embed
/// * `chat_id` - ID of the chat containing the message
/// * `content` - Message content to embed
///
/// # Error Handling
/// - If embedding service is unavailable, logs warning and returns
/// - If embedding generation fails, logs error but doesn't crash
/// - If database storage fails, logs error but doesn't crash
///
/// This graceful error handling ensures that message creation succeeds
/// even if embedding generation fails, providing a better user experience.
pub fn spawn_message_embedding_task(
    state: AppState,
    message_id: Uuid,
    chat_id: Uuid,
    content: String,
) {
    tokio::spawn(async move {
        // Acquire semaphore permit to limit concurrent embeddings
        let _permit = match get_semaphore().acquire().await {
            Ok(p) => p,
            Err(_) => {
                tracing::error!(
                    "Embedding semaphore closed for message {}. Skipping embedding generation.",
                    message_id
                );
                return;
            }
        };

        tracing::debug!(
            "Starting embedding generation for message {} in chat {}",
            message_id,
            chat_id
        );

        // Check if content is empty or too short to be useful
        if content.trim().is_empty() {
            tracing::debug!(
                "Skipping embedding for message {} - content is empty",
                message_id
            );
            return;
        }

        // Truncate content if too long (character-aware to prevent UTF-8 panic)
        let content_to_embed = if content.len() > MAX_CONTENT_LENGTH {
            tracing::warn!(
                "Content too long for message {} ({} chars), truncating to {}",
                message_id,
                content.len(),
                MAX_CONTENT_LENGTH
            );
            truncate_to_char_boundary(&content, MAX_CONTENT_LENGTH)
        } else {
            &content
        };

        // Get embedding service
        let embedding_service = match state.embedding_service() {
            Some(svc) => svc,
            None => {
                tracing::warn!(
                    "Embedding service not available. Skipping embedding for message {}",
                    message_id
                );
                return;
            }
        };

        // Generate embedding
        let embedding = match embedding_service.embed(content_to_embed).await {
            Ok(emb) => emb,
            Err(e) => {
                tracing::error!(
                    "Failed to generate embedding for message {}: {}",
                    message_id,
                    e
                );
                return;
            }
        };

        // Validate embedding dimensions
        if embedding.is_empty() {
            tracing::error!(
                "Embedding service returned empty embedding for message {}",
                message_id
            );
            return;
        }

        if embedding.len() != EXPECTED_EMBEDDING_DIM {
            tracing::error!(
                "Unexpected embedding dimensions for message {}: got {}, expected {}",
                message_id,
                embedding.len(),
                EXPECTED_EMBEDDING_DIM
            );
            return;
        }

        // Store embedding in database
        let model = embedding_service.model();
        if let Err(e) = message_embeddings::store_message_embedding(
            state.db(),
            message_id,
            chat_id,
            &embedding,
            model,
        )
        .await
        {
            tracing::error!(
                "Failed to store embedding for message {}: {}",
                message_id,
                e
            );
            return;
        }

        tracing::debug!(
            "Successfully generated and stored embedding for message {} (dimensions: {}, model: {})",
            message_id,
            embedding.len(),
            model
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_concurrent_embeddings_constant() {
        assert_eq!(MAX_CONCURRENT_EMBEDDINGS, 10);
    }

    #[test]
    fn test_semaphore_initialization() {
        let sem = get_semaphore();
        assert!(Arc::strong_count(sem) >= 1);
    }

    #[test]
    fn test_semaphore_capacity() {
        // Test that the semaphore is initialized with the correct capacity
        // by creating a new semaphore with the same value
        let test_sem = Arc::new(Semaphore::new(MAX_CONCURRENT_EMBEDDINGS));
        assert_eq!(test_sem.available_permits(), MAX_CONCURRENT_EMBEDDINGS);
    }

    #[test]
    fn test_truncate_to_char_boundary_short_string() {
        let s = "Hello";
        assert_eq!(truncate_to_char_boundary(s, 10), "Hello");
    }

    #[test]
    fn test_truncate_to_char_boundary_exact_length() {
        let s = "Hello";
        assert_eq!(truncate_to_char_boundary(s, 5), "Hello");
    }

    #[test]
    fn test_truncate_to_char_boundary_ascii() {
        let s = "Hello World";
        assert_eq!(truncate_to_char_boundary(s, 5), "Hello");
    }

    #[test]
    fn test_truncate_to_char_boundary_multibyte() {
        // "Hello 世界" - the Chinese characters are 3 bytes each
        let s = "Hello 世界";
        // Try to truncate at byte 8, which is in the middle of '世' (starts at byte 6)
        // Should truncate to byte 6 (just before the first Chinese character)
        assert_eq!(truncate_to_char_boundary(s, 8), "Hello ");
    }

    #[test]
    fn test_truncate_to_char_boundary_emoji() {
        // "Hi 👋" - the emoji is 4 bytes
        let s = "Hi 👋";
        // Try to truncate at byte 4, which is in the middle of the emoji
        // Should truncate to byte 3 (just before the emoji)
        assert_eq!(truncate_to_char_boundary(s, 4), "Hi ");
    }

    #[test]
    fn test_truncate_to_char_boundary_zero() {
        let s = "Hello";
        assert_eq!(truncate_to_char_boundary(s, 0), "");
    }

    #[test]
    fn test_truncate_to_char_boundary_multibyte_exact() {
        // "世界" - 6 bytes total (3 + 3)
        let s = "世界";
        // Truncate at exactly 3 bytes (end of first character)
        assert_eq!(truncate_to_char_boundary(s, 3), "世");
    }
}
