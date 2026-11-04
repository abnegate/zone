use serde::Serialize;

use super::selection::Selection;
use crate::services::provider::{Offer, Provider};

/// A chat's providers: its workspace's default, the one it chose, every one it
/// may choose, and the coding agent CLI its turns start on, none when they go
/// over HTTP or cannot run. The router may move a turn to another agent's login.
#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    pub default: Offer,
    pub selected: Option<Selection>,
    pub providers: Vec<Offer>,
    pub agent: Option<Provider>,
}
