//! Which surface a prompt is being assembled for.

/// The two surfaces that receive a built system prompt.
///
/// A chat turn answers a person who can reply; a task run works alone against
/// a checkout. Sections that only make sense on one of them branch on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Chat,
    Task,
}
