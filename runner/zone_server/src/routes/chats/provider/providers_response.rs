use serde::Serialize;

use super::selection::Selection;
use crate::services::provider::Offer;

/// A chat's providers: its workspace's default, the one it chose, every one it
/// may choose, and whether its turns run on a coding agent CLI: which one is the
/// router's pick as each turn starts, and no when they go over HTTP or cannot run.
#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    pub default: Offer,
    pub selected: Option<Selection>,
    pub providers: Vec<Offer>,
    pub cli: bool,
}
