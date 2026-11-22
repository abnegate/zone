use super::floor::Floor;

/// The turn in flight on another replica, as a connection joining it is shown.
#[derive(Debug, Clone)]
pub struct Replay {
    pub frames: Vec<String>,
    pub floor: Floor,
}
