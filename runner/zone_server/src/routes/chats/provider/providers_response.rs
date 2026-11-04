use serde::Serialize;

use super::selection::Selection;
use crate::services::provider::{Offer, Provider};

/// A chat's providers: its workspace's default, the one it chose, every one it
/// may choose, and the coding agent CLI its turns run on, none when they go
/// over HTTP.
#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    pub default: Offer,
    pub selected: Option<Selection>,
    pub providers: Vec<Offer>,
    pub agent: Option<Provider>,
}
