use std::hash::Hash;
use std::hash::Hasher;

use uuid::Uuid;

/// Identifies one chunk across the keyword and semantic legs of a hybrid search.
///
/// Equality and hashing look at `chunk_id` alone, so a semantic row read from a
/// stale embedding still fuses with the keyword row for the same chunk.
#[derive(Debug, Clone, Copy)]
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

impl PartialEq for Id {
    fn eq(&self, other: &Self) -> bool {
        self.chunk_id == other.chunk_id
    }
}

impl Eq for Id {}

impl Hash for Id {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.chunk_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_chunk_alone_is_the_identity() {
        let current = Id::new(Uuid::from_u128(7), Uuid::from_u128(70), Uuid::from_u128(1));
        let stale = Id::new(Uuid::from_u128(7), Uuid::from_u128(71), Uuid::from_u128(2));
        let other = Id::new(Uuid::from_u128(8), Uuid::from_u128(70), Uuid::from_u128(1));

        assert_eq!(current, stale);
        assert_ne!(current, other);
        let identities: HashSet<Id> = [current, stale, other].into_iter().collect();
        assert_eq!(identities.len(), 2);
    }
}
