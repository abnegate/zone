/// Whether a registry offers `run_shell` beside `run_command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// `run_shell` too, for a context that is unrestricted.
    Included,
    /// `run_command` alone.
    Excluded,
}
