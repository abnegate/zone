/// A piece of a rendered conversation, in the order the agent reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// An image attached to the message rendered before it. `current` marks
    /// the images of the last user message, the ones this turn was sent with.
    Image {
        url: String,
        current: bool,
    },
}
