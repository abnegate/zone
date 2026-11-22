use std::time::Duration;

/// One streamed completion: its content chunks, with `pause` before each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub chunks: Vec<String>,
    pub pause: Duration,
}

impl Reply {
    pub fn new(chunks: impl IntoIterator<Item = impl Into<String>>, pause: Duration) -> Self {
        Self {
            chunks: chunks.into_iter().map(Into::into).collect(),
            pause,
        }
    }

    /// `count` distinct chunks, `"word0 word1 ..."` when put together.
    pub fn counted(count: usize, pause: Duration) -> Self {
        Self::new((0..count).map(|index| format!("word{index} ")), pause)
    }

    /// What the chunks add up to.
    pub fn text(&self) -> String {
        self.chunks.concat()
    }

    /// How long the whole reply takes to stream.
    pub fn duration(&self) -> Duration {
        self.pause * u32::try_from(self.chunks.len()).unwrap_or(u32::MAX)
    }
}
