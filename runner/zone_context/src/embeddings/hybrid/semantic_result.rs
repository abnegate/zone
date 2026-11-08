use abnegate_index::Candidate;
use uuid::Uuid;

use super::id::Id;

/// Internal result from semantic search
#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct SemanticResult {
    pub(super) chunk_id: Uuid,
    pub(super) content_item_id: Uuid,
    pub(super) source_id: Uuid,
    pub(super) chunk_text: String,
    pub(super) item_uri: String,
    pub(super) item_title: String,
    pub(super) similarity: f32,
}

impl SemanticResult {
    pub(super) fn id(&self) -> Id {
        Id {
            chunk_id: self.chunk_id,
            content_item_id: self.content_item_id,
            source_id: self.source_id,
        }
    }

    pub(super) fn into_candidate(self) -> Candidate<Id> {
        Candidate::new(
            self.id(),
            self.chunk_text,
            self.item_uri,
            self.item_title,
            self.similarity,
        )
    }
}
