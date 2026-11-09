use abnegate_index::Candidate;
use uuid::Uuid;

use super::id::Id;

/// Internal result from keyword search
#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct KeywordResult {
    pub(super) chunk_id: Uuid,
    pub(super) content_item_id: Uuid,
    pub(super) source_id: Uuid,
    pub(super) chunk_text: String,
    pub(super) item_uri: String,
    pub(super) item_title: String,
    pub(super) score: f32,
}

impl KeywordResult {
    pub(super) fn id(&self) -> Id {
        Id::new(self.chunk_id, self.content_item_id, self.source_id)
    }

    pub(super) fn into_candidate(self) -> Candidate<Id> {
        Candidate::new(
            self.id(),
            self.chunk_text,
            self.item_uri,
            self.item_title,
            self.score,
        )
    }
}
