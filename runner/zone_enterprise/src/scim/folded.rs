use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::scim::folded_visitor::FoldedVisitor;

/// A JSON value whose objects keep, of the keys that differ only in ASCII case, the last one in
/// document order, since SCIM attribute names are case-insensitive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Folded(pub(crate) Value);

impl<'de> Deserialize<'de> for Folded {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FoldedVisitor)
    }
}
