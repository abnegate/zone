use uuid::Uuid;

/// Identifies one chunk across the keyword and semantic legs of a hybrid search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id {
    pub chunk_id: Uuid,
    pub content_item_id: Uuid,
    pub source_id: Uuid,
}

impl Id {
    pub fn new(chunk_id: Uuid, content_item_id: Uuid, source_id: Uuid) -> Self {
        Self {
            chunk_id,
            content_item_id,
            source_id,
        }
    }
}
