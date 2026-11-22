use super::acknowledgement::Acknowledgement;

/// Where a connection's command went.
#[derive(Debug)]
pub enum Route {
    /// Nobody else runs the chat's turn, so this replica applies it.
    Local,
    Forwarded(Acknowledgement),
    /// Another replica holds the chat but did not answer.
    Busy,
}
