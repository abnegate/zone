/// How much of a conversation's images one request to an agent may carry.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Base64 bytes in one image.
    pub image_bytes: usize,
    pub images: usize,
    /// Bytes in the whole request.
    pub request_bytes: usize,
}

impl Limits {
    /// The Claude API's, with the smallest context window's image count and
    /// room in the request for what Claude Code adds to it.
    pub const CLAUDE: Self = Self {
        image_bytes: 10_000_000,
        images: 100,
        request_bytes: 127_000_000,
    };
}
