//! Putting a knowledge entry into the semantic index.
//!
//! A row in `knowledge_embeddings` is what `search_knowledge` searches. An
//! entry without one is reachable by keyword and by nothing else, and the
//! difference is invisible in the entry itself. Creation and recovery both
//! index through [`index`] so an entry cannot be admitted to the index by one
//! rule and recovered by another.

use uuid::Uuid;
use zone_context::error::ContextError;

use crate::db::knowledge;
use crate::state::AppState;

/// Why an entry is not in the semantic index.
///
/// The two failures keep the wording the creation path logged before they were
/// named, because `Failed to generate knowledge embedding` is the line an
/// operator greps a server log for.
#[derive(Debug, thiserror::Error)]
pub enum Unindexed {
    #[error("No embedding service is configured")]
    NoService,
    #[error("Failed to generate knowledge embedding: {0}")]
    Embed(#[from] ContextError),
    #[error("Failed to store knowledge embedding: {0}")]
    Store(#[from] sqlx::Error),
}

/// Embed an entry's content and store the vector semantic search reads.
pub async fn index(
    state: &AppState,
    entry_id: Uuid,
    workspace_id: Uuid,
    content: &str,
) -> Result<(), Unindexed> {
    let service = state.embedding_service().ok_or(Unindexed::NoService)?;
    let vector = service.embed(content).await?;

    knowledge::store_knowledge_embedding(
        state.db(),
        entry_id,
        workspace_id,
        &vector,
        service.model(),
    )
    .await?;

    Ok(())
}
