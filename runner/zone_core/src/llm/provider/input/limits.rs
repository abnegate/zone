/// How much of a conversation's images one request to an agent may carry.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub image_bytes: usize,
    pub images: usize,
    pub request_bytes: usize,
}

impl Limits {
    /// The Claude API's: base64 bytes in one image, the smallest context
    /// window's image count, and the request's bytes less room for what
    /// Claude Code adds to it.
    pub const CLAUDE: Self = Self {
        image_bytes: 10_000_000,
        images: 100,
        request_bytes: 127_000_000,
    };
}
