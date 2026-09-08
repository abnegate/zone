/// Ceilings applied while walking a nominated harness closure.
///
/// The closure is walked over a workspace the model influenced, across a graph
/// that same workspace describes, so the walk is bounded before it starts
/// rather than trusted to terminate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosureBounds {
    pub files: usize,
    pub path_bytes: usize,
    pub source_bytes: u64,
    pub total_bytes: u64,
}

impl ClosureBounds {
    pub const DEFAULT: Self = Self {
        files: 512,
        path_bytes: 1024,
        source_bytes: 1024 * 1024,
        total_bytes: 64 * 1024 * 1024,
    };
}

impl Default for ClosureBounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}
