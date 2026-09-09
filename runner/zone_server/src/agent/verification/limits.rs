/// Byte and count ceilings applied to an untrusted verification marker.
///
/// A marker arrives inside model prose, so every field is bounded before it is
/// parsed rather than after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub field_bytes: usize,
    pub final_message_bytes: usize,
    pub marker_bytes: usize,
    pub recipes: usize,
    pub terms: usize,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        field_bytes: 256,
        final_message_bytes: 64 * 1024,
        marker_bytes: 32 * 1024,
        recipes: 32,
        terms: 8,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
