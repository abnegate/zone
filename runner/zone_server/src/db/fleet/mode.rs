/// Unattended work (task runs, reminder turns, auto-project drives) needs an active runner that
/// carries the required labels. Interactive work (chat turns) also runs on a cordoned runner and
/// ignores labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Unattended,
    Interactive,
}
