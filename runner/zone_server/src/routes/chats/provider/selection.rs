use serde::Serialize;

use crate::services::provider::Offer;

/// The provider a chat chose, as it is offered now, and the model it named.
#[derive(Debug, Serialize)]
pub struct Selection {
    #[serde(flatten)]
    pub offer: Offer,
    pub model: Option<String>,
}
