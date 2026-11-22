use super::ServerMessage;

/// What applying a command did: the frames meant only for the connection
/// that sent it, and whether a turn started at once.
#[derive(Debug, Default)]
pub(super) struct Handled {
    pub frames: Vec<ServerMessage>,
    pub started: bool,
}
