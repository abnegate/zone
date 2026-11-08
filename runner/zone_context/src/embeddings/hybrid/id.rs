use uuid::Uuid;

/// Identifies one chunk across the keyword and semantic legs of a hybrid search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id {
    pub chunk_id: Uuid,
    pub content_item_id: Uuid,
    pub source_id: Uuid,
}
