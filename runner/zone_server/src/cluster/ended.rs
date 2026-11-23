/// Why a subscribed pub/sub connection stopped serving.
pub enum Ended {
    Stopped,
    Lost(String),
}
