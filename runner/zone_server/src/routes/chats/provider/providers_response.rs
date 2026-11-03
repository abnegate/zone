use serde::Serialize;

use super::selection::Selection;
use crate::services::provider::Offer;

/// A chat's providers: its workspace's default, the one it chose, and every
/// one it may choose.
#[derive(Debug, Serialize)]
pub struct ProvidersResponse {
    pub default: Offer,
    pub selected: Option<Selection>,
    pub providers: Vec<Offer>,
}
