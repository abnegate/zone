/// Where a connection's command went.
#[derive(Debug)]
pub enum Route {
    /// Nobody else runs the chat's turn, so this replica applies it.
    Local,
    /// The replica serving the chat took the command, and answered with
    /// these serialized frames for the asking connection alone.
    Forwarded(Vec<String>),
    /// Another replica holds the chat and did not apply the command.
    Busy,
}
