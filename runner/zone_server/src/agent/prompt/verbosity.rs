//! How long an answer runs before the user's request says otherwise.

use super::Surface;

/// The default answer length for a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Brief,
    Standard,
}

impl Verbosity {
    /// A chat turn is read in a message pane; a task run writes a record.
    pub fn of(surface: Surface) -> Self {
        match surface {
            Surface::Chat => Self::Brief,
            Surface::Task => Self::Standard,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_is_brief_and_a_task_run_is_standard() {
        assert_eq!(Verbosity::of(Surface::Chat), Verbosity::Brief);
        assert_eq!(Verbosity::of(Surface::Task), Verbosity::Standard);
    }
}
